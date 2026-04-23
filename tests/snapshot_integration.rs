use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();

    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
}

fn init_git_repo(path: &Path) {
    Command::new("git")
        .arg("init")
        .current_dir(path)
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
        .current_dir(path)
        .output()
        .unwrap();
}

fn commit_file(path: &Path, name: &str, contents: &str) {
    fs::write(path.join(name), contents).unwrap();
    Command::new("git")
        .args(["add", name])
        .current_dir(path)
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
            &format!("add {name}"),
        ])
        .current_dir(path)
        .output()
        .unwrap();
}

fn add_worktree(repo: &Path, worktree: &Path) -> bool {
    Command::new("git")
        .args([
            "worktree",
            "add",
            worktree.to_str().unwrap(),
            "HEAD",
            "--detach",
        ])
        .current_dir(repo)
        .output()
        .unwrap()
        .status
        .success()
}

fn terraform_stub(bin_dir: &Path, log_path: &Path) -> String {
    fs::create_dir_all(bin_dir).unwrap();
    write_executable(
        &bin_dir.join("terraform"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AEGIS_TEST_TERRAFORM_LOG"
exit 0
"#,
    );
    let _ = log_path;
    format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

#[test]
fn git_snapshot_and_rollback_work_from_repo_subdirectory() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let subdir = workspace.path().join("deep/nested");
    let bin_dir = home.path().join("bin");
    let log_path = home.path().join("terraform.log");

    fs::create_dir_all(&subdir).unwrap();
    init_git_repo(workspace.path());
    commit_file(workspace.path(), "tracked.txt", "original\n");
    fs::write(workspace.path().join("tracked.txt"), "subdir change\n").unwrap();

    fs::write(
        subdir.join(".aegis.toml"),
        format!(
            r#"
allowlist_override_level = "Danger"
auto_snapshot_git = true
auto_snapshot_docker = false
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{}"
reason = "subdir snapshot test"
            "#,
            subdir.display()
        ),
    )
    .unwrap();

    let path = terraform_stub(&bin_dir, &log_path);

    let intercept_output = base_command(home.path())
        .current_dir(&subdir)
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    assert!(intercept_output.status.success());
    assert_eq!(
        fs::read_to_string(workspace.path().join("tracked.txt")).unwrap(),
        "original\n"
    );

    let entries = read_audit_entries(home.path());
    let snapshot_id = entries[0]["snapshots"][0]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        snapshot_id.starts_with(&format!("{}{}", subdir.display(), '\t')),
        "snapshot id should encode the subdir cwd: {snapshot_id}"
    );

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
        "subdir change\n"
    );
}

#[test]
fn git_snapshot_and_rollback_work_from_git_worktree() {
    let home = TempDir::new().unwrap();
    let main_repo = TempDir::new().unwrap();
    let worktree = TempDir::new().unwrap();
    let bin_dir = home.path().join("bin-worktree");
    let log_path = home.path().join("terraform-worktree.log");

    init_git_repo(main_repo.path());
    commit_file(main_repo.path(), "tracked.txt", "original\n");
    if !add_worktree(main_repo.path(), worktree.path()) {
        return;
    }

    fs::write(worktree.path().join("tracked.txt"), "worktree change\n").unwrap();
    fs::write(
        worktree.path().join(".aegis.toml"),
        format!(
            r#"
allowlist_override_level = "Danger"
auto_snapshot_git = true
auto_snapshot_docker = false
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{}"
reason = "worktree snapshot test"
            "#,
            worktree.path().display()
        ),
    )
    .unwrap();

    let path = terraform_stub(&bin_dir, &log_path);

    let intercept_output = base_command(home.path())
        .current_dir(worktree.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    assert!(intercept_output.status.success());
    assert_eq!(
        fs::read_to_string(worktree.path().join("tracked.txt")).unwrap(),
        "original\n"
    );

    let entries = read_audit_entries(home.path());
    let snapshot_id = entries[0]["snapshots"][0]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_string();

    let rollback_output = base_command(home.path())
        .current_dir(worktree.path())
        .args(["rollback", &snapshot_id])
        .output()
        .unwrap();

    assert!(
        rollback_output.status.success(),
        "rollback stderr:\n{}",
        String::from_utf8_lossy(&rollback_output.stderr)
    );
    assert_eq!(
        fs::read_to_string(worktree.path().join("tracked.txt")).unwrap(),
        "worktree change\n"
    );
}

#[test]
fn rollback_conflict_reports_manual_recovery_commands_and_preserves_stash() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let bin_dir = home.path().join("bin-conflict");
    let log_path = home.path().join("terraform-conflict.log");

    init_git_repo(workspace.path());
    commit_file(workspace.path(), "tracked.txt", "original\n");
    fs::write(workspace.path().join("tracked.txt"), "stashed change\n").unwrap();
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
allowlist_override_level = "Danger"
auto_snapshot_git = true
auto_snapshot_docker = false
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{}"
reason = "rollback conflict test"
            "#,
            workspace.path().display()
        ),
    )
    .unwrap();

    let path = terraform_stub(&bin_dir, &log_path);

    let intercept_output = base_command(home.path())
        .current_dir(workspace.path())
        .env("PATH", &path)
        .env("AEGIS_TEST_TERRAFORM_LOG", &log_path)
        .args(["-c", "terraform destroy -target=module.test.api"])
        .output()
        .unwrap();

    assert!(intercept_output.status.success());
    fs::write(workspace.path().join("tracked.txt"), "conflicting change\n").unwrap();

    let entries = read_audit_entries(home.path());
    let snapshot_id = entries[0]["snapshots"][0]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_string();

    let rollback_output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["rollback", &snapshot_id])
        .output()
        .unwrap();

    assert_eq!(rollback_output.status.code(), Some(4));
    let stderr = String::from_utf8_lossy(&rollback_output.stderr);
    assert!(stderr.contains("rollback conflict"));
    assert!(stderr.contains("git diff"));
    assert!(stderr.contains("git stash drop"));

    let stash_list = Command::new("git")
        .args(["stash", "list"])
        .current_dir(workspace.path())
        .output()
        .unwrap();
    assert!(stash_list.status.success());
    assert!(
        !String::from_utf8_lossy(&stash_list.stdout)
            .trim()
            .is_empty(),
        "conflicted rollback should preserve stash entry for manual recovery"
    );
}
