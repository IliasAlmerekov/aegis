//! Integration tests for `aegis watch` — end-to-end via child process.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::TempDir;

fn aegis_watch(input: &[u8]) -> std::process::Output {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    aegis_watch_in(home.path(), cwd.path(), input)
}

fn aegis_watch_in(home: &Path, cwd: &Path, input: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aegis"))
        .arg("watch")
        .env("AEGIS_REAL_SHELL", "/bin/sh")
        .env("AEGIS_CI", "0")
        .env("HOME", home)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aegis watch");

    if let Err(err) = child.stdin.as_mut().unwrap().write_all(input)
        && err.kind() != ErrorKind::BrokenPipe
    {
        panic!("failed to write to aegis watch stdin: {err}");
    }
    drop(child.stdin.take()); // close stdin to send EOF

    child
        .wait_with_output()
        .expect("failed to wait for aegis watch")
}

fn write_disabled_toggle(home: &Path) {
    let aegis_dir = home.join(".aegis");
    fs::create_dir_all(&aegis_dir).unwrap();
    fs::write(aegis_dir.join("disabled"), "timestamp=x\npid=1\n").unwrap();
}

fn parse_frames(stdout: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("invalid NDJSON frame"))
        .collect()
}

#[test]
fn safe_command_emits_result_approved() {
    let output = aegis_watch(b"{\"cmd\":\"echo hello\",\"id\":\"1\"}\n");
    assert!(output.status.success(), "watch must exit 0 on clean EOF");

    let frames = parse_frames(&output.stdout);
    let result = frames
        .iter()
        .find(|f| f["type"] == "result")
        .expect("no result frame");

    assert_eq!(result["decision"], "approved");
    assert_eq!(result["exit_code"], 0);
    assert_eq!(result["id"], "1");
}

#[test]
fn safe_command_stdout_chunk_is_base64() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let output = aegis_watch(b"{\"cmd\":\"printf 'hello'\"}\n");
    let frames = parse_frames(&output.stdout);

    let stdout_frame = frames
        .iter()
        .find(|f| f["type"] == "stdout")
        .expect("no stdout frame");
    let data_b64 = stdout_frame["data_b64"]
        .as_str()
        .expect("data_b64 must be a string");
    let decoded = BASE64
        .decode(data_b64)
        .expect("data_b64 must be valid base64");
    assert_eq!(decoded, b"hello");
}

#[test]
fn invalid_json_emits_error_frame_and_continues() {
    let input = b"not-json\n{\"cmd\":\"echo ok\"}\n";
    let output = aegis_watch(input);
    assert!(output.status.success());

    let frames = parse_frames(&output.stdout);
    let error = frames
        .iter()
        .find(|f| f["type"] == "error")
        .expect("no error frame");
    assert_eq!(error["exit_code"], 4);
    assert!(error["message"].as_str().unwrap().contains("invalid JSON"));

    let results: Vec<_> = frames.iter().filter(|f| f["type"] == "result").collect();
    assert_eq!(
        results.len(),
        1,
        "second command must produce a result frame"
    );
    assert_eq!(results[0]["decision"], "approved");
}

#[test]
fn empty_cmd_emits_error_frame() {
    // Exercises the explicit `cmd.trim().is_empty()` guard in process_frame.
    let output = aegis_watch(b"{\"cmd\":\"\"}\n");
    let frames = parse_frames(&output.stdout);
    let error = frames
        .iter()
        .find(|f| f["type"] == "error")
        .expect("no error frame");
    assert_eq!(error["exit_code"], 4);
    assert!(error["message"].as_str().unwrap().contains("cmd"));
}

#[test]
fn missing_cmd_field_emits_error_frame() {
    // JSON parse failure: `cmd` is required (no #[serde(default)]).
    let output = aegis_watch(b"{\"source\":\"test\"}\n");
    let frames = parse_frames(&output.stdout);
    let error = frames
        .iter()
        .find(|f| f["type"] == "error")
        .expect("no error frame");
    assert_eq!(error["exit_code"], 4);
    assert!(error["message"].as_str().unwrap().contains("invalid JSON"));
}

#[test]
fn watch_mode_audit_entry_sets_transport_watch() {
    use std::fs;
    use std::io::Write;
    use std::process::{Command, Stdio};

    let dir = tempfile::TempDir::new().unwrap();
    let audit_path = dir.path().join("audit.jsonl");

    let mut child = Command::new(env!("CARGO_BIN_EXE_aegis"))
        .arg("watch")
        .env("AEGIS_REAL_SHELL", "/bin/sh")
        .env("AEGIS_AUDIT_PATH", &audit_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"{\"cmd\":\"echo audit\",\"source\":\"test-agent\",\"id\":\"a1\"}\n")
        .unwrap();
    drop(child.stdin.take());
    let _ = child.wait_with_output().unwrap();

    assert!(
        audit_path.exists(),
        "watch mode should honor AEGIS_AUDIT_PATH and create the audit log there"
    );

    let contents = fs::read_to_string(&audit_path).unwrap();
    let entry: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
    assert_eq!(entry["transport"], "watch");
    assert_eq!(entry["source"], "test-agent");
    assert_eq!(entry["id"], "a1");
}

#[test]
fn invalid_cwd_emits_error_frame() {
    let output = aegis_watch(b"{\"cmd\":\"echo x\",\"cwd\":\"/nonexistent/path/xyz\"}\n");
    let frames = parse_frames(&output.stdout);
    let error = frames
        .iter()
        .find(|f| f["type"] == "error")
        .expect("no error frame");
    assert_eq!(error["exit_code"], 4);
    assert_eq!(error["message"], "invalid cwd");
}

#[test]
fn watch_invalid_cwd_keeps_current_error_frame_contract_after_planner_migration() {
    let output =
        aegis_watch(b"{\"cmd\":\"echo x\",\"cwd\":\"/nonexistent/path/xyz\",\"id\":\"bad-cwd\"}\n");

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "invalid cwd remains a per-frame error, not a startup stderr failure"
    );

    let frames = parse_frames(&output.stdout);
    assert_eq!(
        frames.len(),
        1,
        "invalid cwd should emit exactly one error frame"
    );
    let error = &frames[0];
    assert_eq!(error["type"], "error");
    assert_eq!(error["id"], "bad-cwd");
    assert_eq!(error["exit_code"], 4);
    assert_eq!(error["message"], "invalid cwd");
}

#[test]
fn oversized_frame_emits_error_frame_and_continues() {
    let big_cmd = "x".repeat(1_100_000);
    let big_frame = format!("{{\"cmd\":\"{big_cmd}\"}}\n");
    let small_frame = b"{\"cmd\":\"echo after\"}\n";

    let mut input = big_frame.into_bytes();
    input.extend_from_slice(small_frame);

    let output = aegis_watch(&input);
    assert!(output.status.success());

    let frames = parse_frames(&output.stdout);
    let error = frames
        .iter()
        .find(|f| f["type"] == "error")
        .expect("no error frame");
    assert!(error["message"].as_str().unwrap().contains("1 MiB"));

    let results: Vec<_> = frames.iter().filter(|f| f["type"] == "result").collect();
    assert_eq!(
        results.len(),
        1,
        "command after oversized frame must execute"
    );
}

#[test]
fn id_field_is_echoed_on_all_frames() {
    let output = aegis_watch(b"{\"cmd\":\"printf 'hi'\",\"id\":\"corr-99\"}\n");
    let frames = parse_frames(&output.stdout);

    for frame in &frames {
        if frame["type"] != "error" {
            assert_eq!(
                frame["id"], "corr-99",
                "id must be echoed on all non-error frames: {frame}"
            );
        }
    }
}

#[test]
fn child_exit_code_is_propagated() {
    let output = aegis_watch(b"{\"cmd\":\"exit 42\",\"id\":\"ec\"}\n");
    let frames = parse_frames(&output.stdout);
    let result = frames.iter().find(|f| f["type"] == "result").unwrap();
    assert_eq!(result["exit_code"], 42);
}

#[test]
fn watch_exits_zero_on_clean_eof() {
    let output = aegis_watch(b"{\"cmd\":\"echo hi\"}\n");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn disabled_watch_mode_passthrough_executes_command_without_audit() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    write_disabled_toggle(home.path());

    let output = aegis_watch_in(
        home.path(),
        cwd.path(),
        b"{\"cmd\":\"printf hi\",\"id\":\"disabled-watch\"}\n",
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());

    let frames = parse_frames(&output.stdout);
    let stdout_frame = frames
        .iter()
        .find(|f| f["type"] == "stdout")
        .expect("disabled watch mode should still emit stdout frames");
    assert!(
        stdout_frame["data_b64"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "stdout frame should carry child output"
    );
    let result = frames
        .iter()
        .find(|f| f["type"] == "result")
        .expect("disabled watch mode should still emit a result frame");
    assert_eq!(result["decision"], "approved");
    assert_eq!(result["exit_code"], 0);

    assert!(
        !home.path().join(".aegis").join("audit.jsonl").exists(),
        "disabled watch passthrough must bypass auditing"
    );
}

#[test]
fn malformed_project_config_aborts_watch_startup_with_clear_error() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let config_path = workspace.path().join(".aegis.toml");
    fs::write(&config_path, "mode = <<<THIS IS NOT VALID TOML\n").unwrap();

    let output = aegis_watch_in(home.path(), workspace.path(), b"{\"cmd\":\"echo hi\"}\n");

    assert_eq!(output.status.code(), Some(4));
    assert!(
        output.stdout.is_empty(),
        "watch must not emit frames on startup failure"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error: failed to load config"),
        "stderr must explain the startup failure: {stderr}"
    );
    assert!(
        stderr.contains(&config_path.display().to_string()),
        "stderr must identify the invalid config file: {stderr}"
    );
    assert!(
        stderr.contains("failed to parse"),
        "stderr must include the parse/validation detail: {stderr}"
    );
    assert!(
        stderr.contains("Fix or remove the invalid config file"),
        "stderr must tell the user how to recover: {stderr}"
    );
}

#[test]
fn invalid_project_config_in_watch_still_fails_before_emitting_frames() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let config_path = workspace.path().join(".aegis.toml");
    fs::write(&config_path, "mode = <<<THIS IS NOT VALID TOML\n").unwrap();

    let output = aegis_watch_in(
        home.path(),
        workspace.path(),
        b"{\"cmd\":\"echo hi\",\"id\":\"watch-config\"}\n",
    );

    assert_eq!(output.status.code(), Some(4));
    assert!(
        output.stdout.is_empty(),
        "watch must not emit frames on startup failure"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: failed to load config"));
    assert!(stderr.contains(&config_path.display().to_string()));
    assert!(stderr.contains("failed to parse"));
    assert!(stderr.contains("Fix or remove the invalid config file"));
}
