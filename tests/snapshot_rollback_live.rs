use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn aegis_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aegis"))
}

fn sqlite_live_tests_enabled() -> bool {
    std::env::var("AEGIS_SQLITE_SNAPSHOT_TESTS").is_ok()
}

fn sqlite3_available() -> bool {
    Command::new("sqlite3")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

macro_rules! require_sqlite3 {
    () => {
        if !sqlite_live_tests_enabled() || !sqlite3_available() {
            eprintln!(
                "skipping: set AEGIS_SQLITE_SNAPSHOT_TESTS=1 and install sqlite3 to run this test"
            );
            return;
        }
    };
}

fn base_command(home: &Path) -> Command {
    let mut command = Command::new(aegis_bin());
    command.env("AEGIS_REAL_SHELL", "/bin/sh");
    command.env("AEGIS_CI", "0");
    command.env("HOME", home);
    command
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();

    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
}

fn run_sqlite3(db_path: &Path, sql: &str) {
    let output = Command::new("sqlite3")
        .arg(db_path)
        .arg(sql)
        .output()
        .unwrap_or_else(|error| panic!("sqlite3 should run: {error}"));

    assert!(
        output.status.success(),
        "sqlite3 failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn query_scalar(db_path: &Path, sql: &str) -> String {
    let output = Command::new("sqlite3")
        .arg("-batch")
        .arg("-noheader")
        .arg(db_path)
        .arg(sql)
        .output()
        .unwrap_or_else(|error| panic!("sqlite3 query should run: {error}"));

    assert!(
        output.status.success(),
        "sqlite3 query failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn read_audit_entries(home: &Path) -> Vec<serde_json::Value> {
    let path = home.join(".aegis").join("audit.jsonl");
    let contents = fs::read_to_string(path).unwrap();

    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect()
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

#[test]
fn sqlite_snapshot_rollback_restores_database_file_through_aegis_cli() {
    require_sqlite3!();

    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = home.path().join("bin");
    let db_path = workspace.path().join("app.db");
    let terraform_log = home.path().join("terraform.log");

    fs::create_dir_all(&bin_dir).unwrap();
    run_sqlite3(
        &db_path,
        "CREATE TABLE items(name TEXT); INSERT INTO items(name) VALUES ('before');",
    );

    write_executable(
        &bin_dir.join("terraform"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_TERRAFORM_LOG"
sqlite3 "$AEGIS_TEST_SQLITE_DB" "INSERT INTO items(name) VALUES ('after');"
"#,
    );

    let workspace_cwd = workspace.path().canonicalize().unwrap();
    write_global_config(home.path(), "allowlist_override_level = \"Danger\"\n");
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
auto_snapshot_git = false
auto_snapshot_docker = false
auto_snapshot_postgres = false
auto_snapshot_mysql = false
auto_snapshot_supabase = false
auto_snapshot_sqlite = true
sqlite_snapshot_path = "app.db"

[[allow]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{}"
reason = "M5.3 live SQLite snapshot rollback test"
"#,
            workspace_cwd.display()
        ),
    )
    .unwrap();

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let intercept_output = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &terraform_log)
        .env("AEGIS_TEST_SQLITE_DB", &db_path)
        .args(["-c", "terraform destroy -target=module.test.sqlite"])
        .output()
        .unwrap();

    assert!(
        intercept_output.status.success(),
        "Aegis command should auto-approve and run\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&intercept_output.stdout),
        String::from_utf8_lossy(&intercept_output.stderr)
    );
    assert_eq!(
        query_scalar(
            &db_path,
            "SELECT group_concat(name, ',') FROM items ORDER BY rowid;"
        ),
        "before,after",
        "child command should mutate the real SQLite database after the snapshot"
    );

    let entries = read_audit_entries(home.path());
    let snapshot_id = entries
        .iter()
        .flat_map(|entry| entry["snapshots"].as_array().into_iter().flatten())
        .find(|snapshot| snapshot["plugin"] == "sqlite")
        .and_then(|snapshot| snapshot["snapshot_id"].as_str())
        .expect("audit log should contain a sqlite snapshot id")
        .to_string();

    let rollback_output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["rollback", &snapshot_id])
        .output()
        .unwrap();

    assert!(
        rollback_output.status.success(),
        "Aegis rollback should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rollback_output.stdout),
        String::from_utf8_lossy(&rollback_output.stderr)
    );
    assert_eq!(
        query_scalar(
            &db_path,
            "SELECT group_concat(name, ',') FROM items ORDER BY rowid;"
        ),
        "before",
        "rollback must restore the pre-command SQLite database snapshot"
    );

    let entries = read_audit_entries(home.path());
    assert!(
        entries.iter().any(|entry| {
            entry["command"].as_str() == Some(format!("aegis rollback {snapshot_id}").as_str())
                && entry["decision"].as_str() == Some("Approved")
                && entry["risk"].as_str() == Some("Safe")
        }),
        "rollback must be recorded in the append-only audit log"
    );
}
