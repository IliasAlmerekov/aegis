use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn read_audit_entries(home: &Path) -> Vec<Value> {
    let path = home.join(".aegis").join("audit.jsonl");
    let contents = fs::read_to_string(path).unwrap();

    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect()
}

#[test]
fn integrity_mode_chains_hashes_and_verify_succeeds_across_rotation() {
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
integrity_mode = "ChainSha256"
"#,
    )
    .unwrap();

    for command in ["printf one", "printf two", "printf three"] {
        let output = base_command(home.path())
            .current_dir(workspace.path())
            .args(["-c", command])
            .output()
            .unwrap();
        assert!(output.status.success(), "{command} failed");
    }

    let entries = read_audit_entries(home.path());
    assert_eq!(
        entries.len(),
        1,
        "rotation should leave only one active entry"
    );
    assert_eq!(entries[0]["chain_alg"], "sha256");
    assert!(entries[0].get("entry_hash").is_some());

    let verify = base_command(home.path())
        .current_dir(workspace.path())
        .args(["audit", "--verify-integrity"])
        .output()
        .unwrap();

    assert!(
        verify.status.success(),
        "verify failed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&verify.stdout),
        String::from_utf8_lossy(&verify.stderr)
    );
}

#[test]
fn verify_integrity_detects_tampered_active_log() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
[audit]
integrity_mode = "ChainSha256"
"#,
    )
    .unwrap();

    for command in ["printf one", "printf two"] {
        let output = base_command(home.path())
            .current_dir(workspace.path())
            .args(["-c", command])
            .output()
            .unwrap();
        assert!(output.status.success());
    }

    let audit_path = home.path().join(".aegis").join("audit.jsonl");
    let tampered = fs::read_to_string(&audit_path)
        .unwrap()
        .replace("printf two", "printf TWo");
    fs::write(&audit_path, tampered).unwrap();

    let verify = base_command(home.path())
        .current_dir(workspace.path())
        .args(["audit", "--verify-integrity"])
        .output()
        .unwrap();

    assert!(
        !verify.status.success(),
        "tampered log must fail verification"
    );
    let stderr = String::from_utf8_lossy(&verify.stderr);
    let stdout = String::from_utf8_lossy(&verify.stdout);
    assert!(
        stderr.contains("integrity") || stdout.contains("integrity"),
        "verify output must explain integrity failure"
    );
}

#[test]
fn verify_integrity_detects_tampered_archive_log() {
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
integrity_mode = "ChainSha256"
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

    let archive_path = home.path().join(".aegis").join("audit.jsonl.1");
    let tampered = fs::read_to_string(&archive_path)
        .unwrap()
        .replace("printf two", "printf TWo");
    fs::write(&archive_path, tampered).unwrap();

    let verify = base_command(home.path())
        .current_dir(workspace.path())
        .args(["audit", "--verify-integrity"])
        .output()
        .unwrap();

    assert!(
        !verify.status.success(),
        "tampered archive must fail verification"
    );
}

#[test]
fn verify_integrity_rejects_legacy_log_without_chain_data() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "printf one"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let verify = base_command(home.path())
        .args(["audit", "--verify-integrity"])
        .output()
        .unwrap();

    assert!(
        !verify.status.success(),
        "legacy log without chain data must not report a false PASS"
    );
    let stderr = String::from_utf8_lossy(&verify.stderr);
    let stdout = String::from_utf8_lossy(&verify.stdout);
    assert!(
        stderr.contains("no integrity") || stdout.contains("no integrity"),
        "verify output must explain that integrity mode was not enabled"
    );
}
