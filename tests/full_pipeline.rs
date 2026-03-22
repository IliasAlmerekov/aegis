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

#[test]
fn dangerous_command_denied_preserves_directory() {
    let home = TempDir::new().unwrap();
    let target_dir = std::env::temp_dir().join("test_aegis");

    let _ = fs::remove_dir_all(&target_dir);
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(target_dir.join("sentinel.txt"), "still here").unwrap();

    // Use home as CWD (not the project root) so the GitPlugin does not
    // git-stash the developer's uncommitted changes as a "snapshot".
    let mut child = base_command(home.path())
        .current_dir(home.path())
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
    // The audit log must record which allowlist rule fired so operators can
    // trace auto-approvals back to their config.
    assert_eq!(
        entries[0]["allowlist_pattern"],
        "terraform destroy -target=module.test.*"
    );
    assert_eq!(entries[1]["decision"], "Denied");
    assert_eq!(entries[1]["risk"], "Danger");
    // Non-matching command — allowlist_pattern field must be absent from JSON.
    assert!(entries[1].get("allowlist_pattern").is_none());
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
        r#"allowlist = ["rm -rf /"]
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

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "Blocked");
    assert_eq!(entries[0]["risk"], "Block");
    // The allowlist_pattern is still recorded in the audit log so the operator
    // can see that their allowlist entry was evaluated (and refused for Block).
    assert_eq!(entries[0]["allowlist_pattern"], "rm -rf /");
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
    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"allowlist = ["terraform destroy -target=module.ci.*"]
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

// ─────────────────────────────────────────────────────────────────────────────
// Regression tests: security-critical failure modes
// ─────────────────────────────────────────────────────────────────────────────

// Config parse failure ─────────────────────────────────────────────────────

/// A malformed `.aegis.toml` must not crash the binary.
/// `load_runtime_config` falls back to `Config::default()` on parse error.
#[test]
fn broken_config_toml_does_not_crash_safe_command_passes() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        "this is : broken toml {{{",
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", "echo hello"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(output.stdout, b"hello\n");
}

/// With a broken config, dangerous commands must still be intercepted.
/// The fallback is `Config::default()` — empty allowlist, Protect mode.
#[test]
fn broken_config_toml_dangerous_command_still_intercepted() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        "not_a_valid_field = true\nunknown_key = 42",
    )
    .unwrap();

    let mut child = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", "rm -rf /tmp/aegis_cfg_test_dir"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.as_mut().unwrap().write_all(b"no\n").unwrap();
    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("AEGIS INTERCEPTED"),
        "dangerous command must be intercepted even when config is broken"
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

/// A Warn-level command with stdin closed (EOF) is auto-approved because
/// empty input means "proceed" in the Warn prompt (`"" != "n"`).
/// Documents the current behaviour so any change is deliberate.
#[test]
fn warn_command_with_eof_stdin_is_auto_approved() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "git stash clear"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Command cancelled."),
        "empty stdin must NOT cancel a Warn command; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("AEGIS INTERCEPTED A SUSPICIOUS COMMAND"),
        "Warn command must show the interception dialog; stderr:\n{stderr}"
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
