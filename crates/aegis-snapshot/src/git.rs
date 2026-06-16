//! Git snapshot provider — creates stashes before dangerous commands.

use std::path::Path;

use async_trait::async_trait;
use tokio::process::Command;

use crate::SnapshotPlugin;
use crate::error::SnapshotError;

type Result<T> = std::result::Result<T, SnapshotError>;

/// Sentinel value stored when the working tree had no changes to stash.
/// A real stash hash is always a 40-char hex string, so this cannot collide.
const CLEAN_SENTINEL: &str = "clean";

/// Separator between the encoded `cwd` and the stash hash in a `snapshot_id`.
/// Tab is not a valid path component on Unix or Windows, so it is safe to use
/// as an unambiguous delimiter.
const SEP: char = '\t';

/// Built-in Git snapshot provider (creates stashes before dangerous commands).
pub struct GitPlugin;

#[async_trait]
impl SnapshotPlugin for GitPlugin {
    fn name(&self) -> &'static str {
        "git"
    }

    async fn is_applicable(&self, cwd: &Path) -> bool {
        Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(cwd)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    async fn snapshot(&self, cwd: &Path, _cmd: &str) -> Result<String> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let message = format!("aegis-snap-{timestamp}");

        let status_out = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(cwd)
            .output()
            .await
            .map_err(|e| SnapshotError::Snapshot(format!("failed to run git status: {e}")))?;

        if !status_out.status.success() {
            return Err(SnapshotError::Snapshot(
                "git status --porcelain failed".to_string(),
            ));
        }

        if status_out.stdout.iter().all(|b| b.is_ascii_whitespace()) {
            tracing::info!("git working tree is clean, nothing to stash");
            return Ok(CLEAN_SENTINEL.to_string());
        }

        let stash_out = Command::new("git")
            .args(["stash", "push", "--include-untracked", "-m", &message])
            .current_dir(cwd)
            .output()
            .await
            .map_err(|e| SnapshotError::Snapshot(format!("failed to run git stash: {e}")))?;

        if !stash_out.status.success() {
            let stderr = String::from_utf8_lossy(&stash_out.stderr);
            return Err(SnapshotError::Snapshot(format!(
                "git stash push failed: {stderr}"
            )));
        }

        let rev_out = Command::new("git")
            .args(["rev-parse", "stash@{0}"])
            .current_dir(cwd)
            .output()
            .await
            .map_err(|e| SnapshotError::Snapshot(format!("git rev-parse failed: {e}")))?;

        if !rev_out.status.success() {
            return Err(SnapshotError::Snapshot(
                "could not resolve stash ref after push".to_string(),
            ));
        }

        let hash = String::from_utf8_lossy(&rev_out.stdout).trim().to_string();

        let snapshot_id = format!("{}{SEP}{hash}", cwd.display());
        tracing::info!(%snapshot_id, "git snapshot created");
        Ok(snapshot_id)
    }

    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        if snapshot_id == CLEAN_SENTINEL {
            tracing::info!("git snapshot was clean, nothing to roll back");
            return Ok(());
        }

        let (cwd_str, hash) = snapshot_id.split_once(SEP).ok_or_else(|| {
            SnapshotError::Snapshot(format!("malformed snapshot_id: {snapshot_id:?}"))
        })?;

        let list_out = Command::new("git")
            .args(["stash", "list", "--format=%H %gd"])
            .current_dir(cwd_str)
            .output()
            .await
            .map_err(|e| SnapshotError::Snapshot(format!("git stash list failed: {e}")))?;

        if !list_out.status.success() {
            return Err(SnapshotError::Snapshot("git stash list failed".to_string()));
        }

        let list_stdout = String::from_utf8_lossy(&list_out.stdout);
        let stash_ref = list_stdout
            .lines()
            .find_map(|line| {
                let (h, r) = line.split_once(' ')?;
                (h == hash).then(|| r.to_string())
            })
            .ok_or_else(|| {
                SnapshotError::Snapshot(format!("stash entry not found for hash {hash}"))
            })?;

        let apply_out = Command::new("git")
            .args(["stash", "apply", "--index", hash])
            .current_dir(cwd_str)
            .output()
            .await
            .map_err(|e| SnapshotError::Snapshot(format!("git stash apply failed: {e}")))?;

        if !apply_out.status.success() {
            let stderr = String::from_utf8_lossy(&apply_out.stderr);
            let stdout = String::from_utf8_lossy(&apply_out.stdout);
            let details = format!("{stdout}{stderr}").trim().to_string();

            tracing::error!(
                stash_ref = %stash_ref,
                cwd = %cwd_str,
                details = %details,
                "git stash apply conflicted — stash entry is preserved for manual recovery"
            );

            return Err(SnapshotError::RollbackConflict {
                stash_ref,
                cwd: cwd_str.to_string(),
                details,
            });
        }

        let drop_out = Command::new("git")
            .args(["stash", "drop", &stash_ref])
            .current_dir(cwd_str)
            .output()
            .await;
        if !drop_out.map(|o| o.status.success()).unwrap_or(false) {
            tracing::warn!(stash_ref = %stash_ref, "git stash drop failed after successful apply");
        }

        tracing::info!(stash_ref = %stash_ref, "git snapshot rolled back");
        Ok(())
    }

    async fn delete(&self, snapshot_id: &str) -> Result<()> {
        if snapshot_id == CLEAN_SENTINEL {
            tracing::info!("git snapshot was clean, nothing to delete");
            return Ok(());
        }

        let (cwd_str, hash) = snapshot_id.split_once(SEP).ok_or_else(|| {
            SnapshotError::Snapshot(format!("malformed snapshot_id: {snapshot_id:?}"))
        })?;

        let cwd_path = Path::new(cwd_str);
        if !cwd_path.exists() {
            tracing::info!(%snapshot_id, "git repository no longer exists, nothing to delete");
            return Ok(());
        }

        let list_out = Command::new("git")
            .args(["stash", "list", "--format=%H %gd"])
            .current_dir(cwd_path)
            .output()
            .await
            .map_err(|e| SnapshotError::Snapshot(format!("git stash list failed: {e}")))?;

        if !list_out.status.success() {
            let stderr = String::from_utf8_lossy(&list_out.stderr);
            if stderr.to_lowercase().contains("not a git repository") {
                tracing::info!(%snapshot_id, "git repository no longer exists, nothing to delete");
                return Ok(());
            }
            return Err(SnapshotError::Snapshot(format!(
                "git stash list failed: {stderr}"
            )));
        }

        let list_stdout = String::from_utf8_lossy(&list_out.stdout);
        let stash_ref = list_stdout.lines().find_map(|line| {
            let (h, r) = line.split_once(' ')?;
            (h == hash).then(|| r.to_string())
        });

        let Some(stash_ref) = stash_ref else {
            tracing::info!(%snapshot_id, "git stash entry already removed");
            return Ok(());
        };

        let drop_out = Command::new("git")
            .args(["stash", "drop", &stash_ref])
            .current_dir(cwd_path)
            .output()
            .await;

        match drop_out {
            Ok(output) if output.status.success() => {
                tracing::info!(%stash_ref, "git snapshot deleted");
                Ok(())
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr
                    .to_lowercase()
                    .contains("is not a valid stash reference")
                {
                    tracing::info!(%stash_ref, "git stash entry already removed");
                    return Ok(());
                }
                Err(SnapshotError::DeleteFailed {
                    plugin: "git".to_string(),
                    snapshot_id: snapshot_id.to_string(),
                    source: format!("git stash drop failed: {stderr}"),
                })
            }
            Err(error) => Err(SnapshotError::DeleteFailed {
                plugin: "git".to_string(),
                snapshot_id: snapshot_id.to_string(),
                source: format!("git stash drop failed: {error}"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Initialise a bare git repo with an empty initial commit so stash works.
    async fn init_repo(dir: &std::path::Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .await
            .unwrap();
        // Set local identity so stash commits don't depend on global git config.
        for (key, val) in [
            ("user.email", "test@aegis.dev"),
            ("user.name", "Aegis Test"),
        ] {
            Command::new("git")
                .args(["config", key, val])
                .current_dir(dir)
                .output()
                .await
                .unwrap();
        }
        // Stash requires at least one commit; create an empty one.
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir)
            .output()
            .await
            .unwrap();
    }

    /// Write `content` to `name`, stage it, and commit it.
    async fn commit_file(dir: &std::path::Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
        Command::new("git")
            .args(["add", name])
            .current_dir(dir)
            .output()
            .await
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
            .current_dir(dir)
            .output()
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn is_applicable_outside_repo() {
        let dir = TempDir::new().unwrap();
        assert!(!GitPlugin.is_applicable(dir.path()).await);
    }

    #[tokio::test]
    async fn is_applicable_at_repo_root() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path()).await;
        assert!(GitPlugin.is_applicable(dir.path()).await);
    }

    #[tokio::test]
    async fn is_applicable_in_repo_subdirectory() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path()).await;
        let sub = dir.path().join("deep/nested/dir");
        fs::create_dir_all(&sub).unwrap();
        // Should detect the repo even though there is no .git in this subdirectory.
        assert!(GitPlugin.is_applicable(&sub).await);
    }

    #[tokio::test]
    async fn snapshot_clean_tree_returns_sentinel() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path()).await;

        let id = GitPlugin.snapshot(dir.path(), "rm -rf .").await.unwrap();
        assert_eq!(id, CLEAN_SENTINEL);
    }

    #[tokio::test]
    async fn snapshot_and_rollback_restores_changes() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path()).await;
        commit_file(dir.path(), "hello.txt", "original\n").await;

        // Introduce an uncommitted change.
        fs::write(dir.path().join("hello.txt"), "modified\n").unwrap();

        let snapshot_id = GitPlugin.snapshot(dir.path(), "rm -rf .").await.unwrap();
        assert_ne!(snapshot_id, CLEAN_SENTINEL, "expected a real stash");

        // File should be back to the committed version.
        assert_eq!(
            fs::read_to_string(dir.path().join("hello.txt"))
                .unwrap()
                .trim(),
            "original"
        );

        GitPlugin.rollback(&snapshot_id).await.unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("hello.txt"))
                .unwrap()
                .trim(),
            "modified"
        );
    }

    #[tokio::test]
    async fn rollback_clean_sentinel_is_noop() {
        // Rolling back a "clean" snapshot must succeed without touching git.
        GitPlugin.rollback(CLEAN_SENTINEL).await.unwrap();
    }

    // ── untracked files ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn snapshot_includes_untracked_files() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path()).await;

        // New file, never `git add`'ed.
        let new_file = dir.path().join("untracked.txt");
        fs::write(&new_file, "brand new\n").unwrap();

        let snapshot_id = GitPlugin.snapshot(dir.path(), "rm -rf .").await.unwrap();
        assert_ne!(snapshot_id, CLEAN_SENTINEL);

        // File should have been swept into the stash.
        assert!(!new_file.exists(), "untracked file should be stashed");

        GitPlugin.rollback(&snapshot_id).await.unwrap();
        assert_eq!(fs::read_to_string(&new_file).unwrap().trim(), "brand new");
    }

    // ── staged + unstaged changes ────────────────────────────────────────────

    #[tokio::test]
    async fn snapshot_and_rollback_preserves_index() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path()).await;
        commit_file(dir.path(), "staged.txt", "base\n").await;
        commit_file(dir.path(), "unstaged.txt", "base\n").await;

        // Stage a change to staged.txt.
        fs::write(dir.path().join("staged.txt"), "staged-change\n").unwrap();
        Command::new("git")
            .args(["add", "staged.txt"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        // Leave a change to unstaged.txt outside the index.
        fs::write(dir.path().join("unstaged.txt"), "unstaged-change\n").unwrap();

        let snapshot_id = GitPlugin.snapshot(dir.path(), "rm -rf .").await.unwrap();
        assert_ne!(snapshot_id, CLEAN_SENTINEL);

        // Both files are back to committed state after the stash.
        assert_eq!(
            fs::read_to_string(dir.path().join("staged.txt"))
                .unwrap()
                .trim(),
            "base"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("unstaged.txt"))
                .unwrap()
                .trim(),
            "base"
        );

        GitPlugin.rollback(&snapshot_id).await.unwrap();

        // Both changes are restored.
        assert_eq!(
            fs::read_to_string(dir.path().join("staged.txt"))
                .unwrap()
                .trim(),
            "staged-change"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("unstaged.txt"))
                .unwrap()
                .trim(),
            "unstaged-change"
        );

        // staged.txt should be in the index after rollback (--index flag).
        let status = Command::new("git")
            .args(["diff", "--cached", "--name-only"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();
        let staged_files = String::from_utf8_lossy(&status.stdout);
        assert!(
            staged_files.contains("staged.txt"),
            "staged.txt should still be staged after rollback"
        );
    }

    // ── snapshot from a repo subdirectory ────────────────────────────────────

    #[tokio::test]
    async fn snapshot_and_rollback_from_subdirectory() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path()).await;
        commit_file(dir.path(), "file.txt", "original\n").await;

        let sub = dir.path().join("subdir");
        fs::create_dir_all(&sub).unwrap();

        // Modify the file from the repo root, but run snapshot from a subdir.
        fs::write(dir.path().join("file.txt"), "modified\n").unwrap();

        let snapshot_id = GitPlugin.snapshot(&sub, "rm -rf .").await.unwrap();
        assert_ne!(snapshot_id, CLEAN_SENTINEL);

        assert_eq!(
            fs::read_to_string(dir.path().join("file.txt"))
                .unwrap()
                .trim(),
            "original"
        );

        GitPlugin.rollback(&snapshot_id).await.unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("file.txt"))
                .unwrap()
                .trim(),
            "modified"
        );
    }

    // ── worktree ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn is_applicable_in_worktree() {
        let main_dir = TempDir::new().unwrap();
        init_repo(main_dir.path()).await;
        // A worktree needs a branch name; HEAD is fine for detection.
        let wt_dir = TempDir::new().unwrap();
        let out = Command::new("git")
            .args([
                "worktree",
                "add",
                wt_dir.path().to_str().unwrap(),
                "HEAD",
                "--detach",
            ])
            .current_dir(main_dir.path())
            .output()
            .await
            .unwrap();
        // Skip if git worktree is unavailable in this environment.
        if !out.status.success() {
            return;
        }
        assert!(
            GitPlugin.is_applicable(wt_dir.path()).await,
            "worktree should be detected as a git repo"
        );
    }

    #[tokio::test]
    async fn snapshot_and_rollback_in_worktree() {
        let main_dir = TempDir::new().unwrap();
        init_repo(main_dir.path()).await;
        commit_file(main_dir.path(), "file.txt", "original\n").await;

        let wt_dir = TempDir::new().unwrap();
        let out = Command::new("git")
            .args([
                "worktree",
                "add",
                wt_dir.path().to_str().unwrap(),
                "HEAD",
                "--detach",
            ])
            .current_dir(main_dir.path())
            .output()
            .await
            .unwrap();
        if !out.status.success() {
            return;
        }

        // Modify the file inside the worktree.
        fs::write(wt_dir.path().join("file.txt"), "modified\n").unwrap();

        let snapshot_id = GitPlugin.snapshot(wt_dir.path(), "rm -rf .").await.unwrap();
        assert_ne!(snapshot_id, CLEAN_SENTINEL);

        assert_eq!(
            fs::read_to_string(wt_dir.path().join("file.txt"))
                .unwrap()
                .trim(),
            "original"
        );

        GitPlugin.rollback(&snapshot_id).await.unwrap();
        assert_eq!(
            fs::read_to_string(wt_dir.path().join("file.txt"))
                .unwrap()
                .trim(),
            "modified"
        );
    }

    // ── rollback conflict ────────────────────────────────────────────────────

    #[tokio::test]
    async fn rollback_returns_conflict_error_with_recovery_hint() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path()).await;
        commit_file(dir.path(), "file.txt", "original\n").await;

        // Stash a diverging change.
        fs::write(dir.path().join("file.txt"), "stashed-content\n").unwrap();
        let snapshot_id = GitPlugin.snapshot(dir.path(), "rm -rf .").await.unwrap();
        assert_ne!(snapshot_id, CLEAN_SENTINEL);

        // Introduce a conflicting change so stash pop cannot auto-merge.
        fs::write(dir.path().join("file.txt"), "conflicting-content\n").unwrap();

        let err = GitPlugin
            .rollback(&snapshot_id)
            .await
            .expect_err("expected a conflict error");

        match err {
            SnapshotError::RollbackConflict {
                ref stash_ref,
                ref cwd,
                ..
            } => {
                // The stash ref should be a positional ref (stash@{N}).
                assert!(
                    stash_ref.starts_with("stash@{"),
                    "stash_ref should be a positional ref, got: {stash_ref}"
                );
                // The cwd must be present so the user knows where to recover.
                assert!(!cwd.is_empty(), "cwd must be non-empty");
                // The error message must contain recovery instructions.
                let msg = err.to_string();
                assert!(
                    msg.contains("git stash drop"),
                    "message should include drop command: {msg}"
                );
                assert!(
                    msg.contains("git diff"),
                    "message should include diff command: {msg}"
                );
            }
            other => panic!("expected RollbackConflict, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn rollback_rejects_malformed_snapshot_id() {
        let err = GitPlugin
            .rollback("not-a-valid-snapshot-id")
            .await
            .expect_err("malformed snapshot id should fail");

        match err {
            SnapshotError::Snapshot(msg) => assert!(msg.contains("malformed snapshot_id")),
            other => panic!("expected snapshot error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rollback_errors_when_stash_entry_not_found() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path()).await;

        let snapshot_id = format!(
            "{}\t0000000000000000000000000000000000000000",
            dir.path().display()
        );
        let err = GitPlugin
            .rollback(&snapshot_id)
            .await
            .expect_err("missing stash hash should fail");

        match err {
            SnapshotError::Snapshot(msg) => assert!(msg.contains("stash entry not found")),
            other => panic!("expected snapshot error, got {other:?}"),
        }
    }
}
