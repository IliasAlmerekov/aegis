use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use aegis::audit::{AuditEntry, AuditLogger, AuditSnapshot, Decision};
use aegis::interceptor::RiskLevel;
use tempfile::TempDir;

fn aegis_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aegis"))
}

fn run_prune(home: &Path, extra_args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(aegis_bin());
    cmd.env("HOME", home)
        .env("AEGIS_CI", "0")
        .current_dir(home)
        .args(["snapshot", "prune"])
        .args(extra_args);
    cmd.output().unwrap()
}

fn run_snapshot_list(home: &Path) -> std::process::Output {
    let mut cmd = Command::new(aegis_bin());
    cmd.env("HOME", home)
        .env("AEGIS_CI", "0")
        .current_dir(home)
        .args(["snapshot", "list"]);
    cmd.output().unwrap()
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

fn write_prune_policy(home: &Path) {
    let config_path = home.join(".aegis.toml");
    fs::write(&config_path, "[prune]\nenabled = true\nmax_age_days = 0\n").unwrap();
}

fn git_snapshot_id(home: &Path, name: &str) -> String {
    // Git snapshot ids are encoded as "<cwd>\t<stash-hash>". Use a non-existent
    // cwd so that GitPlugin::delete treats the artifact as already removed and
    // prune can append a Pruned audit entry idempotently.
    format!("{}\t{name}", home.join("nonexistent-repo").display())
}

fn last_audit_decision(home: &Path) -> Decision {
    let logger = AuditLogger::new(home.join(".aegis").join("audit.jsonl"));
    let entries = logger.read_all().unwrap();
    entries
        .last()
        .expect("audit log must contain entries")
        .as_base()
        .decision
}

fn append_raw_pruned_entry(home: &Path, snapshot_id: &str) {
    use std::fs::OpenOptions;
    use std::io::Write;

    let log = home.join(".aegis").join("audit.jsonl");
    fs::create_dir_all(log.parent().unwrap()).unwrap();
    let command = format!("aegis prune {}", snapshot_id);
    let escaped_command = serde_json::to_string(&command).unwrap();
    let line = format!(
        "{{\"timestamp\":\"2026-01-02T00:00:00Z\",\"sequence\":2,\"command\":{},\"risk\":\"Safe\",\"matched_patterns\":[],\"pattern_ids\":[],\"decision\":\"Pruned\",\"snapshots\":[],\"sandbox_status\":\"NotConfigured\"}}\n",
        escaped_command
    );
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .unwrap();
    file.write_all(line.as_bytes()).unwrap();
}

#[test]
fn test_prune_default_shows_dry_run_preview() {
    let home = TempDir::new().unwrap();
    write_prune_policy(home.path());
    let snapshot_id = git_snapshot_id(home.path(), "deadbeef");
    seed_snapshot(home.path(), "git", &snapshot_id);

    let output = run_prune(home.path(), &[]);

    assert!(
        output.status.success(),
        "default prune must exit 0: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.to_lowercase().contains("would prune"),
        "default prune must be a dry-run preview: stdout=\n{stdout}"
    );
    assert!(
        stdout.contains(&snapshot_id),
        "default prune must name the candidate id: stdout=\n{stdout}"
    );
}

#[test]
fn test_prune_yes_flag_executes_deletion() {
    let home = TempDir::new().unwrap();
    write_prune_policy(home.path());
    let snapshot_id = git_snapshot_id(home.path(), "deadbeef");
    seed_snapshot(home.path(), "git", &snapshot_id);

    let output = run_prune(home.path(), &["--yes"]);

    assert!(
        output.status.success(),
        "prune --yes must execute: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let list_output = run_snapshot_list(home.path());
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(
        !list_stdout.contains(&snapshot_id),
        "snapshot list must hide pruned id: stdout=\n{list_stdout}"
    );

    assert_eq!(
        last_audit_decision(home.path()),
        Decision::Pruned,
        "audit log must end with a Pruned entry"
    );
}

#[test]
fn test_prune_dry_run_and_yes_conflict() {
    let home = TempDir::new().unwrap();
    write_prune_policy(home.path());
    seed_snapshot(
        home.path(),
        "git",
        &git_snapshot_id(home.path(), "deadbeef"),
    );

    let output = run_prune(home.path(), &["--dry-run", "--yes"]);

    assert!(
        !output.status.success(),
        "--dry-run with --yes must fail: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(
        stderr.contains("conflict") || stderr.contains("cannot"),
        "error must explain the flag conflict: stderr=\n{stderr}"
    );
}

#[test]
fn test_prune_yes_reports_delete_failure_as_error() {
    let home = TempDir::new().unwrap();
    write_prune_policy(home.path());
    // A malformed git snapshot id (no tab separator) forces GitPlugin::delete
    // to fail with a malformed-id error instead of treating the artifact as
    // already removed. Prune must surface that failure, not swallow it.
    let snapshot_id = "not-a-tab-separator";
    seed_snapshot(home.path(), "git", snapshot_id);

    let output = run_prune(home.path(), &["--yes"]);

    assert!(
        !output.status.success(),
        "prune --yes must fail when a backend delete fails: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(
        stderr.contains("malformed") || stderr.contains("prune failed"),
        "stderr must surface the delete failure: stderr=\n{stderr}"
    );
}

#[test]
fn test_prune_skips_already_pruned_snapshot_in_command_field() {
    let home = TempDir::new().unwrap();
    write_prune_policy(home.path());
    let snapshot_id = git_snapshot_id(home.path(), "deadbeef");
    seed_snapshot(home.path(), "git", &snapshot_id);
    append_raw_pruned_entry(home.path(), &snapshot_id);

    let output = run_prune(home.path(), &["--yes"]);

    assert!(
        output.status.success(),
        "prune --yes must succeed: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let logger = AuditLogger::new(home.path().join(".aegis").join("audit.jsonl"));
    let pruned_entries: Vec<_> = logger
        .read_all()
        .unwrap()
        .into_iter()
        .filter(|e| e.as_base().decision == Decision::Pruned)
        .collect();
    assert_eq!(
        pruned_entries.len(),
        1,
        "a snapshot already recorded as pruned in the command field must not be re-pruned: {pruned_entries:?}"
    );
}
