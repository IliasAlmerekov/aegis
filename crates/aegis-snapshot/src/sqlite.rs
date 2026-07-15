//! SQLite snapshot provider.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use crate::SnapshotPlugin;
use crate::containment::contain_artifact;
use crate::error::SnapshotError;
use crate::secure_fs::{create_artifact_file, create_store_dir};

type Result<T> = std::result::Result<T, SnapshotError>;

const SEP: char = '\t';
const SNAPSHOT_ID_VERSION: &str = "v2";

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

    fn resolve_db_path(&self, cwd: &Path) -> PathBuf {
        if self.db_path.is_relative() {
            cwd.join(&self.db_path)
        } else {
            self.db_path.clone()
        }
    }

    fn encode_path(path: &Path) -> String {
        #[cfg(unix)]
        {
            path.as_os_str()
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        }

        #[cfg(not(unix))]
        {
            path.to_string_lossy()
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        }
    }

    fn decode_path(encoded: &str, label: &str) -> Result<PathBuf> {
        if !encoded.len().is_multiple_of(2) {
            return Err(SnapshotError::Snapshot(format!(
                "malformed snapshot_id: invalid {label} encoding {encoded:?}"
            )));
        }

        let mut bytes = Vec::with_capacity(encoded.len() / 2);
        for pair in encoded.as_bytes().chunks_exact(2) {
            let hex = std::str::from_utf8(pair).map_err(|_| {
                SnapshotError::Snapshot(format!(
                    "malformed snapshot_id: invalid {label} encoding {encoded:?}"
                ))
            })?;
            let byte = u8::from_str_radix(hex, 16).map_err(|_| {
                SnapshotError::Snapshot(format!(
                    "malformed snapshot_id: invalid {label} encoding {encoded:?}"
                ))
            })?;
            bytes.push(byte);
        }

        #[cfg(unix)]
        let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));

        #[cfg(not(unix))]
        let path = PathBuf::from(String::from_utf8(bytes).map_err(|_| {
            SnapshotError::Snapshot(format!(
                "malformed snapshot_id: invalid {label} encoding {encoded:?}"
            ))
        })?);

        Ok(path)
    }

    fn build_snapshot_id(original_path: &Path, dump_path: &Path) -> String {
        format!(
            "{SNAPSHOT_ID_VERSION}{SEP}{}{SEP}{}",
            Self::encode_path(original_path),
            Self::encode_path(dump_path)
        )
    }

    fn parse_snapshot_id(snapshot_id: &str) -> Result<(PathBuf, PathBuf)> {
        if snapshot_id.starts_with(&format!("{SNAPSHOT_ID_VERSION}{SEP}")) {
            return Self::parse_v2_snapshot_id(snapshot_id);
        }

        Self::parse_legacy_snapshot_id(snapshot_id)
    }

    fn parse_v2_snapshot_id(snapshot_id: &str) -> Result<(PathBuf, PathBuf)> {
        let parts: Vec<_> = snapshot_id.split(SEP).collect();
        if parts.len() != 3 || parts[0] != SNAPSHOT_ID_VERSION {
            return Err(SnapshotError::Snapshot(format!(
                "malformed snapshot_id: {snapshot_id:?}"
            )));
        }

        Ok((
            Self::decode_path(parts[1], "original path")?,
            Self::decode_path(parts[2], "dump path")?,
        ))
    }

    fn parse_legacy_snapshot_id(snapshot_id: &str) -> Result<(PathBuf, PathBuf)> {
        let (original_str, dump_str) = snapshot_id.split_once(SEP).ok_or_else(|| {
            SnapshotError::Snapshot(format!("malformed snapshot_id: {snapshot_id:?}"))
        })?;
        let original_path = PathBuf::from(original_str);
        let dump_path = PathBuf::from(dump_str);
        Ok((original_path, dump_path))
    }
}

async fn path_points_to_file(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

#[async_trait]
impl SnapshotPlugin for SqlitePlugin {
    fn name(&self) -> &'static str {
        "sqlite"
    }

    async fn is_applicable(&self, cwd: &Path) -> bool {
        if self.db_path.as_os_str().is_empty() {
            return false;
        }

        let db_path = self.resolve_db_path(cwd);
        path_points_to_file(&db_path).await
    }

    async fn snapshot(&self, cwd: &Path, _cmd: &str) -> Result<String> {
        let db_path = self.resolve_db_path(cwd).canonicalize()?;
        create_store_dir("sqlite", &self.snapshots_dir)?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let file_stem = db_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("db");
        let base_name = format!("sqlite-{file_stem}-{timestamp}");
        let mut dump_path = contain_artifact(
            "sqlite",
            &self.snapshots_dir,
            &self.snapshots_dir.join(format!("{base_name}.db")),
        )?;
        let mut suffix = 1usize;
        let mut dump_file = loop {
            match create_artifact_file("sqlite", &dump_path) {
                Ok(file) => break file,
                Err(SnapshotError::Io(error))
                    if error.kind() == std::io::ErrorKind::AlreadyExists =>
                {
                    dump_path = contain_artifact(
                        "sqlite",
                        &self.snapshots_dir,
                        &self.snapshots_dir.join(format!("{base_name}-{suffix}.db")),
                    )?;
                    suffix += 1;
                }
                Err(error) => return Err(error),
            }
        };
        let mut source = fs::File::open(&db_path)?;
        std::io::copy(&mut source, &mut dump_file)?;
        drop(dump_file);
        let dump_path = dump_path.canonicalize()?;

        let snapshot_id = Self::build_snapshot_id(&db_path, &dump_path);
        tracing::info!(%snapshot_id, "sqlite snapshot created");
        Ok(snapshot_id)
    }

    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        let (_original_path, dump_path) = Self::parse_snapshot_id(snapshot_id)?;
        let dump_path = contain_artifact("sqlite", &self.snapshots_dir, &dump_path)?;
        if !dump_path.exists() {
            return Err(SnapshotError::RollbackDumpNotFound {
                path: dump_path.to_string_lossy().to_string(),
            });
        }

        let cwd = std::env::current_dir()?;
        let restore_path = self.resolve_db_path(&cwd);
        let mut dump_file = fs::File::open(dump_path)?;
        let mut restore_file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(restore_path)?;
        std::io::copy(&mut dump_file, &mut restore_file)?;
        tracing::info!(snapshot_id = snapshot_id, "sqlite snapshot rolled back");
        Ok(())
    }

    async fn delete(&self, snapshot_id: &str) -> Result<()> {
        let (_original_path, dump_path) = Self::parse_snapshot_id(snapshot_id)?;
        let dump_path = contain_artifact("sqlite", &self.snapshots_dir, &dump_path)?;
        match fs::remove_file(&dump_path) {
            Ok(()) => {
                tracing::info!(path = %dump_path.display(), "sqlite dump deleted");
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(path = %dump_path.display(), "sqlite dump already removed");
                Ok(())
            }
            Err(error) => Err(SnapshotError::DeleteFailed {
                plugin: "sqlite".to_string(),
                snapshot_id: snapshot_id.to_string(),
                source: error.to_string(),
            }),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::secure_fs::{inject_effective_uid, inject_store_metadata_failure};
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn write_db(path: &Path, contents: &[u8]) {
        fs::write(path, contents).unwrap();
    }

    fn decode_hex(encoded: &str) -> String {
        let mut bytes = Vec::with_capacity(encoded.len() / 2);
        for pair in encoded.as_bytes().chunks_exact(2) {
            let hex = std::str::from_utf8(pair).unwrap();
            let byte = u8::from_str_radix(hex, 16).unwrap();
            bytes.push(byte);
        }

        String::from_utf8(bytes).unwrap()
    }

    #[tokio::test]
    async fn is_applicable_when_file_exists() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        write_db(&db_path, b"sqlite-data");

        let plugin = SqlitePlugin::new(db_path, temp_dir.path().join("snaps"));

        assert!(plugin.is_applicable(temp_dir.path()).await);
    }

    #[tokio::test]
    async fn is_not_applicable_when_file_missing_direct() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = SqlitePlugin::new(
            temp_dir.path().join("missing.db"),
            temp_dir.path().join("snaps"),
        );

        assert!(!plugin.is_applicable(temp_dir.path()).await);
    }

    #[tokio::test]
    async fn is_not_applicable_when_path_is_directory() {
        let temp_dir = TempDir::new().unwrap();
        let db_dir = temp_dir.path().join("app.db");
        fs::create_dir_all(&db_dir).unwrap();

        let plugin = SqlitePlugin::new(db_dir, temp_dir.path().join("snaps"));

        assert!(!plugin.is_applicable(temp_dir.path()).await);
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
        let parts: Vec<_> = snapshot_id.split(SEP).collect();
        let canonical_db_path = db_path.canonicalize().unwrap();
        let canonical_snapshots_dir = snapshots_dir.canonicalize().unwrap();

        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "v2");
        assert_eq!(decode_hex(parts[1]), canonical_db_path.to_string_lossy());
        let dump = PathBuf::from(decode_hex(parts[2]));
        assert_eq!(dump.parent().unwrap(), canonical_snapshots_dir.as_path());
        assert_eq!(fs::read(&dump).unwrap(), b"before-snapshot");
        assert_eq!(
            fs::metadata(&canonical_snapshots_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700,
            "the snapshot store must be owner-only on Unix"
        );
        assert_eq!(
            fs::metadata(&dump).unwrap().permissions().mode() & 0o777,
            0o600,
            "the SQLite snapshot artifact must be owner-readable and writable only"
        );
    }

    #[tokio::test]
    async fn snapshot_tightens_an_existing_owner_owned_store_before_copying() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let snapshots_dir = temp_dir.path().join("snaps");
        write_db(&db_path, b"before-snapshot");
        fs::create_dir(&snapshots_dir).unwrap();
        fs::set_permissions(&snapshots_dir, fs::Permissions::from_mode(0o755)).unwrap();
        let plugin = SqlitePlugin::new(db_path, snapshots_dir.clone());

        plugin
            .snapshot(temp_dir.path(), "sqlite-command")
            .await
            .unwrap();

        assert_eq!(
            fs::metadata(snapshots_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[tokio::test]
    async fn snapshot_rejects_a_symlinked_store_before_copying_sensitive_data() {
        use std::os::unix::fs::symlink;

        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let outside_dir = temp_dir.path().join("outside");
        let snapshots_dir = temp_dir.path().join("snaps");
        write_db(&db_path, b"before-snapshot");
        fs::create_dir(&outside_dir).unwrap();
        symlink(&outside_dir, &snapshots_dir).unwrap();
        let plugin = SqlitePlugin::new(db_path.clone(), snapshots_dir);

        let err = plugin
            .snapshot(temp_dir.path(), "sqlite-command")
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            SnapshotError::InsecureSnapshotPermissions { plugin, .. } if plugin == "sqlite"
        ));
        assert!(fs::read_dir(outside_dir).unwrap().next().is_none());
        assert_eq!(fs::read(db_path).unwrap(), b"before-snapshot");
    }

    #[tokio::test]
    async fn snapshot_rejects_unreadable_store_metadata_before_copying_sensitive_data() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let snapshots_dir = temp_dir.path().join("snaps");
        write_db(&db_path, b"before-snapshot");
        inject_store_metadata_failure();
        let plugin = SqlitePlugin::new(db_path.clone(), snapshots_dir.clone());

        let err = plugin
            .snapshot(temp_dir.path(), "sqlite-command")
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            SnapshotError::InsecureSnapshotPermissions { plugin, .. } if plugin == "sqlite"
        ));
        assert!(!snapshots_dir.exists());
        assert_eq!(fs::read(db_path).unwrap(), b"before-snapshot");
    }

    #[tokio::test]
    async fn snapshot_rejects_a_store_owned_by_another_uid_before_copying_sensitive_data() {
        use std::os::unix::fs::MetadataExt;

        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let snapshots_dir = temp_dir.path().join("snaps");
        write_db(&db_path, b"before-snapshot");
        fs::create_dir(&snapshots_dir).unwrap();
        let owner_uid = fs::metadata(&snapshots_dir).unwrap().uid();
        inject_effective_uid(owner_uid.wrapping_add(1));
        let plugin = SqlitePlugin::new(db_path.clone(), snapshots_dir.clone());

        let err = plugin
            .snapshot(temp_dir.path(), "sqlite-command")
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            SnapshotError::InsecureSnapshotPermissions { plugin, .. } if plugin == "sqlite"
        ));
        assert!(fs::read_dir(snapshots_dir).unwrap().next().is_none());
        assert_eq!(fs::read(db_path).unwrap(), b"before-snapshot");
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
        fs::set_permissions(&db_path, fs::Permissions::from_mode(0o664)).unwrap();
        write_db(&db_path, b"after-change");

        plugin.rollback(&snapshot_id).await.unwrap();

        assert_eq!(fs::read(&db_path).unwrap(), b"before-snapshot");
        assert_eq!(
            fs::metadata(&db_path).unwrap().permissions().mode() & 0o777,
            0o664,
            "rollback must not modify the caller-owned live database permissions"
        );
    }

    #[tokio::test]
    async fn rollback_rejects_forged_artifact_path_without_modifying_database() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let snapshots_dir = temp_dir.path().join("snaps");
        let outside_path = temp_dir.path().join("outside.db");
        fs::create_dir_all(&snapshots_dir).unwrap();
        write_db(&db_path, b"configured-database");
        write_db(&outside_path, b"outside-artifact");
        let plugin = SqlitePlugin::new(db_path.clone(), snapshots_dir);
        let snapshot_id = SqlitePlugin::build_snapshot_id(&db_path, &outside_path);

        let err = plugin.rollback(&snapshot_id).await.unwrap_err();

        assert!(matches!(
            err,
            SnapshotError::PathEscapesSnapshotStore {
                plugin: "sqlite",
                ..
            }
        ));
        assert_eq!(fs::read(db_path).unwrap(), b"configured-database");
    }

    #[tokio::test]
    async fn rollback_uses_configured_database_instead_of_snapshot_id_destination() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let snapshots_dir = temp_dir.path().join("snaps");
        let artifact_path = snapshots_dir.join("captured.db");
        let forged_destination = temp_dir.path().join("outside.db");
        fs::create_dir_all(&snapshots_dir).unwrap();
        write_db(&db_path, b"configured-database");
        write_db(&artifact_path, b"captured-database");
        write_db(&forged_destination, b"outside-data");
        let plugin = SqlitePlugin::new(db_path.clone(), snapshots_dir);
        let snapshot_id = SqlitePlugin::build_snapshot_id(&forged_destination, &artifact_path);

        plugin.rollback(&snapshot_id).await.unwrap();

        assert_eq!(fs::read(db_path).unwrap(), b"captured-database");
        assert_eq!(fs::read(forged_destination).unwrap(), b"outside-data");
    }

    #[tokio::test]
    async fn delete_rejects_forged_artifact_path_without_removing_it() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let snapshots_dir = temp_dir.path().join("snaps");
        let outside_path = temp_dir.path().join("outside.db");
        fs::create_dir_all(&snapshots_dir).unwrap();
        write_db(&outside_path, b"outside-artifact");
        let plugin = SqlitePlugin::new(db_path.clone(), snapshots_dir);
        let snapshot_id = SqlitePlugin::build_snapshot_id(&db_path, &outside_path);

        let err = plugin.delete(&snapshot_id).await.unwrap_err();

        assert!(matches!(
            err,
            SnapshotError::PathEscapesSnapshotStore {
                plugin: "sqlite",
                ..
            }
        ));
        assert_eq!(fs::read(outside_path).unwrap(), b"outside-artifact");
    }

    #[tokio::test]
    async fn delete_rejects_symlinked_artifact_outside_snapshot_store() {
        use std::os::unix::fs::symlink;

        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let snapshots_dir = temp_dir.path().join("snaps");
        let outside_path = temp_dir.path().join("outside.db");
        let artifact_path = snapshots_dir.join("captured.db");
        fs::create_dir_all(&snapshots_dir).unwrap();
        write_db(&outside_path, b"outside-artifact");
        symlink(&outside_path, &artifact_path).unwrap();
        let plugin = SqlitePlugin::new(db_path.clone(), snapshots_dir);
        let snapshot_id = SqlitePlugin::build_snapshot_id(&db_path, &artifact_path);

        let err = plugin.delete(&snapshot_id).await.unwrap_err();

        assert!(matches!(
            err,
            SnapshotError::PathEscapesSnapshotStore {
                plugin: "sqlite",
                ..
            }
        ));
        assert_eq!(fs::read(outside_path).unwrap(), b"outside-artifact");
    }

    #[tokio::test]
    async fn delete_rejects_parent_traversal_artifact_path() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let snapshots_dir = temp_dir.path().join("snaps");
        let traversal_path = snapshots_dir.join("..").join("outside.db");
        fs::create_dir_all(&snapshots_dir).unwrap();
        let plugin = SqlitePlugin::new(db_path.clone(), snapshots_dir);
        let snapshot_id = SqlitePlugin::build_snapshot_id(&db_path, &traversal_path);

        let err = plugin.delete(&snapshot_id).await.unwrap_err();

        assert!(matches!(
            err,
            SnapshotError::PathEscapesSnapshotStore {
                plugin: "sqlite",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn delete_rejects_sibling_prefix_artifact_path() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let snapshots_dir = temp_dir.path().join("snaps");
        let sibling_store = temp_dir.path().join("snaps-evil");
        let outside_path = sibling_store.join("captured.db");
        fs::create_dir_all(&snapshots_dir).unwrap();
        fs::create_dir_all(&sibling_store).unwrap();
        write_db(&outside_path, b"outside-artifact");
        let plugin = SqlitePlugin::new(db_path.clone(), snapshots_dir);
        let snapshot_id = SqlitePlugin::build_snapshot_id(&db_path, &outside_path);

        let err = plugin.delete(&snapshot_id).await.unwrap_err();

        assert!(matches!(
            err,
            SnapshotError::PathEscapesSnapshotStore {
                plugin: "sqlite",
                ..
            }
        ));
        assert_eq!(fs::read(outside_path).unwrap(), b"outside-artifact");
    }

    #[tokio::test]
    async fn delete_rejects_symlinked_artifact_parent_outside_snapshot_store() {
        use std::os::unix::fs::symlink;

        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let snapshots_dir = temp_dir.path().join("snaps");
        let outside_dir = temp_dir.path().join("outside");
        let artifact_path = snapshots_dir.join("nested").join("captured.db");
        fs::create_dir_all(&snapshots_dir).unwrap();
        fs::create_dir_all(&outside_dir).unwrap();
        symlink(&outside_dir, snapshots_dir.join("nested")).unwrap();
        let plugin = SqlitePlugin::new(db_path.clone(), snapshots_dir);
        let snapshot_id = SqlitePlugin::build_snapshot_id(&db_path, &artifact_path);

        let err = plugin.delete(&snapshot_id).await.unwrap_err();

        assert!(matches!(
            err,
            SnapshotError::PathEscapesSnapshotStore {
                plugin: "sqlite",
                ..
            }
        ));
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
        let parts: Vec<_> = snapshot_id.split(SEP).collect();
        let dump_path = PathBuf::from(decode_hex(parts[2]));
        fs::remove_file(&dump_path).unwrap();

        let err = plugin.rollback(&snapshot_id).await.unwrap_err();

        match err {
            SnapshotError::RollbackDumpNotFound { path } => {
                assert_eq!(path, dump_path.to_string_lossy())
            }
            other => panic!("expected RollbackDumpNotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rollback_handles_tabs_in_snapshot_paths() {
        let temp_dir = TempDir::new().unwrap();
        let db_dir = temp_dir.path().join("db\troot");
        let snapshots_dir = temp_dir.path().join("snap\ts");
        fs::create_dir_all(&db_dir).unwrap();
        let db_path = db_dir.join("app.db");
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
    async fn rollback_accepts_legacy_snapshot_ids() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("app.db");
        let dump_path = temp_dir.path().join("snaps").join("legacy.db");
        fs::create_dir_all(dump_path.parent().unwrap()).unwrap();
        write_db(&db_path, b"before-snapshot");
        write_db(&dump_path, b"legacy-snapshot");
        let plugin = SqlitePlugin::new(db_path.clone(), temp_dir.path().join("snaps"));
        let snapshot_id = format!("{}{SEP}{}", db_path.display(), dump_path.display());

        plugin.rollback(&snapshot_id).await.unwrap();

        assert_eq!(fs::read(&db_path).unwrap(), b"legacy-snapshot");
    }

    #[tokio::test]
    async fn rollback_errors_when_snapshot_id_is_malformed() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = SqlitePlugin::new(
            temp_dir.path().join("app.db"),
            temp_dir.path().join("snaps"),
        );

        let err = plugin
            .rollback("not-a-valid-snapshot-id")
            .await
            .unwrap_err();

        match err {
            SnapshotError::Snapshot(msg) => assert!(msg.contains("malformed snapshot_id")),
            other => panic!("expected malformed snapshot snapshot error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rollback_errors_when_path_encoding_is_malformed() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = SqlitePlugin::new(
            temp_dir.path().join("app.db"),
            temp_dir.path().join("snaps"),
        );

        let err = plugin
            .rollback("v2\txyz\t2f746d702f64756d702e6462")
            .await
            .unwrap_err();

        match err {
            SnapshotError::Snapshot(msg) => assert!(msg.contains("invalid original path encoding")),
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
        let first_parts: Vec<_> = first_id.split(SEP).collect();
        let first_dump = PathBuf::from(decode_hex(first_parts[2]));

        write_db(&db_path, b"after-first-snapshot");

        let second_id = plugin
            .snapshot(temp_dir.path(), "sqlite-command")
            .await
            .unwrap();
        let second_parts: Vec<_> = second_id.split(SEP).collect();
        let second_dump = PathBuf::from(decode_hex(second_parts[2]));

        assert_ne!(first_id, second_id);
        assert_ne!(first_dump, second_dump);
        assert_eq!(fs::read(first_dump).unwrap(), b"before-snapshot");
        assert_eq!(fs::read(second_dump).unwrap(), b"after-first-snapshot");
    }
}
