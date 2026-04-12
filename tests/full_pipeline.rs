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
    // These end-to-end tests exercise the normal interactive/non-interactive
    // product flow, not the CI fast-path. Force CI detection off so host
    // environments like GitHub Actions do not change the expected exit codes.
    command.env("AEGIS_CI", "0");
    command.env("HOME", home);
    command
}

fn direct_shell_command(home: &Path) -> Command {
    let mut command = Command::new("/bin/sh");
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

/// Read the invocation log written by a PATH-stub executable.
///
/// Returns the lines recorded by the stub, or an empty `Vec` when the log
/// file does not exist (meaning the stub was never called).
fn read_stub_invocations(log_path: &Path) -> Vec<String> {
    match fs::read_to_string(log_path) {
        Ok(contents) => contents
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_owned)
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn dangerous_command_denied_preserves_directory() {
    let home = TempDir::new().unwrap();
    let target_dir = std::env::temp_dir().join("test_aegis");

    let _ = fs::remove_dir_all(&target_dir);
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(target_dir.join("sentinel.txt"), "still here").unwrap();

    // Use home as CWD (not the project root) so the GitPlugin does not
    // git-stash the developer's uncommitted changes as a "snapshot".
    // AEGIS_FORCE_INTERACTIVE=1 lets the test pipe "no\n" as if a human
    // were at the keyboard, so the full interactive dialog is exercised.
    let mut child = base_command(home.path())
        .current_dir(home.path())
        .env("AEGIS_FORCE_INTERACTIVE", "1")
        .args(["-c", &format!("rm -rf {}", target_dir.display())])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.as_mut().unwrap().write_all(b"no\n").unwrap();

    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code(), Some(2));
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
    assert_eq!(entries[0]["pattern_ids"], serde_json::json!([]));
    assert_eq!(entries[0]["mode"], "Protect");
    assert_eq!(entries[0]["ci_detected"], serde_json::json!(false));
    assert_eq!(entries[0]["allowlist_matched"], serde_json::json!(false));
    assert_eq!(entries[0]["allowlist_effective"], serde_json::json!(false));
}

#[test]
fn shell_wrapper_echo_hello_prints_expected_output_and_exit_code() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "echo hello"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"hello\n");
    assert!(output.stderr.is_empty());

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Safe");
}

#[test]
fn shell_wrapper_exit_42_preserves_exit_status() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "exit 42"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(42));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Safe");
}

#[test]
fn json_output_safe_command_returns_single_evaluation_object_without_exec_or_audit() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "safe-command --flag", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "stderr must stay empty in json mode"
    );

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["command"], "safe-command --flag");
    assert_eq!(json["risk"], "safe");
    assert_eq!(json["decision"], "auto_approve");
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["mode"], "protect");
    assert_eq!(json["matched_patterns"], serde_json::json!([]));
    assert_eq!(json["snapshots_created"], serde_json::json!([]));
    assert_eq!(json["allowlist_match"]["matched"], false);
    assert_eq!(json["allowlist_match"]["effective"], false);
    assert_eq!(json["snapshot_plan"]["requested"], false);
    assert_eq!(
        json["snapshot_plan"]["applicable_plugins"],
        serde_json::json!([])
    );
    assert_eq!(json["ci_state"]["detected"], false);
    assert_eq!(json["ci_state"]["policy"], "block");
    assert_eq!(json["execution"]["mode"], "evaluation_only");
    assert_eq!(json["execution"]["will_execute"], false);
    assert!(
        !home.path().join(".aegis").join("audit.jsonl").exists(),
        "evaluation-only json mode must not append an audit entry"
    );
}

#[test]
fn json_output_danger_command_returns_prompt_decision_without_stderr_or_audit() {
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
        "machine consumers must not parse stderr"
    );

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["command"], "terraform destroy -target=module.prod.api");
    assert_eq!(json["risk"], "danger");
    assert_eq!(json["decision"], "prompt");
    assert_eq!(json["exit_code"], 2);
    assert_eq!(json["allowlist_match"]["matched"], false);
    assert_eq!(json["allowlist_match"]["effective"], false);
    assert_eq!(json["snapshot_plan"]["requested"], true);
    assert_eq!(json["snapshots_created"], serde_json::json!([]));
    assert!(
        json["matched_patterns"]
            .as_array()
            .is_some_and(|patterns| !patterns.is_empty()),
        "danger command must report matched patterns in json mode"
    );
    assert!(
        !home.path().join(".aegis").join("audit.jsonl").exists(),
        "evaluation-only json mode must not append an audit entry"
    );
}

#[test]
fn invalid_project_config_in_json_mode_preserves_stderr_contract() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        "mode = <<<THIS IS NOT VALID TOML\n",
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", "echo hi", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    assert!(
        output.stdout.is_empty(),
        "setup failure must keep the current stderr-only contract"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: failed to load config"));
    assert!(stderr.contains("Fix or remove the invalid config file"));
}

#[test]
fn json_mode_still_does_not_write_audit_entries_when_planned() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "echo hi", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["command"], "echo hi");
    assert_eq!(json["execution"]["mode"], "evaluation_only");
    assert_eq!(json["execution"]["will_execute"], false);
    assert!(
        !home.path().join(".aegis").join("audit.jsonl").exists(),
        "planned json evaluation must not append an audit entry"
    );
}

#[test]
fn json_output_snapshot_policy_none_disables_snapshot_request_for_danger() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
snapshot_policy = "None"
auto_snapshot_git = true
auto_snapshot_docker = true
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args([
            "-c",
            "terraform destroy -target=module.prod.api",
            "--output",
            "json",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["risk"], "danger");
    assert_eq!(json["decision"], "prompt");
    assert_eq!(json["snapshot_plan"]["requested"], false);
    assert_eq!(
        json["snapshot_plan"]["applicable_plugins"],
        serde_json::json!([])
    );
}

#[test]
fn json_output_allowlisted_danger_reports_effective_allowlist_and_snapshot_plan_without_exec() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    let workspace_cwd = workspace.path().to_string_lossy();
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
mode = "Strict"
allowlist_override_level = "Danger"
auto_snapshot_git = true
auto_snapshot_docker = false
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{workspace_cwd}"
reason = "strict override allowlist"
"#
        ),
    )
    .unwrap();

    Command::new("git")
        .arg("init")
        .current_dir(workspace.path())
        .output()
        .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args([
            "-c",
            "terraform destroy -target=module.test.api",
            "--output",
            "json",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["risk"], "danger");
    assert_eq!(json["decision"], "auto_approve");
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["mode"], "strict");
    assert_eq!(json["allowlist_match"]["matched"], true);
    assert_eq!(json["allowlist_match"]["effective"], true);
    assert_eq!(
        json["allowlist_match"]["pattern"],
        "terraform destroy -target=module.test.*"
    );
    assert_eq!(
        json["allowlist_match"]["reason"],
        "strict override allowlist"
    );
    assert_eq!(json["snapshot_plan"]["requested"], true);
    assert_eq!(
        json["snapshot_plan"]["applicable_plugins"],
        serde_json::json!(["git"])
    );
    assert_eq!(json["snapshots_created"], serde_json::json!([]));
    assert!(
        !home.path().join(".aegis").join("audit.jsonl").exists(),
        "evaluation-only json mode must not append an audit entry"
    );
}

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
    let workspace_cwd = workspace.path().to_string_lossy();
    fs::write(
        &config_path,
        format!(
            r#"
allowlist_override_level = "Danger"
[[allowlist]]
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

    assert!(allowed_output.status.success());
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
[[allowlist]]
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

/// Block-level commands must never be silently auto-approved by an allowlist
/// entry — even when the glob pattern explicitly covers the command.
///
/// Rationale: Block = catastrophic irreversible harm (rm -rf /, fork bomb,
/// mkfs). No config entry should be able to bypass these without the operator
/// seeing the Block dialog.
#[test]
fn block_command_is_never_allowlisted() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    // Allowlist entry that would match `rm -rf /` if the guard were absent.
    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
[[allowlist]]
pattern = "rm -rf /"
cwd = "/aegis-test-scope"
reason = "test block allowlist"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", "rm -rf /"])
        .stdin(Stdio::null()) // EOF stdin → Block dialog auto-closes
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    // Must be blocked (exit 3), never auto-approved.
    assert_eq!(
        output.status.code(),
        Some(3),
        "Block command must not be auto-approved even when it matches an allowlist entry"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("AEGIS BLOCKED"),
        "Block dialog must still be shown"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("blocked by an explicit danger/block pattern"),
        "explicit block must explain the reason precisely; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("matched patterns"),
        "explicit block must point operators to matched patterns; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("--output json"),
        "explicit block must mention JSON output for machine-readable details; stderr:\n{stderr}"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "Blocked");
    assert_eq!(entries[0]["risk"], "Block");
    // Blocked commands must not be annotated as allowlisted, even if they
    // matched a rule before the hard block decision.
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
    let workspace_cwd = workspace.path().to_string_lossy();
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
allowlist_override_level = "Danger"
[[allowlist]]
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

    assert!(output.status.success());
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
    let workspace_cwd = workspace.path().to_string_lossy();
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
allowlist_override_level = "Danger"
[[allowlist]]
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

    assert!(output.status.success());
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
    let workspace_cwd = workspace.path().to_string_lossy();
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
allowlist_override_level = "Danger"
[[allowlist]]
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

    assert!(output.status.success());
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

#[test]
fn shell_wrapper_ls_nonexistent_matches_real_shell_passthrough() {
    let home = TempDir::new().unwrap();
    let command = "ls /nonexistent";

    let aegis_output = base_command(home.path())
        .args(["-c", command])
        .output()
        .unwrap();
    let shell_output = direct_shell_command(home.path())
        .args(["-c", command])
        .output()
        .unwrap();

    assert_eq!(aegis_output.status.code(), shell_output.status.code());
    assert_eq!(aegis_output.stdout, shell_output.stdout);
    assert_eq!(aegis_output.stderr, shell_output.stderr);

    #[cfg(target_os = "linux")]
    assert_eq!(aegis_output.status.code(), Some(2));

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Safe");
}

#[test]
fn shell_wrapper_preserves_environment_and_working_directory() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let command = "pwd; printf '%s' \"$AEGIS_TEST_VALUE\"";

    let aegis_output = base_command(home.path())
        .current_dir(workspace.path())
        .env("AEGIS_TEST_VALUE", "kept")
        .args(["-c", command])
        .output()
        .unwrap();

    let shell_output = direct_shell_command(home.path())
        .current_dir(workspace.path())
        .env("AEGIS_TEST_VALUE", "kept")
        .args(["-c", command])
        .output()
        .unwrap();

    assert_eq!(aegis_output.status.code(), Some(0));
    assert_eq!(aegis_output.status.code(), shell_output.status.code());
    assert_eq!(aegis_output.stdout, shell_output.stdout);
    assert_eq!(aegis_output.stderr, shell_output.stderr);

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Safe");
}

#[test]
fn custom_pattern_from_config_changes_classification_and_is_labeled_custom() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
[[custom_patterns]]
id = "USR-E2E-001"
category = "Process"
risk = "Warn"
pattern = "echo\\s+hello"
description = "Treat hello echo as suspicious in this project"
"#,
    )
    .unwrap();

    // AEGIS_FORCE_INTERACTIVE=1 ensures the full dialog is rendered so we can
    // assert UI diagnostics (`source: custom`).
    let mut child = base_command(home.path())
        .current_dir(workspace.path())
        .env("AEGIS_FORCE_INTERACTIVE", "1")
        .args(["-c", "echo hello"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.as_mut().unwrap().write_all(b"no\n").unwrap();
    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("source: custom"),
        "interactive UI must label custom-source matches; stderr:\n{stderr}"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["risk"], "Warn");
    assert_eq!(entries[0]["decision"], "Denied");
    assert_eq!(entries[0]["matched_patterns"][0]["id"], "USR-E2E-001");
    assert_eq!(entries[0]["matched_patterns"][0]["source"], "custom");
}

// ─────────────────────────────────────────────────────────────────────────────
// Regression tests: security-critical failure modes
// ─────────────────────────────────────────────────────────────────────────────

// Config parse failure ─────────────────────────────────────────────────────

/// A malformed `.aegis.toml` must fail closed with an internal error.
#[test]
fn broken_project_config_aborts_shell_wrapper_with_clear_error() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        "mode = <<<THIS IS NOT VALID TOML\n",
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", "echo hello"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty(), "command must not execute");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let config_path = workspace.path().join(".aegis.toml");
    assert!(
        stderr.contains("error: failed to load config"),
        "stderr must explain the startup failure: {stderr}"
    );
    assert!(
        stderr.contains(&config_path.display().to_string()),
        "stderr must identify the invalid config file: {stderr}"
    );
    assert!(
        stderr.contains("failed to parse"),
        "stderr must include the parse/validation detail: {stderr}"
    );
    assert!(
        stderr.contains("Fix or remove the invalid config file"),
        "stderr must tell the user how to recover: {stderr}"
    );
}

/// Invalid custom patterns from config must abort startup instead of degrading to Warn.
#[test]
fn invalid_custom_pattern_config_aborts_shell_wrapper() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let config_path = workspace.path().join(".aegis.toml");

    fs::write(
        &config_path,
        r#"
[[custom_patterns]]
id = "FS-001"
category = "Filesystem"
risk = "Warn"
pattern = "echo hello"
description = "Conflicts with built-in pattern id"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", "echo hello"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty(), "command must not execute");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error: failed to load config"),
        "stderr must explain the startup failure: {stderr}"
    );
    assert!(
        stderr.contains(&config_path.display().to_string()),
        "stderr must identify the invalid config file: {stderr}"
    );
    assert!(
        stderr.contains("duplicate pattern id"),
        "stderr must include the custom pattern failure detail: {stderr}"
    );
    assert!(
        stderr.contains("Fix or remove the invalid config file"),
        "stderr must tell the user how to recover for config errors: {stderr}"
    );
}

/// A well-formed but invalid `.aegis.toml` must also fail closed and name the file.
#[test]
fn invalid_project_config_validation_error_aborts_shell_wrapper_with_clear_error() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    let config_path = workspace.path().join(".aegis.toml");
    fs::write(
        &config_path,
        r#"
[audit]
rotation_enabled = true
max_file_size_bytes = 0
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", "echo hello"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty(), "command must not execute");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error: failed to load config"),
        "stderr must explain the startup failure: {stderr}"
    );
    assert!(
        stderr.contains(&config_path.display().to_string()),
        "stderr must identify the invalid config file: {stderr}"
    );
    assert!(
        stderr.contains("audit.max_file_size_bytes"),
        "stderr must include the validation detail: {stderr}"
    );
    assert!(
        stderr.contains("Fix or remove the invalid config file"),
        "stderr must tell the user how to recover: {stderr}"
    );
}

// Audit logger failure ─────────────────────────────────────────────────────

/// If `~/.aegis` is a file instead of a directory, audit append fails.
/// The binary must NOT crash — error is swallowed in non-verbose mode.
#[test]
fn audit_logger_failure_does_not_crash_binary() {
    let home = TempDir::new().unwrap();
    fs::write(home.path().join(".aegis"), "I am a file, not a directory").unwrap();

    let output = base_command(home.path())
        .args(["-c", "echo hello"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "binary must not crash when audit log is unwritable"
    );
    assert_eq!(output.stdout, b"hello\n");
}

/// With `--verbose`, a failed audit append must emit a warning to stderr.
#[test]
fn audit_logger_failure_verbose_prints_warning() {
    let home = TempDir::new().unwrap();
    fs::write(home.path().join(".aegis"), "I am a file, not a directory").unwrap();

    let output = base_command(home.path())
        .args(["-v", "-c", "echo hello"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("failed to append audit log entry"),
        "verbose mode must print a warning when audit append fails"
    );
}

#[test]
fn json_output_verbose_keeps_stderr_empty() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args([
            "--verbose",
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
        "json mode must keep stderr empty even when verbose is requested"
    );

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["risk"], "danger");
    assert_eq!(json["decision"], "prompt");
}

#[test]
fn audit_rotation_config_rotates_and_audit_command_reads_archives() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
[audit]
rotation_enabled = true
max_file_size_bytes = 1
retention_files = 3
compress_rotated = false
"#,
    )
    .unwrap();

    for command in ["printf one", "printf two", "printf three"] {
        let output = base_command(home.path())
            .current_dir(workspace.path())
            .args(["-c", command])
            .output()
            .unwrap();
        assert!(output.status.success());
    }

    let audit_dir = home.path().join(".aegis");
    assert!(audit_dir.join("audit.jsonl").exists());
    assert!(audit_dir.join("audit.jsonl.1").exists());

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["audit", "--last", "3"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("timestamp"));
    assert!(stdout.contains("printf one"));
    assert!(stdout.contains("printf two"));
    assert!(stdout.contains("printf three"));
}

#[test]
fn audit_command_can_export_json_array() {
    let home = TempDir::new().unwrap();

    for command in ["printf one", "git stash clear"] {
        let output = base_command(home.path())
            .args(["-c", command])
            .output()
            .unwrap();
        if command == "git stash clear" {
            assert_eq!(output.status.code(), Some(2));
        } else {
            assert!(output.status.success());
        }
    }

    let output = base_command(home.path())
        .args(["audit", "--format", "json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let entries: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["command"], "printf one");
    assert_eq!(entries[1]["command"], "git stash clear");
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[1]["risk"], "Warn");
    assert_eq!(entries[0]["pattern_ids"], serde_json::json!([]));
    assert_eq!(entries[0]["mode"], "Protect");
    assert_eq!(entries[0]["ci_detected"], serde_json::json!(false));
    assert_eq!(entries[0]["allowlist_matched"], serde_json::json!(false));
    assert_eq!(entries[0]["allowlist_effective"], serde_json::json!(false));
}

#[test]
fn audit_command_can_export_ndjson_with_filters() {
    let home = TempDir::new().unwrap();

    for command in ["printf one", "git stash clear", "printf two"] {
        let output = base_command(home.path())
            .args(["-c", command])
            .output()
            .unwrap();
        if command == "git stash clear" {
            assert_eq!(output.status.code(), Some(2));
        } else {
            assert!(output.status.success());
        }
    }

    let output = base_command(home.path())
        .args([
            "audit", "--format", "ndjson", "--risk", "Warn", "--last", "1",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 1);

    let entry: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(entry["command"], "git stash clear");
    assert_eq!(entry["risk"], "Warn");
}

#[test]
fn audit_command_filters_by_decision_in_text_mode() {
    let home = TempDir::new().unwrap();

    for command in ["printf one", "git stash clear", "printf two"] {
        let output = base_command(home.path())
            .args(["-c", command])
            .output()
            .unwrap();
        if command == "git stash clear" {
            assert_eq!(output.status.code(), Some(2));
        } else {
            assert!(output.status.success());
        }
    }

    let output = base_command(home.path())
        .args(["audit", "--decision", "denied"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("git stash clear"));
    assert!(!stdout.contains("printf one"));
    assert!(!stdout.contains("printf two"));
}

#[test]
fn audit_command_filters_by_command_substring_in_json_mode() {
    let home = TempDir::new().unwrap();

    for command in ["printf alpha", "git stash clear", "printf beta"] {
        let output = base_command(home.path())
            .args(["-c", command])
            .output()
            .unwrap();
        if command == "git stash clear" {
            assert_eq!(output.status.code(), Some(2));
        } else {
            assert!(output.status.success());
        }
    }

    let output = base_command(home.path())
        .args(["audit", "--format", "json", "--command-contains", "stash"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let entries: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["command"], "git stash clear");
}

#[test]
fn audit_command_summary_reports_top_pattern_ids() {
    let home = TempDir::new().unwrap();

    for command in ["git stash clear", "git stash clear", "printf done"] {
        let output = base_command(home.path())
            .args(["-c", command])
            .output()
            .unwrap();
        if command == "git stash clear" {
            assert_eq!(output.status.code(), Some(2));
        } else {
            assert!(output.status.success());
        }
    }

    let output = base_command(home.path())
        .args(["audit", "--summary"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let entries = read_audit_entries(home.path());
    let top_pattern_id = entries
        .iter()
        .find_map(|entry| {
            entry["matched_patterns"]
                .as_array()
                .and_then(|patterns| patterns.first())
                .and_then(|pattern| pattern["id"].as_str())
        })
        .expect("expected at least one matched pattern id in audit log");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Top matched patterns"));
    assert!(stdout.contains(top_pattern_id));
}

#[test]
fn audit_command_rejects_summary_with_ndjson_format() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["audit", "--summary", "--format", "ndjson"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot be used with"));
    assert!(stderr.contains("--summary"));
    assert!(stderr.contains("ndjson"));
}

// Confirmation UI failure (stdin EOF) ────────────────────────────────────

/// A Danger-level command with stdin closed (EOF) must be DENIED.
/// `prompt_danger` reads empty string on EOF; `"" != "yes"` → denied.
/// Prevents silent auto-approval when stdin is /dev/null.
#[test]
fn danger_command_with_eof_stdin_is_denied() {
    let home = TempDir::new().unwrap();
    let target = home.path().join("sentinel_eof_test");
    fs::create_dir_all(&target).unwrap();

    // current_dir = home (not the project root git repo) so the GitPlugin
    // does not stash the developer's working tree when creating a snapshot.
    let output = base_command(home.path())
        .current_dir(home.path())
        .args(["-c", &format!("rm -rf {}", target.display())])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "Danger command must be denied when stdin is closed"
    );
    assert!(target.exists(), "target must still exist after denial");
}

/// A Warn-level command with no TTY must be denied automatically (fail-closed).
///
/// Previously (before non-interactive mode), empty stdin was treated as
/// "proceed" by the Warn prompt (`"" != "n"`), so EOF auto-approved.  That
/// behaviour was unsafe for CI/agent runners: an AI agent could get a
/// suspicious command approved without any human present.
///
/// The new contract: no TTY → non-interactive mode → Warn is denied.
/// To allow a Warn command in CI, add it to the allowlist.
#[test]
fn warn_command_non_interactive_is_denied() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "git stash clear"])
        .stdin(Stdio::null()) // no TTY → non-interactive
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "Warn command must be denied (exit 2) in non-interactive mode"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("non-interactive"),
        "non-interactive denial must say 'non-interactive'; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("allowlist"),
        "non-interactive denial must mention 'allowlist' as the escape hatch; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("command denied"),
        "non-interactive denial must explicitly say the command was denied; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("--output json"),
        "non-interactive denial must mention JSON output for automation; stderr:\n{stderr}"
    );
}

// Shell resolution failure ────────────────────────────────────────────────

/// When `SHELL` points to the aegis binary itself, `resolve_shell` must
/// fall back to `/bin/sh` to prevent an infinite exec loop.
#[test]
fn shell_env_pointing_to_aegis_binary_falls_back_to_bin_sh() {
    let home = TempDir::new().unwrap();

    let output = Command::new(aegis_bin())
        .env("HOME", home.path())
        .env("SHELL", aegis_bin().as_os_str())
        .args(["-c", "echo hi"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "binary must not loop when SHELL points to itself"
    );
    assert_eq!(output.stdout, b"hi\n");
}

/// When `AEGIS_REAL_SHELL` points to a nonexistent binary, `exec_command`
/// must return exit code 1 rather than panicking.
#[test]
fn nonexistent_aegis_real_shell_returns_exit_code_1() {
    let home = TempDir::new().unwrap();

    let output = Command::new(aegis_bin())
        .env("HOME", home.path())
        .env("AEGIS_REAL_SHELL", "/nonexistent/shell/binary")
        .args(["-c", "echo hello"])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(4),
        "unresolvable shell must yield exit code 4 (internal error), not a panic"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Snapshot registry config-flag regressions (Ticket 1.3)
// ─────────────────────────────────────────────────────────────────────────────

/// With `auto_snapshot_git = false` in `.aegis.toml`, the real aegis binary
/// must never invoke the `git` stub in PATH, and the audit entry must record
/// an empty `snapshots` array.
///
/// This proves the Git plugin was never *registered*, not merely skipped by
/// `is_applicable` — the stub would catch any `git` invocation regardless of
/// which code path triggered it.
#[test]
fn snapshot_registry_git_flag_false_skips_plugin_and_audit() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");
    let git_log = workspace.path().join("git_stub.log");

    fs::create_dir_all(&bin_dir).unwrap();

    // Stub git: append arguments to the log file so we can assert it was never
    // called.  Use AEGIS_TEST_GIT_LOG as the log path so the test controls it.
    write_executable(
        &bin_dir.join("git"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_GIT_LOG"
exit 0
"#,
    );

    // Both snapshot flags off: git-off is under test, docker-off isolates the
    // assertion so a stray docker invocation cannot add noise.
    fs::write(
        workspace.path().join(".aegis.toml"),
        "auto_snapshot_git = false\nauto_snapshot_docker = false\n",
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    // Use a unique sentinel file in the temp workspace so the rm -rf target
    // is fully controlled and isolated from the developer's file system.
    let sentinel = workspace.path().join("sentinel_git_off.txt");
    fs::write(&sentinel, "git-off test sentinel").unwrap();

    let mut child = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_GIT_LOG", &git_log)
        .env("AEGIS_FORCE_INTERACTIVE", "1")
        .args(["-c", &format!("rm -rf {}", sentinel.display())])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.as_mut().unwrap().write_all(b"no\n").unwrap();
    let output = child.wait_with_output().unwrap();

    // Danger command denied → exit code 2.
    assert_eq!(
        output.status.code(),
        Some(2),
        "Danger command must be denied (exit 2)"
    );

    // Git stub must never have been called.
    let git_calls = read_stub_invocations(&git_log);
    assert!(
        git_calls.is_empty(),
        "git stub must not be invoked when auto_snapshot_git = false; calls: {git_calls:?}"
    );

    // Exactly one audit entry, decision Denied, risk Danger, snapshots empty.
    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1, "expected exactly one audit entry");
    assert_eq!(entries[0]["decision"], "Denied");
    assert_eq!(entries[0]["risk"], "Danger");
    assert_eq!(entries[0]["snapshots"], serde_json::json!([]));
}

/// With `auto_snapshot_docker = false` in `.aegis.toml`, the real aegis binary
/// must never invoke the `docker` stub in PATH, and the audit entry must record
/// an empty `snapshots` array.
///
/// Both snapshot flags are disabled to keep the assertions fully isolated —
/// disabling git prevents unrelated git activity from adding noise when the
/// workspace happens to be near a git checkout.
#[test]
fn snapshot_registry_docker_flag_false_skips_plugin_and_audit() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");
    let docker_log = workspace.path().join("docker_stub.log");

    fs::create_dir_all(&bin_dir).unwrap();

    // Stub docker: append arguments to the log file so we can assert it was
    // never called.  Use AEGIS_TEST_DOCKER_LOG as the log path.
    write_executable(
        &bin_dir.join("docker"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_DOCKER_LOG"
exit 0
"#,
    );

    // Docker-off is under test; git-off isolates assertions from git noise.
    fs::write(
        workspace.path().join(".aegis.toml"),
        "auto_snapshot_git = false\nauto_snapshot_docker = false\n",
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let sentinel = workspace.path().join("sentinel_docker_off.txt");
    fs::write(&sentinel, "docker-off test sentinel").unwrap();

    let mut child = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_DOCKER_LOG", &docker_log)
        .env("AEGIS_FORCE_INTERACTIVE", "1")
        .args(["-c", &format!("rm -rf {}", sentinel.display())])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.as_mut().unwrap().write_all(b"no\n").unwrap();
    let output = child.wait_with_output().unwrap();

    // Danger command denied → exit code 2.
    assert_eq!(
        output.status.code(),
        Some(2),
        "Danger command must be denied (exit 2)"
    );

    // Docker stub must never have been called.
    let docker_calls = read_stub_invocations(&docker_log);
    assert!(
        docker_calls.is_empty(),
        "docker stub must not be invoked when auto_snapshot_docker = false; calls: {docker_calls:?}"
    );

    // Exactly one audit entry, decision Denied, risk Danger, snapshots empty.
    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1, "expected exactly one audit entry");
    assert_eq!(entries[0]["decision"], "Denied");
    assert_eq!(entries[0]["risk"], "Danger");
    assert_eq!(entries[0]["snapshots"], serde_json::json!([]));
}

#[test]
fn config_show_prints_effective_allowlist_override_level() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
mode = "Strict"
allowlist_override_level = "Danger"
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "/srv/infra"
reason = "ephemeral test teardown"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "show"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("mode = \"Strict\""));
    assert!(stdout.contains("allowlist_override_level = \"Danger\""));
    assert!(stdout.contains("[[allowlist]]"));
    assert!(stdout.contains("pattern = \"terraform destroy -target=module.test.*\""));
    assert!(stdout.contains("reason = \"ephemeral test teardown\""));
    assert!(
        !stdout.contains("allowlist = ["),
        "config show must emit structured allowlist entries, not legacy string-array syntax"
    );
}

#[test]
fn unscoped_structured_allowlist_fails_runtime_execution() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
[[allowlist]]
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

#[test]
fn config_validate_reports_missing_scope_as_error_for_legacy_allowlist() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"allowlist = ["terraform destroy *"]"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        json["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["code"] == "missing_scope")
    );
}

#[test]
fn config_show_uses_inspection_path_for_legacy_allowlist() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"allowlist = ["terraform destroy *"]"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "show"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[[allowlist]]"));
    assert!(stdout.contains("pattern = \"terraform destroy *\""));
    assert!(stdout.contains("reason = \"migrated from legacy allowlist entry\""));
}

#[test]
fn config_init_writes_truthful_mode_comments() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "init"])
        .output()
        .unwrap();

    assert!(output.status.success());

    let contents = fs::read_to_string(workspace.path().join(".aegis.toml")).unwrap();
    assert!(contents.contains("config_version = 1"));
    assert!(contents.contains("Protect prompts on Warn/Danger"));
    assert!(contents.contains("Audit is non-blocking audit-only"));
    assert!(contents.contains("Strict blocks non-safe and indirect execution forms by default"));
    assert!(contents.contains("allowlist_override_level = \"Warn\""));
    assert!(contents.contains("[[allowlist]]"));
    assert!(contents.contains("Protect/Strict allowlist ceiling"));
    assert!(contents.contains("allowlist rule must declare cwd or user scope"));
    assert!(contents.contains("Warn auto-approves allowlisted Warn commands in Protect/Strict"));
    assert!(contents.contains("Danger also auto-approves allowlisted Danger commands"));
    assert!(contents.contains("Never disables allowlist auto-approval for non-safe commands"));
    assert!(contents.contains("Block never bypasses in Protect/Strict"));
    assert!(
        !contents.contains("allowlist = ["),
        "init template must not fall back to legacy string-array syntax"
    );
    assert!(!contents.contains("not yet implemented"));
}

#[test]
fn config_validate_json_outputs_errors_and_warnings() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let config_path = workspace.path().join(".aegis.toml");

    fs::write(
        &config_path,
        r#"
[audit]
rotation_enabled = true
max_file_size_bytes = 0
retention_files = 0

[[allowlist]]
pattern = "terraform destroy *"
reason = "broad rule"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let errors = json.get("errors").unwrap().as_array().unwrap();
    let warnings = json.get("warnings").unwrap().as_array().unwrap();

    assert!(
        errors.iter().any(|e| e["code"] == "audit_max_file_size"),
        "missing audit_max_file_size error: {errors:?}"
    );
    assert!(
        errors.iter().any(|e| e["code"] == "audit_retention_files"),
        "missing audit_retention_files error: {errors:?}"
    );
    assert!(
        warnings.iter().any(|w| w["code"] == "missing_scope"),
        "missing missing_scope warning: {warnings:?}"
    );
    let config_path = config_path.to_string_lossy();
    assert!(
        errors.iter().any(|e| {
            e["location"]
                .as_str()
                .is_some_and(|location| location.contains(config_path.as_ref()))
        }),
        "expected at least one error location to contain config path {config_path}; errors: {errors:?}"
    );
    assert!(
        warnings.iter().any(|w| {
            w["location"]
                .as_str()
                .is_some_and(|location| location.contains(config_path.as_ref()))
        }),
        "expected at least one warning location to contain config path {config_path}; warnings: {warnings:?}"
    );
}

#[test]
fn config_validate_layered_scalar_errors_point_to_actual_source_files() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let global_dir = home.path().join(".config/aegis");
    let global_path = global_dir.join("config.toml");
    let project_path = workspace.path().join(".aegis.toml");

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        &global_path,
        r#"
[audit]
rotation_enabled = true
max_file_size_bytes = 1024
"#,
    )
    .unwrap();
    fs::write(
        &project_path,
        r#"
[audit]
retention_files = 0
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let errors = json["errors"].as_array().unwrap();

    let retention_error = errors
        .iter()
        .find(|e| e["code"] == "audit_retention_files")
        .unwrap();

    assert!(
        retention_error["location"]
            .as_str()
            .is_some_and(|location| location.contains(project_path.to_string_lossy().as_ref())),
        "retention_files location should reference project config path: {retention_error:?}"
    );
    assert!(
        errors.iter().all(|e| e["code"] != "audit_max_file_size"),
        "max_file_size error should not be present when global layer is valid: {errors:?}"
    );
    assert!(
        errors.iter().all(|e| {
            !e["location"]
                .as_str()
                .is_some_and(|location| location.contains(global_path.to_string_lossy().as_ref()))
        }),
        "no error should be attributed to global layer in this scenario: {errors:?}"
    );
}

#[test]
fn config_validate_reports_global_stage_error_even_if_project_overrides_value() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let global_dir = home.path().join(".config/aegis");
    let global_path = global_dir.join("config.toml");
    let project_path = workspace.path().join(".aegis.toml");

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        &global_path,
        r#"
[audit]
rotation_enabled = true
max_file_size_bytes = 0
"#,
    )
    .unwrap();
    fs::write(
        &project_path,
        r#"
[audit]
max_file_size_bytes = 1024
"#,
    )
    .unwrap();

    let validate_output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(validate_output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&validate_output.stdout).unwrap();
    let error = json["errors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["code"] == "audit_max_file_size")
        .unwrap();

    assert!(
        error["location"]
            .as_str()
            .is_some_and(|location| location.contains(global_path.to_string_lossy().as_ref())),
        "audit_max_file_size should reference global config path: {error:?}"
    );

    let runtime_output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", "printf ok"])
        .output()
        .unwrap();
    assert_eq!(runtime_output.status.code(), Some(4));
    assert!(
        runtime_output.stdout.is_empty(),
        "runtime must fail closed and not execute shell command"
    );
}

#[test]
fn config_validate_stops_after_global_hard_failure() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let global_dir = home.path().join(".config/aegis");
    let global_path = global_dir.join("config.toml");
    let project_path = workspace.path().join(".aegis.toml");

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        &global_path,
        r#"
[audit]
rotation_enabled = true
max_file_size_bytes = 0
"#,
    )
    .unwrap();
    fs::write(
        &project_path,
        r#"
[[allowlist]]
pattern = "terraform destroy *"
reason = "would warn if reached"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let errors = json["errors"].as_array().unwrap();
    let warnings = json["warnings"].as_array().unwrap();

    assert!(
        errors.iter().any(|e| e["code"] == "audit_max_file_size"),
        "expected global audit error; got {errors:?}"
    );
    assert!(
        warnings.is_empty(),
        "project warnings should be absent because global hard failure stops processing: {warnings:?}"
    );
}

#[test]
fn config_validate_project_rule_uses_file_local_index_in_location() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let global_dir = home.path().join(".config/aegis");
    let global_path = global_dir.join("config.toml");
    let project_path = workspace.path().join(".aegis.toml");

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        &global_path,
        r#"
[[allowlist]]
pattern = "terraform destroy -target=module.global.api"
cwd = "/srv/global"
user = "ci"
reason = "scoped global"
"#,
    )
    .unwrap();
    fs::write(
        &project_path,
        r#"
[[allowlist]]
pattern = "terraform destroy *"
reason = "broad project"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let error = json["errors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["code"] == "invalid_allowlist_rule")
        .unwrap();
    let location = error["location"].as_str().unwrap();
    assert!(
        location.contains(project_path.to_string_lossy().as_ref())
            && location.contains("allowlist[0]"),
        "project rule location should use project-local index 0: {error:?}"
    );
}

#[test]
fn config_validate_invalid_custom_pattern_reports_offending_entry_only() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let global_dir = home.path().join(".config/aegis");
    let global_path = global_dir.join("config.toml");
    let project_path = workspace.path().join(".aegis.toml");

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        &global_path,
        r#"
[[custom_patterns]]
id = "USR-GLOBAL-001"
category = "Filesystem"
risk = "Warn"
pattern = "echo global"
description = "global custom pattern"
"#,
    )
    .unwrap();
    fs::write(
        &project_path,
        r#"
[[custom_patterns]]
id = "FS-001"
category = "Filesystem"
risk = "Warn"
pattern = "echo bad"
description = "duplicate built-in id"

[[custom_patterns]]
id = "USR-PROJ-002"
category = "Filesystem"
risk = "Warn"
pattern = "echo later"
description = "would be valid"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let error = json["errors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["code"] == "invalid_custom_pattern")
        .unwrap();

    let location = error["location"].as_str().unwrap();
    assert!(
        location.contains(project_path.to_string_lossy().as_ref())
            && location.contains("custom_patterns[0]"),
        "custom pattern error should point to first offending project entry: {error:?}"
    );
    assert!(
        !location.contains(global_path.to_string_lossy().as_ref()),
        "custom pattern error should not be attributed to unrelated global entries: {error:?}"
    );
}

#[test]
fn config_validate_invalid_allowlist_reports_offending_entry_only() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let global_dir = home.path().join(".config/aegis");
    let global_path = global_dir.join("config.toml");
    let project_path = workspace.path().join(".aegis.toml");

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        &global_path,
        r#"
[[allowlist]]
pattern = "terraform destroy -target=module.global.api"
cwd = "/srv/global"
reason = "global valid"
"#,
    )
    .unwrap();
    fs::write(
        &project_path,
        r#"
[[allowlist]]
pattern = ""
reason = "invalid project rule"

[[allowlist]]
pattern = "terraform destroy -target=module.project.api"
reason = "never reached"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let error = json["errors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["code"] == "invalid_allowlist_rule")
        .unwrap();

    let location = error["location"].as_str().unwrap();
    assert!(
        location.contains(project_path.to_string_lossy().as_ref())
            && location.contains("allowlist[0]"),
        "allowlist error should point to first offending project entry: {error:?}"
    );
    assert!(
        !location.contains(global_path.to_string_lossy().as_ref()),
        "allowlist error should not be attributed to unrelated global entries: {error:?}"
    );
}

#[test]
fn config_validate_warnings_only_exits_zero_and_prints_text_report() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
[[allowlist]]
pattern = "terraform destroy *"
cwd = "/srv/infra"
reason = "broad warning only"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("warnings:"));
    assert!(stdout.contains("[broad_pattern]"));
    assert!(!stdout.contains("errors:"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Mode runtime semantics (Ticket 1.4)
// ─────────────────────────────────────────────────────────────────────────────

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
    let workspace_cwd = workspace.path().to_string_lossy();
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
mode = "Protect"
ci_policy = "Block"
allowlist_override_level = "Danger"
auto_snapshot_git = false
auto_snapshot_docker = false
[[allowlist]]
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

    assert!(output.status.success());
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
    let workspace_cwd = workspace.path().to_string_lossy();
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
mode = "Strict"
allowlist_override_level = "Warn"
auto_snapshot_git = false
auto_snapshot_docker = false
[[allowlist]]
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

    assert!(allowed_output.status.success());
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
    let workspace_cwd = workspace.path().to_string_lossy();
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
mode = "Strict"
allowlist_override_level = "Danger"
auto_snapshot_git = false
auto_snapshot_docker = false
[[allowlist]]
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

    assert!(output.status.success());
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

#[test]
fn legacy_allowlist_schema_is_migrated_by_config_show() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
mode = "Strict"
allowlist = ["terraform destroy *"]
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "show"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "legacy config should migrate cleanly"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config_version = 1"));
    assert!(stdout.contains("[[allowlist]]"));
    assert!(stdout.contains("pattern = \"terraform destroy *\""));
    assert!(stdout.contains("reason = \"migrated from legacy allowlist entry\""));
    assert!(!stdout.contains("allowlist = ["));
}

/// Audit mode must never block or prompt — even Block-level commands in CI
/// with ci_policy = Block must be auto-approved and executed.
#[test]
fn audit_mode_stays_non_blocking_for_block_classification() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");
    let log_path = workspace.path().join("rm.log");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("rm"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_RM_LOG"
exit 0
"#,
    );
    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
mode = "Audit"
ci_policy = "Block"
auto_snapshot_git = false
auto_snapshot_docker = false
[[allowlist]]
pattern = "rm -rf /"
cwd = "/aegis-test-scope"
reason = "audit mode should not attribute allowlist authorization"
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
        .env("AEGIS_CI", "1")
        .env("PATH", &path)
        .env("AEGIS_TEST_RM_LOG", &log_path)
        .args(["-c", "rm -rf /"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(fs::read_to_string(&log_path).unwrap(), "-rf /\n");

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Block");
    assert_eq!(entries[0]["snapshots"], serde_json::json!([]));
    assert!(entries[0].get("allowlist_pattern").is_none());
    assert!(entries[0].get("allowlist_reason").is_none());
}

/// Strict mode must block Warn commands even when ci_policy = Allow.
/// CI policy cannot weaken Strict mode's non-safe default.
#[test]
fn strict_mode_blocks_warn_even_when_ci_policy_allows() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");
    let log_path = workspace.path().join("git.log");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("git"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_GIT_LOG"
exit 0
"#,
    );
    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
mode = "Strict"
ci_policy = "Allow"
auto_snapshot_git = false
auto_snapshot_docker = false
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
        .env("AEGIS_CI", "1")
        .env("PATH", &path)
        .env("AEGIS_TEST_GIT_LOG", &log_path)
        .args(["-c", "git stash clear"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    assert!(read_stub_invocations(&log_path).is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("blocked by strict mode"),
        "strict mode block must name strict mode; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("allowlist"),
        "strict mode block must mention allowlist guidance; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("config validate"),
        "strict mode block must mention config validation; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("--output json"),
        "strict mode block must mention JSON output; stderr:\n{stderr}"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries[0]["decision"], "Blocked");
    assert_eq!(entries[0]["risk"], "Warn");
}

#[test]
fn protect_ci_policy_block_message_is_actionable() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");
    let log_path = workspace.path().join("git.log");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("git"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_GIT_LOG"
exit 0
"#,
    );
    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
mode = "Protect"
ci_policy = "Block"
auto_snapshot_git = false
auto_snapshot_docker = false
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
        .env("AEGIS_CI", "1")
        .env("PATH", &path)
        .env("AEGIS_TEST_GIT_LOG", &log_path)
        .args(["-c", "git stash clear"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    assert!(read_stub_invocations(&log_path).is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("blocked by CI policy"),
        "CI policy block must name CI policy explicitly; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("allowlist"),
        "CI policy block must mention allowlist guidance; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("config validate"),
        "CI policy block must mention config validation; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("--output json"),
        "CI policy block must mention JSON output; stderr:\n{stderr}"
    );
}

/// Strict mode must treat indirect execution wrappers as blocked-by-default
/// policy forms even when their payload looks otherwise safe.
#[test]
fn strict_mode_blocks_nested_shell_execution_profile() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let marker_path = workspace.path().join("strict-indirect-exec.txt");

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
mode = "Strict"
auto_snapshot_git = false
auto_snapshot_docker = false
"#,
    )
    .unwrap();

    let indirect = format!("bash -c 'touch {}'", marker_path.display());

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", &indirect])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    assert!(
        !marker_path.exists(),
        "strict mode must block nested shell execution before it touches the filesystem"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "Blocked");
    assert_eq!(entries[0]["risk"], "Warn");
}

/// Strict mode with allowlist_override_level = Danger and an allowlisted
/// Danger command must auto-approve and create a git snapshot.
#[test]
fn strict_override_allowlisted_danger_executes_and_creates_snapshot() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    // bin_dir and log_path must be outside the workspace so that git stash
    // (--include-untracked) does not sweep them into the stash and make
    // terraform un-findable after the snapshot is created.
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
    let workspace_cwd = workspace.path().to_string_lossy();
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
mode = "Strict"
allowlist_override_level = "Danger"
auto_snapshot_git = true
auto_snapshot_docker = false
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{workspace_cwd}"
reason = "strict override allowlist"
"#
        ),
    )
    .unwrap();

    Command::new("git")
        .arg("init")
        .current_dir(workspace.path())
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
        .current_dir(workspace.path())
        .output()
        .unwrap();
    fs::write(workspace.path().join("dirty.txt"), "needs snapshot\n").unwrap();

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

    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(&log_path).unwrap(),
        "destroy -target=module.test.api\n"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Danger");
    assert_ne!(entries[0]["snapshots"], serde_json::json!([]));
}

#[test]
fn rollback_restores_git_snapshot_from_audit_and_logs_action() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
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
    let workspace_cwd = workspace.path().to_string_lossy();
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
allowlist_override_level = "Danger"
auto_snapshot_git = true
auto_snapshot_docker = false
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{workspace_cwd}"
reason = "rollback test allowlist"
"#
        ),
    )
    .unwrap();

    Command::new("git")
        .arg("init")
        .current_dir(workspace.path())
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
        .current_dir(workspace.path())
        .output()
        .unwrap();
    fs::write(workspace.path().join("tracked.txt"), "original\n").unwrap();
    Command::new("git")
        .args(["add", "tracked.txt"])
        .current_dir(workspace.path())
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "-c",
            "user.email=test@aegis.dev",
            "-c",
            "user.name=Aegis Test",
            "commit",
            "-m",
            "add tracked file",
        ])
        .current_dir(workspace.path())
        .output()
        .unwrap();

    fs::write(workspace.path().join("tracked.txt"), "needs rollback\n").unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let intercept_output = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    assert!(intercept_output.status.success());
    assert_eq!(
        fs::read_to_string(workspace.path().join("tracked.txt")).unwrap(),
        "original\n"
    );

    let entries = read_audit_entries(home.path());
    let snapshot_id = entries[0]["snapshots"][0]["snapshot_id"]
        .as_str()
        .expect("snapshot_id must be a string")
        .to_string();

    let rollback_output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["rollback", &snapshot_id])
        .output()
        .unwrap();

    assert!(
        rollback_output.status.success(),
        "rollback stderr:\n{}",
        String::from_utf8_lossy(&rollback_output.stderr)
    );
    assert_eq!(
        fs::read_to_string(workspace.path().join("tracked.txt")).unwrap(),
        "needs rollback\n"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[1]["command"],
        format!("aegis rollback {snapshot_id}")
    );
    assert_eq!(entries[1]["decision"], "Approved");
    assert_eq!(entries[1]["risk"], "Safe");
    assert_eq!(entries[1]["snapshots"][0]["plugin"], "git");
    assert_eq!(
        entries[1]["snapshots"][0]["snapshot_id"].as_str(),
        Some(snapshot_id.as_str())
    );
}

#[test]
fn rollback_missing_snapshot_prints_recovery_hint() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["rollback", "missing-snapshot"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing-snapshot"));
    assert!(stderr.contains("aegis audit"));
    assert!(stderr.contains("snapshot"));
}

#[test]
fn rollback_with_malformed_project_config_fails_closed_instead_of_falling_back() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let config_path = workspace.path().join(".aegis.toml");

    fs::write(&config_path, "mode = <<<THIS IS NOT VALID TOML\n").unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["rollback", "missing-snapshot"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&config_path.display().to_string()),
        "rollback must report the malformed config path: {stderr}"
    );
    assert!(
        stderr.contains("failed to parse"),
        "rollback must surface config parsing errors instead of silently falling back: {stderr}"
    );
    assert!(
        !stderr.contains("snapshot id"),
        "rollback must fail on config load before attempting snapshot lookup: {stderr}"
    );
}

#[test]
fn rollback_with_malformed_project_config_uses_standard_config_load_error_format() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let config_path = workspace.path().join(".aegis.toml");

    fs::write(&config_path, "mode = <<<THIS IS NOT VALID TOML\n").unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["rollback", "missing-snapshot"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error: failed to load config:"),
        "rollback config failures should use the standard config-load prefix: {stderr}"
    );
    assert!(
        stderr.contains(&config_path.display().to_string()),
        "rollback config failures should identify the invalid config path: {stderr}"
    );
    assert!(
        stderr.contains("Fix or remove the invalid config file and try again."),
        "rollback config failures should print the standard recovery hint: {stderr}"
    );
    assert!(
        !stderr.contains("error: rollback failed:"),
        "rollback config failures should not use the generic rollback-failed wrapper: {stderr}"
    );
}
