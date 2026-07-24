//! Block behavior and policy: danger denial, intrinsic Block bypass
//! prevention, custom-pattern classification, fail-closed config regressions,
//! non-interactive denial, and mode/CI-policy block semantics.
//!
//! Split from the original `tests/full_pipeline.rs` (behavior-preserving move).

mod support;

use std::fs;
use std::io::Write;
use std::process::Stdio;

use tempfile::TempDir;

use support::*;

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
[[allow]]
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
        stderr.contains("blocked by an explicit block-level pattern"),
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

/// Audit mode must block intrinsic Block-level commands — RiskLevel::Block is
/// never bypassable, not even in Audit mode with ci_policy = Block.
#[test]
fn audit_mode_blocks_intrinsic_block_classification() {
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
[[allow]]
pattern = "rm -rf /"
cwd = "/aegis-test-scope"
reason = "allowlist should not bypass intrinsic block"
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

    // Command must be blocked — the process exits non-zero and rm is not invoked.
    assert!(!output.status.success());
    assert!(!log_path.exists());

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["decision"], "Blocked");
    assert_eq!(entries[0]["risk"], "Block");
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
    write_global_config(home.path(), "ci_policy = \"Allow\"\n");
    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
mode = "Strict"
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

#[test]
fn language_aware_confirmation_cannot_be_persisted_from_shell() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let target = workspace.path().join("artifact.txt");
    fs::write(&target, "keep").unwrap();
    let command = format!(
        "python3 -c 'import os; os.remove(\"{}\")'",
        target.display()
    );

    let mut child = base_command(home.path())
        .current_dir(workspace.path())
        .env("AEGIS_FORCE_INTERACTIVE", "1")
        .args(["-c", &command])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"always\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(
        target.exists(),
        "a rejected persistent approval must not execute"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Approve this language-aware assessment once?"),
        "the shell must present the non-persistable language-analysis prompt"
    );
    assert!(
        !workspace.path().join(".aegis.toml").exists(),
        "language-analysis approval must never append an allow rule"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries[0]["decision"], "Denied");
    assert!(
        matches!(
            entries[0]["analysis"]["status"].as_str(),
            Some("complete" | "degraded")
        ),
        "the bounded worker may degrade under concurrent test load"
    );
}

#[test]
fn strict_analysis_override_approves_exactly_once_from_shell() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let target = workspace.path().join("artifact.txt");
    fs::write(&target, "delete").unwrap();
    write_global_config(home.path(), "mode = \"Strict\"\n");
    let command = "printf '%s' \"$PAYLOAD\" | python3";
    let payload = format!("import os\nos.remove({:?})\n", target.display().to_string());

    let mut child = base_command(home.path())
        .current_dir(workspace.path())
        .env("AEGIS_FORCE_INTERACTIVE", "1")
        .env("PAYLOAD", payload)
        .args(["-c", command])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(b"yes\n").unwrap();
    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(!target.exists(), "the approved command must execute once");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Approve one-time analysis override?"),
        "Strict must use the narrow Analysis override prompt"
    );
    assert!(!workspace.path().join(".aegis.toml").exists());

    let entries = read_audit_entries(home.path());
    assert_eq!(entries[0]["decision"], "Approved");
    assert_eq!(entries[0]["analysis"]["status"], "degraded");
}
