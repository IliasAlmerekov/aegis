//! Red tests for M1.3 snapshot ordering.
//!
//! These tests enforce the PRD contract that snapshots are created only after a
//! dangerous command is approved and before it runs; blocked and denied commands
//! must never create snapshots.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use tempfile::TempDir;

fn aegis_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aegis"))
}

fn base_command(home: &Path) -> Command {
    base_command_with_shell(home, Path::new("/bin/sh"))
}

fn base_command_with_shell(home: &Path, shell: &Path) -> Command {
    let mut command = Command::new(aegis_bin());
    command.env("AEGIS_REAL_SHELL", shell);
    command.env("AEGIS_CI", "0");
    command.env("HOME", home);
    command
}

#[cfg(unix)]
fn sandbox_backend_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        Command::new("bwrap")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(target_os = "macos")]
    {
        const PROBE: &str =
            "(version 1)\n(deny default)\n(allow process*)\n(allow file-read*)\n(allow signal*)\n";
        Command::new("/usr/bin/sandbox-exec")
            .args(["-p", PROBE, "/usr/bin/true"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        false
    }
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

fn init_git_repo(path: &Path) {
    let init = Command::new("git")
        .arg("init")
        .current_dir(path)
        .output()
        .expect("git init");
    assert!(init.status.success(), "git init failed: {init:?}");

    let commit = Command::new("git")
        .args([
            "-c",
            "user.email=test@aegis.dev",
            "-c",
            "user.name=Aegis Test",
            "commit",
            "--allow-empty",
            "-m",
            "init",
        ])
        .current_dir(path)
        .output()
        .expect("git commit");
    assert!(commit.status.success(), "git commit failed: {commit:?}");
}

fn canonical_test_path(path: &Path) -> PathBuf {
    path.canonicalize()
        .unwrap_or_else(|err| panic!("failed to canonicalize test path {}: {err}", path.display()))
}

/// Write a global user config (`~/.config/aegis/config.toml`) with the given
/// contents.
///
/// The project layer can no longer weaken `allowlist_override_level` (C3
/// security ratchet). Tests needing a permissive override must set it in the
/// trusted global config.
fn write_global_config(home: &Path, contents: &str) {
    let global_dir = home.join(".config/aegis");
    fs::create_dir_all(&global_dir).unwrap();
    fs::write(global_dir.join("config.toml"), contents).unwrap();
}

fn aegis_watch_in(home: &Path, cwd: &Path, input: &[u8]) -> std::process::Output {
    let mut child = Command::new(aegis_bin())
        .arg("watch")
        .env("AEGIS_REAL_SHELL", "/bin/sh")
        .env("AEGIS_CI", "0")
        .env("AEGIS_FORCE_NO_TTY", "1")
        .env("HOME", home)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn aegis watch");

    if let Err(err) = child.stdin.as_mut().unwrap().write_all(input) {
        assert!(
            err.kind() == std::io::ErrorKind::BrokenPipe,
            "failed to write watch input: {err}"
        );
    }
    drop(child.stdin.take());

    child.wait_with_output().expect("wait for aegis watch")
}

fn parse_frames(stdout: &[u8]) -> Vec<Value> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("invalid NDJSON frame"))
        .collect()
}

#[test]
fn test_denied_danger_command_records_no_snapshots() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    init_git_repo(cwd.path());

    let mut child = base_command(home.path())
        .current_dir(cwd.path())
        .env("AEGIS_FORCE_INTERACTIVE", "1")
        .args(["-c", "rm -rf /tmp/aegis-denied-target"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn aegis");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"no\n")
        .expect("write denial");
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("wait for aegis");
    assert_eq!(
        output.status.code(),
        Some(2),
        "denied command must exit 2, stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1, "exactly one audit entry expected");
    assert_eq!(entries[0]["decision"], "Denied");
    assert_eq!(entries[0]["risk"], "Danger");

    let snapshots = entries[0]["snapshots"]
        .as_array()
        .expect("snapshots must be an array");
    assert!(
        snapshots.is_empty(),
        "denied danger command must record no snapshots, got {snapshots:?}"
    );
}

#[test]
fn test_watch_mode_denied_danger_command_records_no_snapshots() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    init_git_repo(cwd.path());

    let input = b"{\"cmd\":\"rm -rf /tmp/aegis-watch-denied\",\"id\":\"denied-1\"}\n";
    let output = aegis_watch_in(home.path(), cwd.path(), input);

    assert!(
        output.status.success(),
        "watch must exit 0 on a single denied frame, stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let frames = parse_frames(&output.stdout);
    let result = frames
        .iter()
        .find(|f| f["type"] == "result")
        .expect("result frame");
    assert_eq!(result["decision"], "denied");
    assert_eq!(result["exit_code"], 2);

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1, "exactly one audit entry expected");
    assert_eq!(entries[0]["decision"], "Denied");
    assert_eq!(entries[0]["risk"], "Danger");

    let snapshots = entries[0]["snapshots"]
        .as_array()
        .expect("snapshots must be an array");
    assert!(
        snapshots.is_empty(),
        "denied watch danger command must record no snapshots, got {snapshots:?}"
    );
}

#[test]
fn test_watch_mode_approved_danger_command_records_snapshots_before_exec() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    init_git_repo(cwd.path());
    let cwd_path = canonical_test_path(cwd.path());

    write_global_config(home.path(), "allowlist_override_level = \"Danger\"\n");
    fs::write(
        cwd_path.join(".aegis.toml"),
        format!(
            r#"
[[allow]]
pattern = "rm -rf /tmp/aegis-watch-approved"
cwd = "{}"
reason = "approved watch test"
            "#,
            cwd_path.display()
        ),
    )
    .unwrap();

    let input = b"{\"cmd\":\"rm -rf /tmp/aegis-watch-approved\",\"id\":\"approved-1\"}\n";
    let output = aegis_watch_in(home.path(), &cwd_path, input);

    assert!(
        output.status.success(),
        "watch must exit 0 on a single approved frame, stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let frames = parse_frames(&output.stdout);
    let result = frames
        .iter()
        .find(|f| f["type"] == "result")
        .expect("result frame");
    assert_eq!(result["decision"], "approved");

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1, "exactly one audit entry expected");
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Danger");

    let snapshots = entries[0]["snapshots"]
        .as_array()
        .expect("snapshots must be an array");
    assert!(
        !snapshots.is_empty(),
        "approved watch danger command must record snapshots, got {snapshots:?}"
    );
}

#[test]
fn test_blocked_danger_command_records_no_snapshots() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    init_git_repo(cwd.path());

    let output = base_command(home.path())
        .current_dir(cwd.path())
        .args(["-c", "rm -rf /"])
        .output()
        .expect("run aegis shell wrapper");

    assert_eq!(
        output.status.code(),
        Some(3),
        "blocked command must exit 3, stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1, "exactly one audit entry expected");
    assert_eq!(entries[0]["decision"], "Blocked");
    assert_eq!(entries[0]["risk"], "Block");

    let snapshots = entries[0]["snapshots"]
        .as_array()
        .expect("snapshots must be an array");
    assert!(
        snapshots.is_empty(),
        "blocked danger command must record no snapshots, got {snapshots:?}"
    );
}

#[test]
fn test_watch_mode_denied_danger_command_audit_failure_emits_error_no_result() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    init_git_repo(cwd.path());

    // Force audit append to fail by making the ~/.aegis directory path a regular
    // file. create_dir_all on the audit parent will fail, so the denied frame
    // must not be downgraded to a normal result frame.
    fs::write(home.path().join(".aegis"), "not a directory").unwrap();

    let input =
        b"{\"cmd\":\"rm -rf /tmp/aegis-watch-denied-audit-fail\",\"id\":\"denied-audit-fail-1\"}\n";
    let output = aegis_watch_in(home.path(), cwd.path(), input);

    assert!(
        output.status.success(),
        "watch must exit 0 on a single frame even on audit failure, stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let frames = parse_frames(&output.stdout);

    let result_frames: Vec<_> = frames.iter().filter(|f| f["type"] == "result").collect();
    assert!(
        result_frames.is_empty(),
        "audit failure must not emit a normal denied/blocked result frame, got {result_frames:?}"
    );

    let error_frames: Vec<_> = frames.iter().filter(|f| f["type"] == "error").collect();
    assert!(
        !error_frames.is_empty(),
        "audit failure must emit an error frame, got {frames:?}"
    );
    assert_eq!(error_frames[0]["exit_code"], 4);
    let message = error_frames[0]["message"].as_str().unwrap_or("");
    assert!(
        message.contains("audit log write failed"),
        "error frame must mention audit write failure, got {message:?}"
    );
}

#[test]
fn test_watch_mode_approved_danger_command_child_observes_snapshot_before_exec() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    init_git_repo(cwd.path());
    let cwd_path = canonical_test_path(cwd.path());

    // Commit a baseline file so the repo is valid for stashing.
    let baseline = cwd_path.join("baseline.txt");
    fs::write(&baseline, "baseline\n").unwrap();
    let add = Command::new("git")
        .args(["add", "baseline.txt"])
        .current_dir(&cwd_path)
        .output()
        .expect("git add baseline");
    assert!(add.status.success(), "git add failed: {add:?}");
    let commit = Command::new("git")
        .args([
            "-c",
            "user.email=test@aegis.dev",
            "-c",
            "user.name=Aegis Test",
            "commit",
            "-m",
            "baseline",
        ])
        .current_dir(&cwd_path)
        .output()
        .expect("git commit baseline");
    assert!(commit.status.success(), "git commit failed: {commit:?}");

    // Create an untracked marker file. The git snapshot must stash it before
    // the child runs, so the child should not see it.
    let marker = cwd_path.join("marker.txt");
    fs::write(&marker, "present\n").unwrap();

    write_global_config(home.path(), "allowlist_override_level = \"Danger\"\n");
    fs::write(
        cwd_path.join(".aegis.toml"),
        format!(
            r#"
[[allow]]
pattern = "rm -rf /tmp/aegis-watch-before-exec*"
cwd = "{}"
reason = "approved before-exec test"
            "#,
            cwd_path.display()
        ),
    )
    .unwrap();

    // If the snapshot ran before the child, marker.txt is gone and the test
    // command succeeds. If the child ran first, marker.txt still exists, the
    // test fails, and the shell exits non-zero. Use a newline to separate the
    // harmless rm from the marker assertion so the allowlist glob (which
    // excludes `;`, `&`, and `|`) still matches the whole command.
    let input = "{\"cmd\":\"rm -rf /tmp/aegis-watch-before-exec\\ntest ! -f marker.txt\",\"id\":\"before-exec-1\"}\n";
    let output = aegis_watch_in(home.path(), &cwd_path, input.as_bytes());

    assert!(
        output.status.success(),
        "watch must exit 0 on a single approved frame, stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let frames = parse_frames(&output.stdout);
    assert!(
        !frames.is_empty(),
        "expected at least one output frame, got stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let result = frames
        .iter()
        .find(|f| f["type"] == "result")
        .unwrap_or_else(|| {
            panic!(
                "expected result frame, got frames {frames:?}\nstdout:\n{}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
    assert_eq!(result["decision"], "approved", "frames: {frames:?}");
    assert_eq!(
        result["exit_code"], 0,
        "child must observe marker.txt already stashed (snapshot before exec), frames: {frames:?}"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1, "exactly one audit entry expected");
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Danger");

    let snapshots = entries[0]["snapshots"]
        .as_array()
        .expect("snapshots must be an array");
    assert!(
        !snapshots.is_empty(),
        "approved watch danger command must record snapshots, got {snapshots:?}"
    );
}

#[test]
fn test_shell_approved_danger_command_child_observes_snapshot_before_exec() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    init_git_repo(cwd.path());
    let cwd_path = canonical_test_path(cwd.path());

    // Commit a baseline file so the repo is valid for stashing.
    let baseline = cwd_path.join("baseline.txt");
    fs::write(&baseline, "baseline\n").unwrap();
    let add = Command::new("git")
        .args(["add", "baseline.txt"])
        .current_dir(&cwd_path)
        .output()
        .expect("git add baseline");
    assert!(add.status.success(), "git add failed: {add:?}");
    let commit = Command::new("git")
        .args([
            "-c",
            "user.email=test@aegis.dev",
            "-c",
            "user.name=Aegis Test",
            "commit",
            "-m",
            "baseline",
        ])
        .current_dir(&cwd_path)
        .output()
        .expect("git commit baseline");
    assert!(commit.status.success(), "git commit failed: {commit:?}");

    // Create an untracked marker file. The git snapshot must stash it before
    // the child runs, so the child should not see it.
    let marker = cwd_path.join("marker.txt");
    fs::write(&marker, "present\n").unwrap();

    write_global_config(home.path(), "allowlist_override_level = \"Danger\"\n");
    fs::write(
        cwd_path.join(".aegis.toml"),
        format!(
            r#"
[[allow]]
pattern = "rm -rf /tmp/aegis-shell-before-exec*"
cwd = "{}"
reason = "approved shell before-exec test"
            "#,
            cwd_path.display()
        ),
    )
    .unwrap();

    // If the snapshot ran before the child, marker.txt is gone and the test
    // command succeeds. If the child ran first, marker.txt still exists, the
    // test fails, and the shell exits non-zero. Use a newline to separate the
    // harmless rm from the marker assertion so the allowlist glob (which
    // excludes `;`, `&`, and `|`) still matches the whole command.
    let output = base_command(home.path())
        .current_dir(&cwd_path)
        .args([
            "-c",
            "rm -rf /tmp/aegis-shell-before-exec\ntest ! -f marker.txt",
        ])
        .output()
        .expect("run aegis shell wrapper");

    assert!(
        output.status.success(),
        "shell child must observe marker.txt already stashed (snapshot before exec), stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1, "exactly one audit entry expected");
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Danger");

    let snapshots = entries[0]["snapshots"]
        .as_array()
        .expect("snapshots must be an array");
    assert!(
        !snapshots.is_empty(),
        "approved shell danger command must record snapshots, got {snapshots:?}"
    );
}

#[cfg(unix)]
#[test]
fn test_sandboxed_approved_danger_command_records_snapshots_before_exec() {
    if !sandbox_backend_available() {
        println!("sandbox backend not available on this host; skipping");
        return;
    }

    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    init_git_repo(cwd.path());
    let cwd_path = canonical_test_path(cwd.path());

    // Commit a baseline file so the repo is valid for stashing.
    let baseline = cwd_path.join("baseline.txt");
    fs::write(&baseline, "baseline\n").unwrap();
    let add = Command::new("git")
        .args(["add", "baseline.txt"])
        .current_dir(&cwd_path)
        .output()
        .expect("git add baseline");
    assert!(add.status.success(), "git add failed: {add:?}");
    let commit = Command::new("git")
        .args([
            "-c",
            "user.email=test@aegis.dev",
            "-c",
            "user.name=Aegis Test",
            "commit",
            "-m",
            "baseline",
        ])
        .current_dir(&cwd_path)
        .output()
        .expect("git commit baseline");
    assert!(commit.status.success(), "git commit failed: {commit:?}");

    // Create an untracked marker file. The git snapshot must stash it before
    // the child runs, so the child should not see it.
    let marker = cwd_path.join("marker.txt");
    fs::write(&marker, "present\n").unwrap();

    // bwrap on Linux does not bind /bin, so use a shell located under /usr so
    // the sandboxed child can actually exec it.
    let shell = Path::new("/usr/bin/bash");
    if !shell.exists() {
        println!("{shell:?} not present; skipping sandboxed ordering test");
        return;
    }

    write_global_config(home.path(), "allowlist_override_level = \"Danger\"\n");
    fs::write(
        cwd_path.join(".aegis.toml"),
        format!(
            r#"
[[allow]]
pattern = "rm -rf /tmp/aegis-sandbox-before-exec*"
cwd = "{}"
reason = "approved sandbox before-exec test"

[sandbox]
enabled = true
required = false
allow_write = ["{}"]
allow_network = false
            "#,
            cwd_path.display(),
            cwd_path.display()
        ),
    )
    .unwrap();

    let output = base_command_with_shell(home.path(), shell)
        .current_dir(&cwd_path)
        .args([
            "-c",
            "rm -rf /tmp/aegis-sandbox-before-exec\ntest ! -f marker.txt",
        ])
        .output()
        .expect("run aegis shell wrapper");

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success()
        && stderr.contains("landlock restrict_self: Operation not permitted")
    {
        println!("landlock restrict_self unavailable in this environment; skipping");
        return;
    }

    assert!(
        output.status.success(),
        "sandboxed child must observe marker.txt already stashed (snapshot before exec), stderr:\n{stderr}"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1, "exactly one audit entry expected");
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Danger");
    assert_eq!(
        entries[0]["sandbox_status"], "active",
        "sandboxed command must record active sandbox status, got: {}",
        entries[0]["sandbox_status"]
    );

    let snapshots = entries[0]["snapshots"]
        .as_array()
        .expect("snapshots must be an array");
    assert!(
        !snapshots.is_empty(),
        "approved sandboxed danger command must record snapshots, got {snapshots:?}"
    );
}

#[test]
fn test_watch_mode_blocked_command_records_no_snapshots() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    init_git_repo(cwd.path());

    let input = b"{\"cmd\":\"rm -rf /\",\"id\":\"blocked-1\"}\n";
    let output = aegis_watch_in(home.path(), cwd.path(), input);

    assert!(
        output.status.success(),
        "watch must exit 0 on a single blocked frame, stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let frames = parse_frames(&output.stdout);
    let result = frames
        .iter()
        .find(|f| f["type"] == "result")
        .expect("result frame");
    assert_eq!(result["decision"], "blocked");
    assert_eq!(result["exit_code"], 3);

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1, "exactly one audit entry expected");
    assert_eq!(entries[0]["decision"], "Blocked");
    assert_eq!(entries[0]["risk"], "Block");

    let snapshots = entries[0]["snapshots"]
        .as_array()
        .expect("snapshots must be an array");
    assert!(
        snapshots.is_empty(),
        "blocked watch command must record no snapshots, got {snapshots:?}"
    );
}
