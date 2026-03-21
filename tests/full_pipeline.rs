use std::fs;
use std::io::Write;
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

#[test]
fn dangerous_command_denied_preserves_directory() {
    let home = TempDir::new().unwrap();
    let target_dir = std::env::temp_dir().join("test_aegis");

    let _ = fs::remove_dir_all(&target_dir);
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(target_dir.join("sentinel.txt"), "still here").unwrap();

    let mut child = base_command(home.path())
        .args(["-c", &format!("rm -rf {}", target_dir.display())])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.as_mut().unwrap().write_all(b"no\n").unwrap();

    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(
        target_dir.exists(),
        "directory should still exist after denying the command"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("AEGIS INTERCEPTED A DANGEROUS COMMAND"));
    assert!(stderr.contains("Command cancelled."));

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "Denied");
    assert_eq!(entries[0]["risk"], "Danger");

    let _ = fs::remove_dir_all(&target_dir);
}

#[test]
fn safe_command_passthroughs_stdout_and_exit_code() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "printf hello"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(output.stdout, b"hello");
    assert!(output.stderr.is_empty());

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Safe");
}

#[test]
fn allowlisted_terraform_destroy_skips_dialog_but_other_targets_are_denied() {
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
    fs::write(
        &config_path,
        r#"allowlist = ["terraform destroy -target=module.test.*"]
"#,
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

    assert!(allowed_output.status.success());
    assert!(!String::from_utf8_lossy(&allowed_output.stderr).contains("AEGIS INTERCEPTED"));
    assert_eq!(
        fs::read_to_string(&log_path).unwrap(),
        "destroy -target=module.test.api\n"
    );

    let mut denied_child = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
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

    assert_eq!(denied_output.status.code(), Some(1));
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
    assert_eq!(entries[1]["decision"], "Denied");
    assert_eq!(entries[1]["risk"], "Danger");
}

#[test]
fn safe_command_passthroughs_stderr_and_exit_code() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "printf boom >&2; exit 42"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(42));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"boom");

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Safe");
}
