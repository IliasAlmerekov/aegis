use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use serde_json::Value;
use tempfile::TempDir;

fn aegis_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aegis"))
}

fn base_command(home: &Path) -> Command {
    let mut command = Command::new(aegis_bin());
    command.env("AEGIS_REAL_SHELL", "/bin/sh");
    command.env("AEGIS_CI", "0");
    command.env("HOME", home);
    command
}

fn write_rotation_config(workspace: &Path, max_file_size_bytes: u64) {
    fs::write(
        workspace.join(".aegis.toml"),
        format!(
            r#"
[audit]
rotation_enabled = true
max_file_size_bytes = {max_file_size_bytes}
retention_files = 128
compress_rotated = false
"#
        ),
    )
    .unwrap();
}

fn write_rotation_config_with_compression(
    workspace: &Path,
    max_file_size_bytes: u64,
    compress_rotated: bool,
) {
    fs::write(
        workspace.join(".aegis.toml"),
        format!(
            r#"
[audit]
rotation_enabled = true
max_file_size_bytes = {max_file_size_bytes}
retention_files = 128
compress_rotated = {compress_rotated}
"#
        ),
    )
    .unwrap();
}

fn run_safe_command(home: &Path, workspace: &Path, command: &str) -> Output {
    base_command(home)
        .current_dir(workspace)
        .args(["-c", command])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .unwrap()
}

fn read_audit_json(home: &Path, workspace: &Path) -> Vec<Value> {
    let output = base_command(home)
        .current_dir(workspace)
        .args(["audit", "--format", "json"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "audit command failed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn concurrent_writers_do_not_corrupt_audit_log() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let writers = 24usize;
    let barrier = Arc::new(std::sync::Barrier::new(writers));

    let handles = (0..writers)
        .map(|index| {
            let home_path = home.path().to_path_buf();
            let workspace_path = workspace.path().to_path_buf();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let command = format!("printf writer-{index:02}");
                run_safe_command(&home_path, &workspace_path, &command)
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        let output = handle.join().unwrap();
        assert!(
            output.status.success(),
            "writer failed with status {:?}",
            output.status.code()
        );
    }

    let entries = read_audit_json(home.path(), workspace.path());
    assert_eq!(entries.len(), writers);

    let commands = entries
        .iter()
        .map(|entry| entry["command"].as_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    let expected = (0..writers)
        .map(|index| format!("printf writer-{index:02}"))
        .collect::<BTreeSet<_>>();
    assert_eq!(commands, expected);
}

#[test]
fn concurrent_writers_with_rotation_keep_audit_log_readable() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    write_rotation_config(workspace.path(), 350);

    let writers = 32usize;
    let barrier = Arc::new(std::sync::Barrier::new(writers));

    let handles = (0..writers)
        .map(|index| {
            let home_path = home.path().to_path_buf();
            let workspace_path = workspace.path().to_path_buf();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let payload = "x".repeat(80);
                let command = format!("printf writer-{index:02}-{payload}");
                run_safe_command(&home_path, &workspace_path, &command)
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        let output = handle.join().unwrap();
        assert!(
            output.status.success(),
            "writer failed with status {:?}",
            output.status.code()
        );
    }

    let entries = read_audit_json(home.path(), workspace.path());
    assert_eq!(entries.len(), writers);
    assert!(
        home.path().join(".aegis").join("audit.jsonl.1").exists(),
        "rotation should have produced at least one archive"
    );
}

#[test]
fn concurrent_reader_during_repeated_rotation_never_observes_broken_json() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    write_rotation_config(workspace.path(), 350);

    let writer_done = Arc::new(AtomicBool::new(false));

    let writer_home = home.path().to_path_buf();
    let writer_workspace = workspace.path().to_path_buf();
    let writer_done_signal = Arc::clone(&writer_done);
    let writer = thread::spawn(move || {
        for index in 0..40usize {
            let payload = "y".repeat(96);
            let command = format!("printf rotate-{index:02}-{payload}");
            let output = run_safe_command(&writer_home, &writer_workspace, &command);
            assert!(output.status.success(), "writer process failed");
        }
        writer_done_signal.store(true, Ordering::Release);
    });

    let reader_home = home.path().to_path_buf();
    let reader_workspace = workspace.path().to_path_buf();
    let reader_done_signal = Arc::clone(&writer_done);
    let reader = thread::spawn(move || {
        let mut successful_reads = 0usize;
        while !reader_done_signal.load(Ordering::Acquire) || successful_reads < 40 {
            let entries = read_audit_json(&reader_home, &reader_workspace);
            let _ = serde_json::to_vec(&entries).unwrap();
            successful_reads += 1;
        }
    });

    writer.join().unwrap();
    reader.join().unwrap();

    let entries = read_audit_json(home.path(), workspace.path());
    assert_eq!(entries.len(), 40);
}

#[test]
fn concurrent_multi_process_bursts_do_not_lose_or_duplicate_entries() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let workers = 12usize;
    let commands_per_worker = 5usize;
    let barrier = Arc::new(std::sync::Barrier::new(workers));

    let handles = (0..workers)
        .map(|worker| {
            let home_path = home.path().to_path_buf();
            let workspace_path = workspace.path().to_path_buf();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for command_index in 0..commands_per_worker {
                    let command = format!("printf worker-{worker:02}-cmd-{command_index:02}");
                    let output = run_safe_command(&home_path, &workspace_path, &command);
                    assert!(
                        output.status.success(),
                        "writer {worker} command {command_index} failed"
                    );
                }
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        handle.join().unwrap();
    }

    let entries = read_audit_json(home.path(), workspace.path());
    assert_eq!(entries.len(), workers * commands_per_worker);

    let commands = entries
        .iter()
        .map(|entry| entry["command"].as_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    let expected = (0..workers)
        .flat_map(|worker| {
            (0..commands_per_worker).map(move |command_index| {
                format!("printf worker-{worker:02}-cmd-{command_index:02}")
            })
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(commands, expected);
}

#[test]
fn concurrent_writers_with_compressed_rotation_keep_all_entries_readable() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    write_rotation_config_with_compression(workspace.path(), 350, true);

    let writers = 28usize;
    let barrier = Arc::new(std::sync::Barrier::new(writers));

    let handles = (0..writers)
        .map(|index| {
            let home_path = home.path().to_path_buf();
            let workspace_path = workspace.path().to_path_buf();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                let payload = "z".repeat(96);
                let command = format!("printf zipped-{index:02}-{payload}");
                run_safe_command(&home_path, &workspace_path, &command)
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        let output = handle.join().unwrap();
        assert!(output.status.success(), "writer failed");
    }

    let entries = read_audit_json(home.path(), workspace.path());
    assert_eq!(entries.len(), writers);
    assert!(
        home.path().join(".aegis").join("audit.jsonl.1.gz").exists(),
        "compressed rotation should produce a gz archive"
    );

    let commands = entries
        .iter()
        .map(|entry| entry["command"].as_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    let expected = (0..writers)
        .map(|index| format!("printf zipped-{index:02}-{}", "z".repeat(96)))
        .collect::<BTreeSet<_>>();
    assert_eq!(commands, expected);
}

#[test]
fn concurrent_reader_during_compressed_rotation_never_observes_data_loss() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    write_rotation_config_with_compression(workspace.path(), 320, true);

    let writer_done = Arc::new(AtomicBool::new(false));

    let writer_home = home.path().to_path_buf();
    let writer_workspace = workspace.path().to_path_buf();
    let writer_done_signal = Arc::clone(&writer_done);
    let writer = thread::spawn(move || {
        for index in 0..36usize {
            let payload = "c".repeat(110);
            let command = format!("printf compressed-{index:02}-{payload}");
            let output = run_safe_command(&writer_home, &writer_workspace, &command);
            assert!(output.status.success(), "writer process failed");
        }
        writer_done_signal.store(true, Ordering::Release);
    });

    let reader_home = home.path().to_path_buf();
    let reader_workspace = workspace.path().to_path_buf();
    let reader_done_signal = Arc::clone(&writer_done);
    let reader = thread::spawn(move || {
        let mut saw_nonempty_read = false;
        while !reader_done_signal.load(Ordering::Acquire) || !saw_nonempty_read {
            let entries = read_audit_json(&reader_home, &reader_workspace);
            if !entries.is_empty() {
                saw_nonempty_read = true;
            }
            let _ = serde_json::to_vec(&entries).unwrap();
        }
    });

    writer.join().unwrap();
    reader.join().unwrap();

    let entries = read_audit_json(home.path(), workspace.path());
    assert_eq!(entries.len(), 36);
    let commands = entries
        .iter()
        .map(|entry| entry["command"].as_str().unwrap().to_string())
        .collect::<BTreeSet<_>>();
    let expected = (0..36usize)
        .map(|index| format!("printf compressed-{index:02}-{}", "c".repeat(110)))
        .collect::<BTreeSet<_>>();
    assert_eq!(commands, expected);
}
