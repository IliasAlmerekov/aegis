//! Disabled-toggle behavior: when the `~/.aegis/disabled` toggle file is
//! present, the shell wrapper bypasses interception/auditing while the JSON
//! evaluation contract is preserved.
//!
//! Split from the original `tests/full_pipeline.rs` (behavior-preserving move).

mod support;

use serde_json::Value;
use tempfile::TempDir;

use support::*;

#[test]
fn disabled_shell_wrapper_text_passthrough_executes_without_audit() {
    let home = TempDir::new().unwrap();
    write_disabled_toggle(home.path());

    let output = base_command(home.path())
        .args(["-c", "printf passthrough"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "passthrough");
    assert!(output.stderr.is_empty());
    assert!(
        !home.path().join(".aegis").join("audit.jsonl").exists(),
        "disabled passthrough must bypass auditing"
    );
}

#[test]
fn disabled_shell_wrapper_json_mode_preserves_evaluation_contract() {
    let home = TempDir::new().unwrap();
    write_disabled_toggle(home.path());

    let output = base_command(home.path())
        .args(["-c", "echo hi", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "json mode must stay stderr-free even when the toggle is disabled"
    );

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["command"], "echo hi");
    assert_eq!(json["execution"]["mode"], "evaluation_only");
    assert_eq!(json["execution"]["will_execute"], false);
    assert_eq!(json["decision"], "auto_approve");
    assert!(
        !home.path().join(".aegis").join("audit.jsonl").exists(),
        "disabled json mode must continue to avoid audit writes"
    );
}

#[test]
fn disabled_toggle_bypasses_language_analysis_in_text_mode() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let target = workspace.path().join("artifact.txt");
    std::fs::write(&target, "delete").unwrap();
    write_disabled_toggle(home.path());
    let command = format!(
        "python3 -c 'import os; os.remove(\"{}\")'",
        target.display()
    );

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", &command])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(
        !target.exists(),
        "trusted Toggle posture must bypass analysis"
    );
    assert!(!home.path().join(".aegis").join("audit.jsonl").exists());
}
