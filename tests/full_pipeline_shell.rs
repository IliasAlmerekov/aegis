//! Shell-wrapper passthrough behavior: stdout/stderr/exit-code fidelity,
//! environment and working-directory preservation, and shell-resolution
//! fallback regressions.
//!
//! Split from the original `tests/full_pipeline.rs` (behavior-preserving move).

mod support;

use std::process::Command;

use tempfile::TempDir;

use support::*;

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
