use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::Value;
use tempfile::TempDir;

fn aegis_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aegis"))
}

fn shell_command(home: &Path, cwd: &Path) -> Command {
    let mut command = Command::new(aegis_bin());
    command
        .env("AEGIS_REAL_SHELL", "/bin/sh")
        .env("AEGIS_CI", "0")
        .env("HOME", home)
        .current_dir(cwd);
    command
}

#[cfg(target_os = "linux")]
fn single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn interactive_shell_output(home: &Path, cwd: &Path, response: &[u8]) -> Output {
    const PROMPT: &[u8] = b"Run once without recovery?";

    let mut command = Command::new("script");
    command
        .env("AEGIS_REAL_SHELL", "/bin/sh")
        .env("AEGIS_CI", "0")
        .env("HOME", home)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "linux")]
    command.args([
        "-qfec",
        &format!(
            "{} -c {}",
            single_quote(&aegis_bin().to_string_lossy()),
            single_quote("sh ./run.sh")
        ),
        "/dev/null",
    ]);

    #[cfg(target_os = "macos")]
    command
        .args(["-q", "/dev/null"])
        .arg(aegis_bin())
        .args(["-c", "sh ./run.sh"]);

    let mut child = command.spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (prompt_sender, prompt_receiver) = mpsc::channel();
    let stdout_reader = capture_until_prompt(stdout, PROMPT, prompt_sender.clone());
    let stderr_reader = capture_until_prompt(stderr, PROMPT, prompt_sender);

    if prompt_receiver
        .recv_timeout(Duration::from_secs(10))
        .is_err()
    {
        let _ = child.kill();
        let status = child.wait().unwrap();
        let stdout = stdout_reader.join().unwrap();
        let stderr = stderr_reader.join().unwrap();
        panic!(
            "recovery prompt was not observed (status {:?}): stdout=\n{}\nstderr=\n{}",
            status.code(),
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
    }

    child.stdin.as_mut().unwrap().write_all(response).unwrap();
    drop(child.stdin.take());
    let status = child.wait().unwrap();
    Output {
        status,
        stdout: stdout_reader.join().unwrap(),
        stderr: stderr_reader.join().unwrap(),
    }
}

fn capture_until_prompt<R>(
    mut reader: R,
    prompt: &'static [u8],
    prompt_sender: mpsc::Sender<()>,
) -> thread::JoinHandle<Vec<u8>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0_u8; 1024];
        let mut prompt_reported = false;
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(length) => {
                    output.extend_from_slice(&buffer[..length]);
                    if !prompt_reported && output.windows(prompt.len()).any(|part| part == prompt) {
                        prompt_reported = true;
                        let _ = prompt_sender.send(());
                    }
                }
                Err(error) => panic!("failed to read PTY transcript: {error}"),
            }
        }
        output
    })
}

fn read_single_audit_entry(home: &Path) -> Value {
    let contents = fs::read_to_string(home.join(".aegis").join("audit.jsonl")).unwrap();
    serde_json::from_str(contents.trim()).unwrap()
}

fn init_git_repo(path: &Path) {
    let init = Command::new("git")
        .arg("init")
        .current_dir(path)
        .output()
        .unwrap();
    assert!(init.status.success());
    let add = Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()
        .unwrap();
    assert!(add.status.success());
    let commit = Command::new("git")
        .args([
            "-c",
            "user.email=test@aegis.dev",
            "-c",
            "user.name=Aegis Test",
            "commit",
            "-m",
            "init",
        ])
        .current_dir(path)
        .output()
        .unwrap();
    assert!(commit.status.success());
}

#[test]
fn noninteractive_required_recovery_degradation_denies_before_child_execution() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let marker = workspace.path().join("executed");
    fs::write(workspace.path().join("run.sh"), "printf ran > executed\n").unwrap();

    let output = shell_command(home.path(), workspace.path())
        .stdin(Stdio::null())
        .args(["-c", "sh ./run.sh"])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!marker.exists(), "degraded command must not execute");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No required Snapshot was created"));

    let entry = read_single_audit_entry(home.path());
    assert_eq!(entry["decision"], "Denied");
    assert_eq!(entry["effect_opaque"], true);
    assert_eq!(entry["snapshots_required"], true);
    assert_eq!(entry["recovery_degradation"], "no_snapshot_available");
    assert_eq!(entry["snapshots"], serde_json::json!([]));
}

#[test]
fn force_interactive_env_cannot_enable_recovery_override_without_tty() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let marker = workspace.path().join("executed");
    fs::write(workspace.path().join("run.sh"), "printf ran > executed\n").unwrap();

    let mut child = shell_command(home.path(), workspace.path())
        .env("AEGIS_FORCE_INTERACTIVE", "1")
        .args(["-c", "sh ./run.sh"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(b"r\n").unwrap();
    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(
        !marker.exists(),
        "a pipe must not enable the recovery override"
    );
    let entry = read_single_audit_entry(home.path());
    assert_eq!(entry["decision"], "Denied");
    assert_eq!(entry["recovery_degradation"], "no_snapshot_available");
}

#[test]
fn interactive_recovery_deny_prevents_child_execution() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let marker = workspace.path().join("executed");
    fs::write(workspace.path().join("run.sh"), "printf ran > executed\n").unwrap();

    let output = interactive_shell_output(home.path(), workspace.path(), b"n\n");

    assert_eq!(output.status.code(), Some(2));
    assert!(!marker.exists());
    let entry = read_single_audit_entry(home.path());
    assert_eq!(entry["decision"], "Denied");
    assert_eq!(entry["recovery_degradation"], "no_snapshot_available");
}

#[test]
fn interactive_recovery_run_once_executes_and_records_human_approval() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let marker = workspace.path().join("executed");
    fs::write(workspace.path().join("run.sh"), "printf ran > executed\n").unwrap();

    let output = interactive_shell_output(home.path(), workspace.path(), b"r\n");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(marker.exists());
    let entry = read_single_audit_entry(home.path());
    assert_eq!(entry["decision"], "Approved");
    assert_eq!(entry["recovery_degradation"], "no_snapshot_available");
}

#[test]
fn successful_required_snapshot_executes_without_recovery_prompt() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let marker = workspace.path().join("executed");
    fs::write(workspace.path().join("run.sh"), "printf ran > executed\n").unwrap();
    fs::write(workspace.path().join("state.txt"), "before\n").unwrap();
    init_git_repo(workspace.path());
    fs::write(workspace.path().join("state.txt"), "changed\n").unwrap();

    let output = shell_command(home.path(), workspace.path())
        .stdin(Stdio::null())
        .args(["-c", "sh ./run.sh"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(marker.exists());
    assert!(output.stderr.is_empty(), "no Recovery prompt was expected");
    let entry = read_single_audit_entry(home.path());
    assert_eq!(entry["decision"], "AutoApproved");
    assert!(
        entry["snapshots"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(entry.get("recovery_degradation").is_none());
}

#[test]
fn degraded_audit_write_failure_remains_fail_closed() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let marker = workspace.path().join("executed");
    fs::write(home.path().join(".aegis"), "not a directory").unwrap();
    fs::write(workspace.path().join("run.sh"), "printf ran > executed\n").unwrap();

    let output = shell_command(home.path(), workspace.path())
        .stdin(Stdio::null())
        .args(["-c", "sh ./run.sh"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    assert!(!marker.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to write audit log"));
}

#[test]
fn evaluation_reports_required_plan_without_simulating_recovery_degradation() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    let output = shell_command(home.path(), workspace.path())
        .args(["-c", "sh ./run.sh", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["snapshot_plan"]["requested"], true);
    assert_eq!(
        value["snapshot_plan"]["applicable_plugins"],
        serde_json::json!([])
    );
    assert_eq!(value["execution"]["mode"], "evaluation_only");
    assert_eq!(value["execution"]["will_execute"], false);
    assert!(value.get("recovery_degradation").is_none());
    assert!(!home.path().join(".aegis").join("audit.jsonl").exists());
}
