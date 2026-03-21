use std::path::Path;

use async_trait::async_trait;
use tokio::process::Command;

use crate::error::AegisError;
use crate::snapshot::SnapshotPlugin;

type Result<T> = std::result::Result<T, AegisError>;

/// Sentinel value stored when the working tree had no changes to stash.
/// A real stash hash is always a 40-char hex string, so this cannot collide.
const CLEAN_SENTINEL: &str = "clean";

/// Separator between the encoded `cwd` and the stash hash in a `snapshot_id`.
/// Tab is not a valid path component on Unix or Windows, so it is safe to use
/// as an unambiguous delimiter.
const SEP: char = '\t';

pub struct GitPlugin;

#[async_trait]
impl SnapshotPlugin for GitPlugin {
    fn name(&self) -> &'static str {
        "git"
    }

    fn is_applicable(&self, cwd: &Path) -> bool {
        cwd.join(".git").exists()
    }

    async fn snapshot(&self, cwd: &Path, _cmd: &str) -> Result<String> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let message = format!("aegis-snap-{timestamp}");

        let stash_out = Command::new("git")
            .args(["stash", "push", "--include-untracked", "-m", &message])
            .current_dir(cwd)
            .output()
            .await
            .map_err(|e| AegisError::Snapshot(format!("failed to run git stash: {e}")))?;

        let stdout = String::from_utf8_lossy(&stash_out.stdout);

        // Working tree was clean — nothing to stash.
        if stdout.contains("No local changes to save") {
            tracing::info!("git working tree is clean, nothing to stash");
            return Ok(CLEAN_SENTINEL.to_string());
        }

        if !stash_out.status.success() {
            let stderr = String::from_utf8_lossy(&stash_out.stderr);
            return Err(AegisError::Snapshot(format!(
                "git stash push failed: {stderr}"
            )));
        }

        // Resolve the stash to a stable hash. The positional ref `stash@{0}`
        // would shift if another stash is pushed later, but the hash is permanent.
        let rev_out = Command::new("git")
            .args(["rev-parse", "stash@{0}"])
            .current_dir(cwd)
            .output()
            .await
            .map_err(|e| AegisError::Snapshot(format!("git rev-parse failed: {e}")))?;

        if !rev_out.status.success() {
            return Err(AegisError::Snapshot(
                "could not resolve stash ref after push".to_string(),
            ));
        }

        let hash = String::from_utf8_lossy(&rev_out.stdout).trim().to_string();

        // Encode both cwd and hash so rollback can re-enter the correct repo.
        let snapshot_id = format!("{}{SEP}{hash}", cwd.display());
        tracing::info!(%snapshot_id, "git snapshot created");
        Ok(snapshot_id)
    }

    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        if snapshot_id == CLEAN_SENTINEL {
            tracing::info!("git snapshot was clean, nothing to roll back");
            return Ok(());
        }

        // Parse the encoded snapshot_id: "<cwd>\t<hash>"
        let (cwd_str, hash) = snapshot_id.split_once(SEP).ok_or_else(|| {
            AegisError::Snapshot(format!("malformed snapshot_id: {snapshot_id:?}"))
        })?;

        // Find the stash entry that matches the saved hash.
        // `git stash list --format="%H %gd"` prints "<hash> stash@{N}" per line.
        let list_out = Command::new("git")
            .args(["stash", "list", "--format=%H %gd"])
            .current_dir(cwd_str)
            .output()
            .await
            .map_err(|e| AegisError::Snapshot(format!("git stash list failed: {e}")))?;

        if !list_out.status.success() {
            return Err(AegisError::Snapshot("git stash list failed".to_string()));
        }

        let list_stdout = String::from_utf8_lossy(&list_out.stdout);
        let stash_ref = list_stdout
            .lines()
            .find_map(|line| {
                let (h, r) = line.split_once(' ')?;
                (h == hash).then(|| r.to_string())
            })
            .ok_or_else(|| {
                AegisError::Snapshot(format!("stash entry not found for hash {hash}"))
            })?;

        let pop_out = Command::new("git")
            .args(["stash", "pop", "--index", &stash_ref])
            .current_dir(cwd_str)
            .output()
            .await
            .map_err(|e| AegisError::Snapshot(format!("git stash pop failed: {e}")))?;

        if !pop_out.status.success() {
            let stderr = String::from_utf8_lossy(&pop_out.stderr);
            return Err(AegisError::Snapshot(format!(
                "git stash pop failed: {stderr}"
            )));
        }

        tracing::info!(stash_ref = %stash_ref, "git snapshot rolled back");
        Ok(())
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
        // Stash requires at least one commit; create an empty one.
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
            .current_dir(dir)
            .output()
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn is_applicable_without_git_dir() {
        let dir = TempDir::new().unwrap();
        assert!(!GitPlugin.is_applicable(dir.path()));
    }

    #[tokio::test]
    async fn is_applicable_with_git_dir() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path()).await;
        assert!(GitPlugin.is_applicable(dir.path()));
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

        // Create and commit a file so there is a base state.
        let file = dir.path().join("hello.txt");
        fs::write(&file, "original\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
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
                "add file",
            ])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        // Introduce an uncommitted change.
        fs::write(&file, "modified\n").unwrap();

        // Snapshot stashes the change.
        let snapshot_id = GitPlugin
            .snapshot(dir.path(), "rm -rf .")
            .await
            .unwrap();
        assert_ne!(snapshot_id, CLEAN_SENTINEL, "expected a real stash");

        // File should be back to the committed version (trim to ignore CRLF on Windows/WSL).
        assert_eq!(fs::read_to_string(&file).unwrap().trim(), "original");

        // Rollback restores the modification.
        GitPlugin.rollback(&snapshot_id).await.unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap().trim(), "modified");
    }

    #[tokio::test]
    async fn rollback_clean_sentinel_is_noop() {
        // Rolling back a "clean" snapshot must succeed without touching git.
        GitPlugin.rollback(CLEAN_SENTINEL).await.unwrap();
    }
}
