use std::path::{Path, PathBuf};
use std::process::Command;

use aegis::audit::{AuditEntry, AuditLogger, AuditSnapshot, Decision};
use aegis::interceptor::RiskLevel;
use tempfile::TempDir;

fn aegis_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aegis"))
}

fn run_snapshot_list(home: &Path) -> std::process::Output {
    Command::new(aegis_bin())
        .env("HOME", home)
        .env("AEGIS_CI", "0")
        .args(["snapshot", "list"])
        .output()
        .unwrap()
}

fn seed_snapshot(home: &Path, plugin: &str, snapshot_id: &str) {
    let logger = AuditLogger::new(home.join(".aegis").join("audit.jsonl"));
    logger
        .append(AuditEntry::new(
            "rm -rf src",
            RiskLevel::Danger,
            Vec::new(),
            Decision::Approved,
            vec![AuditSnapshot {
                plugin: plugin.to_string(),
                snapshot_id: snapshot_id.to_string(),
            }],
            None,
            None,
        ))
        .unwrap();
}

#[test]
fn snapshot_list_prints_recorded_snapshot() {
    let home = TempDir::new().unwrap();
    seed_snapshot(home.path(), "git", "snap-git-abc123");

    let output = run_snapshot_list(home.path());

    assert!(
        output.status.success(),
        "snapshot list failed: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("snap-git-abc123"), "stdout=\n{stdout}");
    assert!(stdout.contains("git"), "stdout=\n{stdout}");
}

#[test]
fn snapshot_list_on_empty_log_exits_zero_with_message() {
    let home = TempDir::new().unwrap();

    let output = run_snapshot_list(home.path());

    assert!(
        output.status.success(),
        "snapshot list must not error on an empty log: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    assert!(stdout.contains("no snapshot"), "stdout=\n{stdout}");
}
