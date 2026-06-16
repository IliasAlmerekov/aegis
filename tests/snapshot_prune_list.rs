use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn append_line(home: &Path, line: &str) {
    let log = home.join(".aegis").join("audit.jsonl");
    fs::create_dir_all(log.parent().unwrap()).unwrap();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .unwrap();
    writeln!(file, "{line}").unwrap();
}

fn snapshot_line(snapshot_id: &str) -> String {
    let escaped = serde_json::to_string(snapshot_id).unwrap();
    format!(
        "{{\"timestamp\":\"2026-01-01T00:00:00Z\",\"sequence\":1,\"command\":\"rm -rf src\",\"risk\":\"Danger\",\"matched_patterns\":[],\"pattern_ids\":[],\"decision\":\"Approved\",\"snapshots\":[{{\"plugin\":\"git\",\"snapshot_id\":{}}}],\"sandbox_status\":\"NotConfigured\"}}",
        escaped
    )
}

fn prune_line(snapshot_id: &str) -> String {
    let command = format!("aegis prune {}", snapshot_id);
    let escaped_command = serde_json::to_string(&command).unwrap();
    format!(
        "{{\"timestamp\":\"2026-01-02T00:00:00Z\",\"sequence\":2,\"command\":{},\"risk\":\"Safe\",\"matched_patterns\":[],\"pattern_ids\":[],\"decision\":\"Pruned\",\"snapshots\":[],\"sandbox_status\":\"NotConfigured\"}}",
        escaped_command
    )
}

#[test]
fn test_snapshot_list_hides_pruned_ids() {
    let home = TempDir::new().unwrap();
    append_line(home.path(), &snapshot_line("snap-to-hide"));
    append_line(home.path(), &prune_line("snap-to-hide"));

    let output = run_snapshot_list(home.path());

    assert!(
        output.status.success(),
        "snapshot list must succeed: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("snap-to-hide"),
        "pruned snapshot id must be hidden: stdout=\n{stdout}"
    );
}

#[test]
fn test_snapshot_list_hides_pruned_git_id_from_command_field() {
    let home = TempDir::new().unwrap();
    let git_id = format!("{}\t{}", home.path().join("repo").display(), "deadbeef1234");
    append_line(home.path(), &snapshot_line(&git_id));
    append_line(home.path(), &prune_line(&git_id));

    let output = run_snapshot_list(home.path());

    assert!(
        output.status.success(),
        "snapshot list must succeed: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains(&git_id),
        "pruned git snapshot id must be hidden: stdout=\n{stdout}"
    );
}

#[test]
fn test_snapshot_list_shows_note_when_pruned_ids_hidden() {
    let home = TempDir::new().unwrap();
    append_line(home.path(), &snapshot_line("snap-to-hide"));
    append_line(home.path(), &prune_line("snap-to-hide"));

    let output = run_snapshot_list(home.path());

    assert!(
        output.status.success(),
        "snapshot list must succeed: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    assert!(
        !stdout.contains("snap-to-hide"),
        "pruned snapshot id must be hidden: stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("hidden") || stdout.contains("pruned") || stdout.contains("dangling"),
        "snapshot list should note that pruned ids were hidden: stdout=\n{stdout}"
    );
}
