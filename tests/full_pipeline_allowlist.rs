//! Allowlist policy: danger/warn override auto-approval, ceiling enforcement,
//! verbose/quiet diagnostics, scoped-rule runtime rejection, and CI/Strict
//! allowlist interactions.
//!
//! Split from the original `tests/full_pipeline.rs` (behavior-preserving move).

mod support;

use std::fs;
use std::io::Write;
use std::process::Stdio;

use tempfile::TempDir;

use support::*;

#[test]
fn allowlisted_terraform_destroy_with_danger_override_skips_dialog_but_other_targets_are_denied() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");
    let log_path = workspace.path().join("terraform.log");
    let config_path = workspace.path().join(".aegis.toml");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("terraform"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_TERRAFORM_LOG"
exit 0
"#,
    );
    let workspace_cwd = workspace
        .path()
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    write_global_config(home.path(), "allowlist_override_level = \"Danger\"\n");
    fs::write(
        &config_path,
        format!(
            r#"
[[allow]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{workspace_cwd}"
reason = "test allowlist"
"#
        ),
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let allowed_output = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    assert!(
        allowed_output.status.success(),
        "allowlisted terraform destroy must succeed; status: {:?}\nstderr:\n{}",
        allowed_output.status.code(),
        String::from_utf8_lossy(&allowed_output.stderr),
    );
    assert!(!String::from_utf8_lossy(&allowed_output.stderr).contains("AEGIS INTERCEPTED"));
    assert_eq!(
        fs::read_to_string(&log_path).unwrap(),
        "destroy -target=module.test.api\n"
    );

    // AEGIS_FORCE_INTERACTIVE=1 lets the test pipe "no\n" to simulate a
    // human denying the dangerous non-allowlisted command.
    let mut denied_child = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .env("AEGIS_FORCE_INTERACTIVE", "1")
        .args(["-c", "terraform destroy -target=module.prod.api"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    denied_child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"no\n")
        .unwrap();

    let denied_output = denied_child.wait_with_output().unwrap();

    assert_eq!(denied_output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&denied_output.stderr)
            .contains("AEGIS INTERCEPTED A DANGEROUS COMMAND")
    );
    assert_eq!(
        fs::read_to_string(&log_path).unwrap(),
        "destroy -target=module.test.api\n"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Danger");
    assert!(entries[0]["pattern_ids"].as_array().is_some());
    assert_eq!(entries[0]["mode"], "Protect");
    assert_eq!(entries[0]["ci_detected"], serde_json::json!(false));
    assert_eq!(entries[0]["allowlist_matched"], serde_json::json!(true));
    assert_eq!(entries[0]["allowlist_effective"], serde_json::json!(true));
    // The audit log must record which allowlist rule fired so operators can
    // trace auto-approvals back to their config.
    assert_eq!(
        entries[0]["allowlist_pattern"],
        "terraform destroy -target=module.test.*"
    );
    assert_eq!(entries[1]["decision"], "Denied");
    assert_eq!(entries[1]["risk"], "Danger");
    assert!(entries[1]["pattern_ids"].as_array().is_some());
    assert_eq!(entries[1]["mode"], "Protect");
    assert_eq!(entries[1]["ci_detected"], serde_json::json!(false));
    assert_eq!(entries[1]["allowlist_matched"], serde_json::json!(false));
    assert_eq!(entries[1]["allowlist_effective"], serde_json::json!(false));
    // Non-matching command — allowlist_pattern field must be absent from JSON.
    assert!(entries[1].get("allowlist_pattern").is_none());
}

#[test]
fn protect_mode_allowlisted_danger_without_danger_override_is_denied_non_interactive() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");
    let log_path = workspace.path().join("terraform.log");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("terraform"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_TERRAFORM_LOG"
exit 0
"#,
    );
    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
mode = "Protect"
allowlist_override_level = "Warn"
auto_snapshot_git = false
auto_snapshot_docker = false
[[allow]]
pattern = "terraform destroy -target=module.test.*"
cwd = "/aegis-test-scope"
reason = "protect warn ceiling"
"#,
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .stdin(Stdio::null())
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(
        read_stub_invocations(&log_path).is_empty(),
        "Protect mode must not auto-approve allowlisted Danger without Danger override"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "Denied");
    assert_eq!(entries[0]["risk"], "Danger");
    assert!(entries[0].get("allowlist_pattern").is_none());
    assert!(entries[0].get("allowlist_reason").is_none());
}

/// When verbose mode is on and a command matches the allowlist, stderr must
/// include a message identifying which rule fired.
#[test]
fn verbose_allowlist_match_prints_rule_name() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(&bin_dir.join("terraform"), "#!/bin/sh\nexit 0\n");
    let workspace_cwd = workspace
        .path()
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    write_global_config(home.path(), "allowlist_override_level = \"Danger\"\n");
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
[[allow]]
pattern = "terraform destroy -target=module.ci.*"
cwd = "{workspace_cwd}"
reason = "verbose allowlist test"
"#
        ),
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .args(["-v", "-c", "terraform destroy -target=module.ci.api"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "verbose allowlist match must succeed; status: {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("allowlist"),
        "verbose output must mention 'allowlist'; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("terraform destroy -target=module.ci.*"),
        "verbose output must include the matched rule; stderr:\n{stderr}"
    );
}

#[test]
fn quiet_allowlist_match_suppresses_aegis_diagnostics() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(&bin_dir.join("terraform"), "#!/bin/sh\nexit 0\n");
    let workspace_cwd = workspace
        .path()
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    write_global_config(home.path(), "allowlist_override_level = \"Danger\"\n");
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
[[allow]]
pattern = "terraform destroy -target=module.ci.*"
cwd = "{workspace_cwd}"
reason = "quiet allowlist test"
"#
        ),
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .args(["--quiet", "-c", "terraform destroy -target=module.ci.api"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "quiet allowlist match must succeed silently; status: {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        output.stderr.is_empty(),
        "quiet mode must suppress Aegis diagnostics on stderr"
    );
}

#[test]
fn verbosity_verbose_allowlist_match_prints_rule_name() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(&bin_dir.join("terraform"), "#!/bin/sh\nexit 0\n");
    let workspace_cwd = workspace
        .path()
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    write_global_config(home.path(), "allowlist_override_level = \"Danger\"\n");
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
[[allow]]
pattern = "terraform destroy -target=module.ci.*"
cwd = "{workspace_cwd}"
reason = "verbosity verbose test"
"#
        ),
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .args([
            "--verbosity",
            "verbose",
            "-c",
            "terraform destroy -target=module.ci.api",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "verbose allowlist match must succeed; status: {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("allowlist"),
        "verbose output must mention 'allowlist'; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("terraform destroy -target=module.ci.*"),
        "verbose output must include the matched rule; stderr:\n{stderr}"
    );
}

#[test]
fn unscoped_structured_allowlist_fails_runtime_execution() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
[[allow]]
pattern = "terraform destroy *"
reason = "too broad"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", "printf should-not-run"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("must declare cwd or user scope"));
}

/// Protect mode + CI policy Block + allowlisted Danger command must
/// auto-approve (allowlist wins over CI policy in Protect mode).
#[test]
fn protect_ci_allowlisted_danger_executes_and_logs_autoapproved() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");
    let log_path = workspace.path().join("terraform.log");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("terraform"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_TERRAFORM_LOG"
exit 0
"#,
    );
    let workspace_cwd = workspace
        .path()
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    write_global_config(home.path(), "allowlist_override_level = \"Danger\"\n");
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
mode = "Protect"
ci_policy = "Block"
auto_snapshot_git = false
auto_snapshot_docker = false
[[allow]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{workspace_cwd}"
reason = "protect allowlist"
"#
        ),
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .env("AEGIS_CI", "1")
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "protect+ci+allowlisted danger must auto-approve; status: {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        fs::read_to_string(&log_path).unwrap(),
        "destroy -target=module.test.api\n"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Danger");
}

#[test]
fn structured_allowlist_warn_override_autoapproves_warn_but_not_danger() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");
    let git_log = workspace.path().join("git.log");
    let terraform_log = workspace.path().join("terraform.log");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("git"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_GIT_LOG"
exit 0
"#,
    );
    write_executable(
        &bin_dir.join("terraform"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_TERRAFORM_LOG"
exit 0
"#,
    );
    let workspace_cwd = workspace
        .path()
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
mode = "Strict"
allowlist_override_level = "Warn"
auto_snapshot_git = false
auto_snapshot_docker = false
[[allow]]
pattern = "*"
cwd = "{workspace_cwd}"
reason = "structured ceiling test"
"#
        ),
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let allowed_output = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_GIT_LOG", &git_log)
        .args(["-c", "git stash clear"])
        .output()
        .unwrap();

    assert!(
        allowed_output.status.success(),
        "allowlisted warn must auto-approve; status: {:?}\nstderr:\n{}",
        allowed_output.status.code(),
        String::from_utf8_lossy(&allowed_output.stderr),
    );
    assert_eq!(fs::read_to_string(&git_log).unwrap(), "stash clear\n");

    let denied_output = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &terraform_log)
        .stdin(Stdio::null())
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    assert!(
        !denied_output.status.success(),
        "Warn ceiling must not auto-approve Danger commands"
    );
    assert!(
        !terraform_log.exists(),
        "Warn ceiling must not auto-approve Danger commands"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Warn");
    assert_eq!(entries[0]["allowlist_pattern"], "*");
    assert_eq!(entries[1]["decision"], "Blocked");
    assert_eq!(entries[1]["risk"], "Danger");
    assert!(entries[1].get("allowlist_pattern").is_none());
    assert!(entries[1].get("allowlist_reason").is_none());
}

#[test]
fn structured_allowlist_danger_override_autoapproves_danger_and_logs_rule_reason() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");
    let log_path = workspace.path().join("terraform.log");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("terraform"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_TERRAFORM_LOG"
exit 0
"#,
    );
    let workspace_cwd = workspace
        .path()
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    write_global_config(home.path(), "allowlist_override_level = \"Danger\"\n");
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
mode = "Strict"
auto_snapshot_git = false
auto_snapshot_docker = false
[[allow]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{workspace_cwd}"
reason = "ephemeral test teardown"
"#
        ),
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "allowlisted danger must auto-approve; status: {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        fs::read_to_string(&log_path).unwrap(),
        "destroy -target=module.test.api\n"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Danger");
    assert_eq!(
        entries[0]["allowlist_pattern"],
        "terraform destroy -target=module.test.*"
    );
    assert_eq!(entries[0]["allowlist_reason"], "ephemeral test teardown");
}
