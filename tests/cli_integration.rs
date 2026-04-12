use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use tempfile::TempDir;

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

#[test]
fn shell_wrapper_command_flag_executes_command_with_stdio_and_exit_code() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "printf \"hello\\n\"; exit 0"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"hello\n");
    assert!(output.stderr.is_empty());

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "AutoApproved");
}

#[test]
fn shell_wrapper_command_flag_preserves_child_exit_status() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "printf hi; exit 42"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(42));
    assert_eq!(output.stdout, b"hi");
    assert!(output.stderr.is_empty());
}

#[test]
fn malformed_project_config_load_is_fail_closed_and_prevents_execution() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let sentinel = workspace.path().join("sentinel.txt");
    let config_path = workspace.path().join(".aegis.toml");

    fs::write(&config_path, "mode = <<<THIS IS NOT VALID TOML\n").unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", &format!("printf 'oops' > {}", sentinel.display())])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(
        !sentinel.exists(),
        "command must not execute on config load failure"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: failed to load config"));
    assert!(stderr.contains(&config_path.display().to_string()));
    assert!(stderr.contains("Fix or remove the invalid config file"));
}

#[test]
fn evaluation_json_reports_prompt_decision_and_does_not_execute_command() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args([
            "-c",
            "terraform destroy -target=module.prod.api",
            "--output",
            "json",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stderr.is_empty(),
        "json mode should keep stderr empty"
    );

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["decision"], "prompt");
    assert_eq!(json["exit_code"], 2);
    assert_eq!(json["risk"], "danger");
    assert_eq!(json["execution"]["mode"], "evaluation_only");
    assert_eq!(json["execution"]["will_execute"], false);
}

#[test]
fn audit_entry_is_appended_for_execution_and_denial_decisions() {
    let home = TempDir::new().unwrap();

    let safe_output = base_command(home.path())
        .args(["-c", "printf safe"])
        .output()
        .unwrap();
    assert!(safe_output.status.success());

    let denied_output = base_command(home.path())
        .stdin(Stdio::null())
        .args(["-c", "git stash clear"])
        .output()
        .unwrap();

    assert_eq!(denied_output.status.code(), Some(2));

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["command"], "printf safe");
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[1]["command"], "git stash clear");
    assert_eq!(entries[1]["decision"], "Denied");
}

#[test]
fn ci_policy_block_prevents_execution_when_detected() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        "ci_policy = \"Block\"\n",
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .env("AEGIS_CI", "1")
        .args(["-c", "git stash clear"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("blocked by CI policy"));
    assert!(stderr.contains("allowlist"));

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "Blocked");
    assert_eq!(entries[0]["risk"], "Warn");
}

#[test]
fn ci_environment_is_reflected_in_evaluation_json() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        "ci_policy = \"Block\"\n",
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .env("AEGIS_CI", "1")
        .args(["-c", "safe-command --flag", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ci_state"]["detected"], true);
    assert_eq!(json["ci_state"]["policy"], "block");
    assert_eq!(json["decision"], "auto_approve");
}
