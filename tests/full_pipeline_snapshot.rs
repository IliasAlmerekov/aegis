//! Snapshot behavior: registry config-flag regressions, supabase snapshot
//! suppression on denial, strict-mode snapshot creation, and the `rollback`
//! subcommand (restore, missing/malformed ids, malformed config fail-closed).
//!
//! Split from the original `tests/full_pipeline.rs` (behavior-preserving move).

mod support;

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::TempDir;

use support::*;

// ─────────────────────────────────────────────────────────────────────────────
// Snapshot registry config-flag regressions (Ticket 1.3)
// ─────────────────────────────────────────────────────────────────────────────

/// With `auto_snapshot_git = false` in `.aegis.toml`, the real aegis binary
/// must never invoke the `git` stub in PATH, and the audit entry must record
/// an empty `snapshots` array.
///
/// This proves the Git plugin was never *registered*, not merely skipped by
/// `is_applicable` — the stub would catch any `git` invocation regardless of
/// which code path triggered it.
#[test]
fn snapshot_registry_git_flag_false_skips_plugin_and_audit() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");
    let git_log = workspace.path().join("git_stub.log");

    fs::create_dir_all(&bin_dir).unwrap();

    // Stub git: append arguments to the log file so we can assert it was never
    // called.  Use AEGIS_TEST_GIT_LOG as the log path so the test controls it.
    write_executable(
        &bin_dir.join("git"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_GIT_LOG"
exit 0
"#,
    );

    // Both snapshot flags off: git-off is under test, docker-off isolates the
    // assertion so a stray docker invocation cannot add noise. `auto_snapshot_git`
    // defaults to `true` and a project can no longer disable it (C3 security
    // ratchet), so the trusted global config opts out of git snapshots; the
    // project's matching `false` is then a no-op rather than a weakening.
    write_global_config(home.path(), "auto_snapshot_git = false\n");
    fs::write(
        workspace.path().join(".aegis.toml"),
        "auto_snapshot_git = false\nauto_snapshot_docker = false\n",
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    // Use a unique sentinel file in the temp workspace so the rm -rf target
    // is fully controlled and isolated from the developer's file system.
    let sentinel = workspace.path().join("sentinel_git_off.txt");
    fs::write(&sentinel, "git-off test sentinel").unwrap();

    let mut child = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_GIT_LOG", &git_log)
        .env("AEGIS_FORCE_INTERACTIVE", "1")
        .args(["-c", &format!("rm -rf {}", sentinel.display())])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.as_mut().unwrap().write_all(b"no\n").unwrap();
    let output = child.wait_with_output().unwrap();

    // Danger command denied → exit code 2.
    assert_eq!(
        output.status.code(),
        Some(2),
        "Danger command must be denied (exit 2)"
    );

    // Git stub must never have been called.
    let git_calls = read_stub_invocations(&git_log);
    assert!(
        git_calls.is_empty(),
        "git stub must not be invoked when auto_snapshot_git = false; calls: {git_calls:?}"
    );

    // Exactly one audit entry, decision Denied, risk Danger, snapshots empty.
    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1, "expected exactly one audit entry");
    assert_eq!(entries[0]["decision"], "Denied");
    assert_eq!(entries[0]["risk"], "Danger");
    assert_eq!(entries[0]["snapshots"], serde_json::json!([]));
}

/// With `auto_snapshot_docker = false` in `.aegis.toml`, the real aegis binary
/// must never invoke the `docker` stub in PATH, and the audit entry must record
/// an empty `snapshots` array.
///
/// Both snapshot flags are disabled to keep the assertions fully isolated —
/// disabling git prevents unrelated git activity from adding noise when the
/// workspace happens to be near a git checkout.
#[test]
fn snapshot_registry_docker_flag_false_skips_plugin_and_audit() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");
    let docker_log = workspace.path().join("docker_stub.log");

    fs::create_dir_all(&bin_dir).unwrap();

    // Stub docker: append arguments to the log file so we can assert it was
    // never called.  Use AEGIS_TEST_DOCKER_LOG as the log path.
    write_executable(
        &bin_dir.join("docker"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_DOCKER_LOG"
exit 0
"#,
    );

    // Docker-off is under test; git-off isolates assertions from git noise.
    fs::write(
        workspace.path().join(".aegis.toml"),
        "auto_snapshot_git = false\nauto_snapshot_docker = false\n",
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let sentinel = workspace.path().join("sentinel_docker_off.txt");
    fs::write(&sentinel, "docker-off test sentinel").unwrap();

    let mut child = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_DOCKER_LOG", &docker_log)
        .env("AEGIS_FORCE_INTERACTIVE", "1")
        .args(["-c", &format!("rm -rf {}", sentinel.display())])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.as_mut().unwrap().write_all(b"no\n").unwrap();
    let output = child.wait_with_output().unwrap();

    // Danger command denied → exit code 2.
    assert_eq!(
        output.status.code(),
        Some(2),
        "Danger command must be denied (exit 2)"
    );

    // Docker stub must never have been called.
    let docker_calls = read_stub_invocations(&docker_log);
    assert!(
        docker_calls.is_empty(),
        "docker stub must not be invoked when auto_snapshot_docker = false; calls: {docker_calls:?}"
    );

    // Exactly one audit entry, decision Denied, risk Danger, snapshots empty.
    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1, "expected exactly one audit entry");
    assert_eq!(entries[0]["decision"], "Denied");
    assert_eq!(entries[0]["risk"], "Danger");
    assert_eq!(entries[0]["snapshots"], serde_json::json!([]));
}

#[test]
fn danger_command_denied_records_no_supabase_snapshot_in_audit() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = workspace.path().join("bin");
    let sentinel = workspace.path().join("sentinel_supabase.txt");

    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(&sentinel, "keep me\n").unwrap();

    write_executable(
        &bin_dir.join("pg_dump"),
        r#"#!/bin/sh
dump_path=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-f" ]; then
    dump_path="$2"
    shift 2
    continue
  fi
  shift
done

if [ -z "$dump_path" ]; then
  echo "missing -f dump path" >&2
  exit 1
fi

printf 'supabase phase1 dump\n' > "$dump_path"
"#,
    );
    write_executable(&bin_dir.join("pg_restore"), "#!/bin/sh\nexit 0\n");

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
auto_snapshot_git = false
auto_snapshot_docker = false
auto_snapshot_postgres = false
auto_snapshot_mysql = false
auto_snapshot_sqlite = false
auto_snapshot_supabase = true

[supabase_snapshot]
project_ref = "proj_e2e"
require_config_target_match_on_rollback = true

[supabase_snapshot.db]
database = "postgres"
host = "db.supabase.co"
port = 5432
user = "postgres"
"#,
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let mut child = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_FORCE_INTERACTIVE", "1")
        .args(["-c", &format!("rm -rf {}", sentinel.display())])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.as_mut().unwrap().write_all(b"no\n").unwrap();
    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(sentinel.exists(), "denied danger command must not run");

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 1);
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

/// Strict mode with allowlist_override_level = Danger and an allowlisted
/// Danger command must auto-approve and create a git snapshot.
#[test]
fn strict_override_allowlisted_danger_executes_and_creates_snapshot() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    // bin_dir and log_path must be outside the workspace so that git stash
    // (--include-untracked) does not sweep them into the stash and make
    // terraform un-findable after the snapshot is created.
    let bin_dir = home.path().join("bin");
    let log_path = home.path().join("terraform.log");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("terraform"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_TERRAFORM_LOG"
exit 0
"#,
    );
    let workspace_cwd = workspace
        .path()
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    write_global_config(home.path(), "allowlist_override_level = \"Danger\"\n");
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
mode = "Strict"
auto_snapshot_git = true
auto_snapshot_docker = false
[[allow]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{workspace_cwd}"
reason = "strict override allowlist"
"#
        ),
    )
    .unwrap();

    Command::new("git")
        .arg("init")
        .current_dir(workspace.path())
        .output()
        .unwrap();
    Command::new("git")
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
        .current_dir(workspace.path())
        .output()
        .unwrap();
    fs::write(workspace.path().join("dirty.txt"), "needs snapshot\n").unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "strict+allowlisted danger must auto-approve; status: {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        fs::read_to_string(&log_path).unwrap(),
        "destroy -target=module.test.api\n"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries[0]["decision"], "AutoApproved");
    assert_eq!(entries[0]["risk"], "Danger");
    assert_ne!(entries[0]["snapshots"], serde_json::json!([]));
}

#[test]
fn rollback_restores_git_snapshot_from_audit_and_logs_action() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = home.path().join("bin");
    let log_path = home.path().join("terraform.log");

    fs::create_dir_all(&bin_dir).unwrap();
    write_executable(
        &bin_dir.join("terraform"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_TERRAFORM_LOG"
exit 0
"#,
    );
    let workspace_cwd = workspace
        .path()
        .canonicalize()
        .unwrap()
        .display()
        .to_string();
    write_global_config(home.path(), "allowlist_override_level = \"Danger\"\n");
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
auto_snapshot_git = true
auto_snapshot_docker = false
[[allow]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{workspace_cwd}"
reason = "rollback test allowlist"
"#
        ),
    )
    .unwrap();

    Command::new("git")
        .arg("init")
        .current_dir(workspace.path())
        .output()
        .unwrap();
    Command::new("git")
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
        .current_dir(workspace.path())
        .output()
        .unwrap();
    fs::write(workspace.path().join("tracked.txt"), "original\n").unwrap();
    Command::new("git")
        .args(["add", "tracked.txt"])
        .current_dir(workspace.path())
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "-c",
            "user.email=test@aegis.dev",
            "-c",
            "user.name=Aegis Test",
            "commit",
            "-m",
            "add tracked file",
        ])
        .current_dir(workspace.path())
        .output()
        .unwrap();

    fs::write(workspace.path().join("tracked.txt"), "needs rollback\n").unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let intercept_output = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    assert!(
        intercept_output.status.success(),
        "intercept must succeed before rollback; status: {:?}\nstderr:\n{}",
        intercept_output.status.code(),
        String::from_utf8_lossy(&intercept_output.stderr),
    );
    assert_eq!(
        fs::read_to_string(workspace.path().join("tracked.txt")).unwrap(),
        "original\n"
    );

    let entries = read_audit_entries(home.path());
    let snapshot_id = entries[0]["snapshots"][0]["snapshot_id"]
        .as_str()
        .expect("snapshot_id must be a string")
        .to_string();

    let rollback_output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["rollback", &snapshot_id])
        .output()
        .unwrap();

    assert!(
        rollback_output.status.success(),
        "rollback stderr:\n{}",
        String::from_utf8_lossy(&rollback_output.stderr)
    );
    assert_eq!(
        fs::read_to_string(workspace.path().join("tracked.txt")).unwrap(),
        "needs rollback\n"
    );

    let entries = read_audit_entries(home.path());
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[1]["command"],
        format!("aegis rollback {snapshot_id}")
    );
    assert_eq!(entries[1]["decision"], "Approved");
    assert_eq!(entries[1]["risk"], "Safe");
    assert_eq!(entries[1]["snapshots"][0]["plugin"], "git");
    assert_eq!(
        entries[1]["snapshots"][0]["snapshot_id"].as_str(),
        Some(snapshot_id.as_str())
    );
}

#[test]
fn rollback_missing_snapshot_prints_recovery_hint() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["rollback", "missing-snapshot"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing-snapshot"));
    assert!(stderr.contains("aegis audit"));
    assert!(stderr.contains("snapshot"));
}

#[test]
fn rollback_with_malformed_project_config_fails_closed_instead_of_falling_back() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let config_path = workspace.path().join(".aegis.toml");

    fs::write(&config_path, "mode = <<<THIS IS NOT VALID TOML\n").unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["rollback", "missing-snapshot"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&config_path.display().to_string()),
        "rollback must report the malformed config path: {stderr}"
    );
    assert!(
        stderr.contains("failed to parse"),
        "rollback must surface config parsing errors instead of silently falling back: {stderr}"
    );
    assert!(
        !stderr.contains("snapshot id"),
        "rollback must fail on config load before attempting snapshot lookup: {stderr}"
    );
}

#[test]
fn rollback_with_malformed_project_config_uses_standard_config_load_error_format() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let config_path = workspace.path().join(".aegis.toml");

    fs::write(&config_path, "mode = <<<THIS IS NOT VALID TOML\n").unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["rollback", "missing-snapshot"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error: failed to load config:"),
        "rollback config failures should use the standard config-load prefix: {stderr}"
    );
    assert!(
        stderr.contains(&config_path.display().to_string()),
        "rollback config failures should identify the invalid config path: {stderr}"
    );
    assert!(
        stderr.contains("Fix or remove the invalid config file and try again."),
        "rollback config failures should print the standard recovery hint: {stderr}"
    );
    assert!(
        !stderr.contains("error: rollback failed:"),
        "rollback config failures should not use the generic rollback-failed wrapper: {stderr}"
    );
}

#[test]
fn rollback_with_known_provider_but_malformed_id_fails_closed() {
    let home = TempDir::new().unwrap();
    let snapshot_id = "malformed-id-format";

    append_audit_entry(home.path(), "git", snapshot_id);

    let output = base_command(home.path())
        .args(["rollback", snapshot_id])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("malformed snapshot_id"));
    assert!(stderr.contains("error: rollback failed:"));
}

#[test]
fn rollback_with_unknown_plugin_is_rejected_without_fallback() {
    let home = TempDir::new().unwrap();
    let snapshot_id = "not-known\tabcdef";

    append_audit_entry(home.path(), "legacy-provider", snapshot_id);

    let output = base_command(home.path())
        .args(["rollback", snapshot_id])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("snapshot plugin \"legacy-provider\" is not available for rollback"));
    assert!(!stderr.contains("snapshot id was not found"));
}
