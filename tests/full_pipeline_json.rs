//! JSON planning (`--output json`): evaluation-only contract, decision
//! projection, snapshot-plan reporting, and stderr-free machine consumption.
//!
//! Split from the original `tests/full_pipeline.rs` (behavior-preserving move).

mod support;

use std::fs;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

use support::*;

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
fn planner_migration_keeps_json_block_reason_contract() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "rm -rf /", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["decision"], "block");
    assert_eq!(json["block_reason"], "intrinsic_risk_block");
}

#[test]
fn json_output_snapshot_policy_none_disables_snapshot_request_for_danger() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    write_global_config(home.path(), "snapshot_policy = \"None\"\n");
    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
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
auto_snapshot_git = true
auto_snapshot_docker = false
[[allow]]
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

    assert!(
        output.status.success(),
        "allowlisted danger in json mode must succeed; status: {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
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
