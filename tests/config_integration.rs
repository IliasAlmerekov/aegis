use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn aegis_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aegis"))
}

fn base_command(home: &Path) -> Command {
    let mut command = Command::new(aegis_bin());
    command.env("AEGIS_REAL_SHELL", "/bin/sh");
    command.env("AEGIS_CI", "0");
    command.env("HOME", home);
    command
}

fn read_audit_entries(home: &Path) -> Vec<Value> {
    let path = home.join(".aegis").join("audit.jsonl");
    let contents = fs::read_to_string(path).unwrap();

    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect()
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();

    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
}

fn init_git_repo(path: &Path) {
    Command::new("git")
        .arg("init")
        .current_dir(path)
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "-c",
            "user.email=test@aegis.dev",
            "-c",
            "user.name=Aegis Test",
            "commit",
            "--allow-empty",
            "-m",
            "init",
        ])
        .current_dir(path)
        .output()
        .unwrap();
}

#[test]
fn custom_patterns_from_config_change_runtime_decision_and_source_label() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
[[custom_patterns]]
id = "USR-CONFIG-001"
category = "Process"
risk = "Warn"
pattern = "echo\\s+hello"
description = "Treat hello echo as suspicious"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", "echo hello", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["risk"], "warn");
    assert_eq!(json["decision"], "prompt");
    assert_eq!(json["matched_patterns"][0]["id"], "USR-CONFIG-001");
    assert_eq!(json["matched_patterns"][0]["source"], "custom");
}

#[test]
fn mode_config_changes_runtime_outcome_for_same_warn_command() {
    let home = TempDir::new().unwrap();
    let protect_workspace = TempDir::new().unwrap();
    let strict_workspace = TempDir::new().unwrap();

    fs::write(
        protect_workspace.path().join(".aegis.toml"),
        r#"
mode = "Protect"
auto_snapshot_git = false
auto_snapshot_docker = false
"#,
    )
    .unwrap();
    fs::write(
        strict_workspace.path().join(".aegis.toml"),
        r#"
mode = "Strict"
auto_snapshot_git = false
auto_snapshot_docker = false
"#,
    )
    .unwrap();

    let protect_output = base_command(home.path())
        .current_dir(protect_workspace.path())
        .stdin(Stdio::null())
        .args(["-c", "git stash clear"])
        .output()
        .unwrap();

    let strict_output = base_command(home.path())
        .current_dir(strict_workspace.path())
        .stdin(Stdio::null())
        .args(["-c", "git stash clear"])
        .output()
        .unwrap();

    assert_eq!(protect_output.status.code(), Some(2));
    assert_eq!(strict_output.status.code(), Some(3));

    let protect_entries = read_audit_entries(home.path());
    assert_eq!(protect_entries.len(), 2);
    assert_eq!(protect_entries[0]["decision"], "Denied");
    assert_eq!(protect_entries[0]["risk"], "Warn");
    assert_eq!(protect_entries[1]["decision"], "Blocked");
    assert_eq!(protect_entries[1]["risk"], "Warn");
}

#[test]
fn allowlist_cwd_scope_only_applies_inside_matching_workspace() {
    let home = TempDir::new().unwrap();
    let allowed_workspace = TempDir::new().unwrap();
    let denied_workspace = TempDir::new().unwrap();
    let bin_dir = home.path().join("bin");
    let log_path = home.path().join("terraform.log");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("terraform"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_TERRAFORM_LOG"
exit 0
"#,
    );

    let allowed_config = format!(
        r#"
allowlist_override_level = "Danger"
auto_snapshot_git = false
auto_snapshot_docker = false
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{}"
reason = "cwd-scoped allowlist"
"#,
        allowed_workspace.path().display()
    );
    fs::write(
        allowed_workspace.path().join(".aegis.toml"),
        &allowed_config,
    )
    .unwrap();
    fs::write(denied_workspace.path().join(".aegis.toml"), &allowed_config).unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let allowed_output = base_command(home.path())
        .current_dir(allowed_workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    let denied_output = base_command(home.path())
        .current_dir(denied_workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .stdin(Stdio::null())
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    assert!(allowed_output.status.success());
    assert_eq!(denied_output.status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(&log_path).unwrap(),
        "destroy -target=module.test.api\n"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["allowlist_effective"], true);
    assert_eq!(entries[1]["decision"], "Denied");
    assert_eq!(entries[1]["allowlist_effective"], false);
}

#[test]
fn snapshot_flags_toggle_git_plugin_for_dangerous_allowlisted_command() {
    let home = TempDir::new().unwrap();
    let enabled_workspace = TempDir::new().unwrap();
    let disabled_workspace = TempDir::new().unwrap();
    let bin_dir = home.path().join("bin-snapshot");
    let log_path = home.path().join("snapshot-terraform.log");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("terraform"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_TERRAFORM_LOG"
exit 0
"#,
    );

    init_git_repo(enabled_workspace.path());
    init_git_repo(disabled_workspace.path());
    fs::write(enabled_workspace.path().join("dirty.txt"), "enabled\n").unwrap();
    fs::write(disabled_workspace.path().join("dirty.txt"), "disabled\n").unwrap();

    fs::write(
        enabled_workspace.path().join(".aegis.toml"),
        r#"
mode = "Strict"
allowlist_override_level = "Danger"
auto_snapshot_git = true
auto_snapshot_docker = false
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
reason = "snapshot enabled"
"#,
    )
    .unwrap();
    fs::write(
        disabled_workspace.path().join(".aegis.toml"),
        r#"
mode = "Strict"
allowlist_override_level = "Danger"
auto_snapshot_git = false
auto_snapshot_docker = false
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
reason = "snapshot disabled"
"#,
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let enabled_output = base_command(home.path())
        .current_dir(enabled_workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    let disabled_output = base_command(home.path())
        .current_dir(disabled_workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    assert!(enabled_output.status.success());
    assert!(disabled_output.status.success());

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["snapshots"][0]["plugin"], "git");
    assert_eq!(entries[1]["decision"], "AutoApproved");
    assert_eq!(entries[1]["snapshots"], serde_json::json!([]));
}
