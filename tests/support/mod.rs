//! Shared helpers for the `full_pipeline_*` integration test crates.
//!
//! These helpers were extracted verbatim from the original
//! `tests/full_pipeline.rs` so the split test files can share the same
//! environment setup, binary resolution, and audit-reading utilities without
//! duplicating logic. They are test-only and intentionally panic on internal
//! errors (`.unwrap()` is acceptable here).
//
// Each integration test file is compiled as its own crate and includes
// `mod support;`, so only a subset of these helpers is referenced by any
// given crate. Silence dead-code warnings for the unused subset rather than
// forcing every test file to touch every helper.
#![allow(dead_code)]

pub mod installer;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use aegis::audit::{AuditEntry, AuditLogger, AuditSnapshot, Decision};
use aegis::interceptor::RiskLevel;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub fn aegis_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aegis"))
}

pub fn base_command(home: &Path) -> Command {
    let mut command = Command::new(aegis_bin());
    command.env("AEGIS_REAL_SHELL", "/bin/sh");
    // These end-to-end tests exercise the normal interactive/non-interactive
    // product flow, not the CI fast-path. Force CI detection off so host
    // environments like GitHub Actions do not change the expected exit codes.
    command.env("AEGIS_CI", "0");
    command.env("HOME", home);
    command
}

pub fn direct_shell_command(home: &Path) -> Command {
    let mut command = Command::new("/bin/sh");
    command.env("HOME", home);
    command
}

pub fn read_audit_entries(home: &Path) -> Vec<serde_json::Value> {
    let path = home.join(".aegis").join("audit.jsonl");
    let contents = fs::read_to_string(path).unwrap();

    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect()
}

pub fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();

    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
}

pub fn write_disabled_toggle(home: &Path) {
    let aegis_dir = home.join(".aegis");
    fs::create_dir_all(&aegis_dir).unwrap();
    fs::write(aegis_dir.join("disabled"), "timestamp=x\npid=1\n").unwrap();
}

/// Read the invocation log written by a PATH-stub executable.
///
/// Returns the lines recorded by the stub, or an empty `Vec` when the log
/// file does not exist (meaning the stub was never called).
pub fn read_stub_invocations(log_path: &Path) -> Vec<String> {
    match fs::read_to_string(log_path) {
        Ok(contents) => contents
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_owned)
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub fn append_audit_entry(home: &Path, plugin: &str, snapshot_id: &str) {
    let logger = AuditLogger::new(home.join(".aegis").join("audit.jsonl"));
    logger
        .append(AuditEntry::new(
            "manual test command",
            RiskLevel::Safe,
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
