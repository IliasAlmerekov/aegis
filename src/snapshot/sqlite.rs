use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use crate::error::AegisError;
use crate::snapshot::SnapshotPlugin;

type Result<T> = std::result::Result<T, AegisError>;

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
            return Err(AegisError::Snapshot(format!(
                "malformed snapshot_id: invalid {label} encoding {encoded:?}"
            )));
        }

        let mut bytes = Vec::with_capacity(encoded.len() / 2);
        for pair in encoded.as_bytes().chunks_exact(2) {
            let hex = std::str::from_utf8(pair).map_err(|_| {
                AegisError::Snapshot(format!(
                    "malformed snapshot_id: invalid {label} encoding {encoded:?}"
                ))
            })?;
            let byte = u8::from_str_radix(hex, 16).map_err(|_| {
                AegisError::Snapshot(format!(
                    "malformed snapshot_id: invalid {label} encoding {encoded:?}"
                ))
            })?;
            bytes.push(byte);
        }

        #[cfg(unix)]
        let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));

        #[cfg(not(unix))]
        let path = PathBuf::from(String::from_utf8(bytes).map_err(|_| {
            AegisError::Snapshot(format!(
                "malformed snapshot_id: invalid {label} encoding {encoded:?}"
            ))
        })?);

        Self::validate_snapshot_path(&path, label)?;
        Ok(path)
    }

    fn validate_snapshot_path(path: &Path, label: &str) -> Result<()> {
        if !path.is_absolute()
            || path.file_name().is_none()
            || path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::CurDir | std::path::Component::ParentDir
                )
            })
        {
            return Err(AegisError::Snapshot(format!(
                "malformed snapshot_id: invalid {label} {path:?}"
            )));
        }

        Ok(())
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
            return Err(AegisError::Snapshot(format!(
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
            AegisError::Snapshot(format!("malformed snapshot_id: {snapshot_id:?}"))
        })?;
        let original_path = PathBuf::from(original_str);
        let dump_path = PathBuf::from(dump_str);
        Self::validate_snapshot_path(&original_path, "original path")?;
        Self::validate_snapshot_path(&dump_path, "dump path")?;
        Ok((original_path, dump_path))
    }
}

#[async_trait]
impl SnapshotPlugin for SqlitePlugin {
    fn name(&self) -> &'static str {
        "sqlite"
    }

    fn is_applicable(&self, cwd: &Path) -> bool {
        let db_path = self.resolve_db_path(cwd);
        !self.db_path.as_os_str().is_empty() && db_path.is_file()
    }

    async fn snapshot(&self, cwd: &Path, _cmd: &str) -> Result<String> {
        let db_path = self.resolve_db_path(cwd).canonicalize()?;
        fs::create_dir_all(&self.snapshots_dir)?;

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
        let mut dump_path = self.snapshots_dir.join(format!("{base_name}.db"));
        let mut suffix = 1usize;
        while dump_path.exists() {
            dump_path = self.snapshots_dir.join(format!("{base_name}-{suffix}.db"));
            suffix += 1;
        }

        fs::copy(&db_path, &dump_path)?;
        let dump_path = dump_path.canonicalize()?;

        let snapshot_id = Self::build_snapshot_id(&db_path, &dump_path);
        tracing::info!(%snapshot_id, "sqlite snapshot created");
        Ok(snapshot_id)
    }

    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        let (original_path, dump_path) = Self::parse_snapshot_id(snapshot_id)?;
        if !dump_path.exists() {
            return Err(AegisError::RollbackDumpNotFound {
                path: dump_path.to_string_lossy().to_string(),
            });
        }

        fs::copy(dump_path, original_path)?;
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
        let parts: Vec<_> = snapshot_id.split(SEP).collect();

        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "v2");
        assert_eq!(decode_hex(parts[1]), db_path.to_string_lossy());
        let dump = PathBuf::from(decode_hex(parts[2]));
        assert_eq!(dump.parent().unwrap(), snapshots_dir.as_path());
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
        let parts: Vec<_> = snapshot_id.split(SEP).collect();
        let dump_path = PathBuf::from(decode_hex(parts[2]));
        fs::remove_file(&dump_path).unwrap();

        let err = plugin.rollback(&snapshot_id).await.unwrap_err();

        match err {
            AegisError::RollbackDumpNotFound { path } => {
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
            AegisError::Snapshot(msg) => assert!(msg.contains("malformed snapshot_id")),
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
            AegisError::Snapshot(msg) => assert!(msg.contains("invalid original path encoding")),
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
