use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use aegis::audit::{AuditLogger, Decision};
use tempfile::TempDir;

fn aegis_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aegis"))
}

fn run_prune(home: &Path, extra_args: &[&str]) -> Output {
    let mut cmd = Command::new(aegis_bin());
    cmd.env("HOME", home)
        .env("AEGIS_CI", "0")
        .current_dir(home)
        .args(["snapshot", "prune"])
        .args(extra_args);
    cmd.output().unwrap()
}

fn run_git(args: &[&str], cwd: &Path) -> Output {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(cwd);
    cmd.output().unwrap()
}

fn init_git_repo(repo: &Path) {
    run_git(&["init"], repo);
    run_git(&["config", "user.email", "test@aegis.dev"], repo);
    run_git(&["config", "user.name", "Aegis Test"], repo);
    run_git(&["commit", "--allow-empty", "-m", "init"], repo);
}

fn create_stash(repo: &Path, name: &str) -> String {
    fs::write(repo.join(name), format!("{name}\n")).unwrap();
    run_git(&["add", name], repo);
    let message = format!("aegis-snap-{name}");
    run_git(&["stash", "push", "-m", &message], repo);
    let out = run_git(&["rev-parse", "stash@{0}"], repo);
    let hash = String::from_utf8_lossy(&out.stdout).trim().to_string();
    format!("{}{}{}", repo.display(), '\t', hash)
}

fn seed_snapshot_record(home: &Path, snapshot_id: &str, timestamp: &str) {
    let log = home.join(".aegis").join("audit.jsonl");
    fs::create_dir_all(log.parent().unwrap()).unwrap();
    let line = format!(
        "{{\"timestamp\":\"{}\",\"sequence\":1,\"command\":\"rm -rf src\",\"risk\":\"Danger\",\"matched_patterns\":[],\"pattern_ids\":[],\"decision\":\"Approved\",\"snapshots\":[]}}\n",
        timestamp
    );

    // The seeded JSON is an audit record with snapshots outside the JSON body, because
    // `snapshot_id` may contain a tab and a fully escaped JSON string is easier to
    // compose by inserting the snapshot list after the initial serialization.
    let snapshots = format!(
        "{{\"plugin\":\"git\",\"snapshot_id\":\"{}\"}}",
        snapshot_id.replace('\t', "\\t")
    );
    let line = line.replace(
        "\"snapshots\":[]",
        &format!("\"snapshots\":[{}]", snapshots),
    );

    use std::fs::OpenOptions;
    use std::io::Write;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .unwrap();
    file.write_all(line.as_bytes()).unwrap();
}

fn stash_list(repo: &Path) -> Vec<String> {
    let out = run_git(&["stash", "list", "--format=%gd %s"], repo);
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

fn last_audit_decision(home: &Path) -> Option<Decision> {
    let logger = AuditLogger::new(home.join(".aegis").join("audit.jsonl"));
    logger
        .read_all()
        .ok()
        .and_then(|entries| entries.last().map(|e| e.as_base().decision))
}

#[test]
fn test_git_prune_retention_deletes_outside_window() {
    let home = TempDir::new().unwrap();
    let repo = home.path().join("repo");
    fs::create_dir(&repo).unwrap();
    init_git_repo(&repo);

    let old_snapshot_id = create_stash(&repo, "old-file");
    let new_snapshot_id = create_stash(&repo, "new-file");

    // Seed audit records so prune can resolve the real snapshot ids.
    seed_snapshot_record(home.path(), &old_snapshot_id, "2024-01-01T00:00:00Z");
    seed_snapshot_record(home.path(), &new_snapshot_id, "2026-01-01T00:00:00Z");

    let config_path = home.path().join(".aegis.toml");
    fs::write(
        &config_path,
        "[prune]\nenabled = true\nmax_count_per_provider = 1\n",
    )
    .unwrap();

    let output = run_prune(home.path(), &["--yes"]);

    assert!(
        output.status.success(),
        "prune --yes must succeed: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let remaining = stash_list(&repo);
    assert!(
        !remaining
            .iter()
            .any(|ref_name| ref_name.contains("old-file")),
        "old stash must be pruned: {:?}",
        remaining
    );
    assert!(
        remaining
            .iter()
            .any(|ref_name| ref_name.contains("new-file")),
        "new stash must be kept: {:?}",
        remaining
    );

    let decision = last_audit_decision(home.path());
    assert_eq!(
        decision,
        Some(Decision::Pruned),
        "audit log must end with a Pruned entry"
    );
}
