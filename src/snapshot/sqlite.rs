use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

use crate::error::AegisError;
use crate::snapshot::SnapshotPlugin;

type Result<T> = std::result::Result<T, AegisError>;

const SEP: char = '\t';

/// Snapshot plugin for SQLite database files.
pub struct SqlitePlugin {
    db_path: PathBuf,
    snapshots_dir: PathBuf,
}

impl SqlitePlugin {
    /// Create a new SQLite snapshot plugin.
    pub fn new(db_path: PathBuf, snapshots_dir: PathBuf) -> Self {
        Self {
            db_path,
            snapshots_dir,
        }
    }
}

#[async_trait]
impl SnapshotPlugin for SqlitePlugin {
    fn name(&self) -> &'static str {
        "sqlite"
    }

    fn is_applicable(&self, _cwd: &Path) -> bool {
        !self.db_path.as_os_str().is_empty() && self.db_path.is_file()
    }

    async fn snapshot(&self, _cwd: &Path, _cmd: &str) -> Result<String> {
        fs::create_dir_all(&self.snapshots_dir)?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let file_stem = self
            .db_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("db");
        let base_name = format!("sqlite-{file_stem}-{timestamp}");
        let mut dump_path = self.snapshots_dir.join(format!("{base_name}.db"));
        let mut suffix = 1usize;
        while dump_path.exists() {
            dump_path = self
                .snapshots_dir
                .join(format!("{base_name}-{suffix}.db"));
            suffix += 1;
        }

        fs::copy(&self.db_path, &dump_path)?;

        let snapshot_id = format!("{}{SEP}{}", self.db_path.display(), dump_path.display());
        tracing::info!(%snapshot_id, "sqlite snapshot created");
        Ok(snapshot_id)
    }

    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        let (original_str, dump_str) = snapshot_id.split_once(SEP).ok_or_else(|| {
            AegisError::Snapshot(format!("malformed snapshot_id: {snapshot_id:?}"))
        })?;

        let dump_path = Path::new(dump_str);
        if !dump_path.exists() {
            return Err(AegisError::RollbackDumpNotFound {
                path: dump_str.to_string(),
            });
        }

        fs::copy(dump_path, original_str)?;
        tracing::info!(snapshot_id = snapshot_id, "sqlite snapshot rolled back");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_db(path: &Path, contents: &[u8]) {
        fs::write(path, contents).unwrap();
    }

    #[tokio::test]
    async fn is_applicable_when_file_exists() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        write_db(&db_path, b"sqlite-data");

        let plugin = SqlitePlugin::new(db_path, temp_dir.path().join("snaps"));

        assert!(plugin.is_applicable(temp_dir.path()));
    }

    #[tokio::test]
    async fn is_not_applicable_when_file_missing_direct() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = SqlitePlugin::new(
            temp_dir.path().join("missing.db"),
            temp_dir.path().join("snaps"),
        );

        assert!(!plugin.is_applicable(temp_dir.path()));
    }

    #[tokio::test]
    async fn is_not_applicable_when_path_is_directory() {
        let temp_dir = TempDir::new().unwrap();
        let db_dir = temp_dir.path().join("app.db");
        fs::create_dir_all(&db_dir).unwrap();

        let plugin = SqlitePlugin::new(db_dir, temp_dir.path().join("snaps"));

        assert!(!plugin.is_applicable(temp_dir.path()));
    }

    #[tokio::test]
    async fn snapshot_copies_file_and_returns_encoded_id() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let snapshots_dir = temp_dir.path().join("snaps");
        write_db(&db_path, b"before-snapshot");
        let plugin = SqlitePlugin::new(db_path.clone(), snapshots_dir.clone());

        let snapshot_id = plugin
            .snapshot(temp_dir.path(), "sqlite-command")
            .await
            .unwrap();
        let (original, dump) = snapshot_id.split_once(SEP).unwrap();

        assert_eq!(original, db_path.to_string_lossy());
        assert_eq!(
            PathBuf::from(dump).parent().unwrap(),
            snapshots_dir.as_path()
        );
        assert_eq!(fs::read(dump).unwrap(), b"before-snapshot");
    }

    #[tokio::test]
    async fn rollback_restores_original_file() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let snapshots_dir = temp_dir.path().join("snaps");
        write_db(&db_path, b"before-snapshot");
        let plugin = SqlitePlugin::new(db_path.clone(), snapshots_dir);

        let snapshot_id = plugin
            .snapshot(temp_dir.path(), "sqlite-command")
            .await
            .unwrap();
        write_db(&db_path, b"after-change");

        plugin.rollback(&snapshot_id).await.unwrap();

        assert_eq!(fs::read(&db_path).unwrap(), b"before-snapshot");
    }

    #[tokio::test]
    async fn rollback_errors_when_dump_file_missing() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let snapshots_dir = temp_dir.path().join("snaps");
        write_db(&db_path, b"before-snapshot");
        let plugin = SqlitePlugin::new(db_path, snapshots_dir);

        let snapshot_id = plugin
            .snapshot(temp_dir.path(), "sqlite-command")
            .await
            .unwrap();
        let (_, dump_path) = snapshot_id.split_once(SEP).unwrap();
        fs::remove_file(dump_path).unwrap();

        let err = plugin.rollback(&snapshot_id).await.unwrap_err();

        match err {
            AegisError::RollbackDumpNotFound { path } => assert_eq!(path, dump_path),
            other => panic!("expected RollbackDumpNotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rollback_errors_when_snapshot_id_is_malformed() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = SqlitePlugin::new(
            temp_dir.path().join("app.db"),
            temp_dir.path().join("snaps"),
        );

        let err = plugin.rollback("not-a-valid-snapshot-id").await.unwrap_err();

        match err {
            AegisError::Snapshot(msg) => assert!(msg.contains("malformed snapshot_id")),
            other => panic!("expected malformed snapshot snapshot error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn snapshot_generates_distinct_ids_for_back_to_back_calls() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let snapshots_dir = temp_dir.path().join("snaps");
        write_db(&db_path, b"before-snapshot");
        let plugin = SqlitePlugin::new(db_path.clone(), snapshots_dir);

        let first_id = plugin
            .snapshot(temp_dir.path(), "sqlite-command")
            .await
            .unwrap();
        let (_, first_dump) = first_id.split_once(SEP).unwrap();

        write_db(&db_path, b"after-first-snapshot");

        let second_id = plugin
            .snapshot(temp_dir.path(), "sqlite-command")
            .await
            .unwrap();
        let (_, second_dump) = second_id.split_once(SEP).unwrap();

        assert_ne!(first_id, second_id);
        assert_ne!(first_dump, second_dump);
        assert_eq!(fs::read(first_dump).unwrap(), b"before-snapshot");
        assert_eq!(fs::read(second_dump).unwrap(), b"after-first-snapshot");
    }
}
