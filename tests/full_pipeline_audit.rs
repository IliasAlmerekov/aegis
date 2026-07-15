//! Audit projection: shell audit-log fields, audit-logger failure modes,
//! rotation, and the `aegis audit` subcommand (export/filter/summary).
//!
//! Split from the original `tests/full_pipeline.rs` (behavior-preserving move).

mod support;

use std::fs;

use serde_json::Value;
use tempfile::TempDir;

use support::*;

#[test]
fn planner_migration_keeps_shell_audit_projection_fields() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "printf hello"])
        .output()
        .unwrap();

    assert!(output.status.success());

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["mode"], "Protect");
    assert_eq!(entries[0]["ci_detected"], serde_json::json!(false));
    assert_eq!(entries[0]["allowlist_matched"], serde_json::json!(false));
    assert_eq!(entries[0]["allowlist_effective"], serde_json::json!(false));
}

// Audit logger failure ──────────────────────────────────────────────────────

/// If `~/.aegis` is a file instead of a directory, audit append fails.
/// The binary must exit non-zero and print an error — audit is a security
/// artifact; silently dropping write failures defeats integrity checking.
#[test]
fn audit_logger_failure_exits_nonzero_with_error() {
    let home = TempDir::new().unwrap();
    fs::write(home.path().join(".aegis"), "I am a file, not a directory").unwrap();

    let output = base_command(home.path())
        .args(["-c", "echo hello"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "binary must exit non-zero when audit log is unwritable"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("failed to write audit log"),
        "error message must be printed to stderr when audit append fails"
    );
}

/// Audit write failures must always be reported — not gated on --verbose.
#[test]
fn audit_logger_failure_always_prints_error() {
    let home = TempDir::new().unwrap();
    fs::write(home.path().join(".aegis"), "I am a file, not a directory").unwrap();

    let output = base_command(home.path())
        .args(["-c", "echo hello"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("failed to write audit log"),
        "audit failure must print an error regardless of verbose flag"
    );
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
