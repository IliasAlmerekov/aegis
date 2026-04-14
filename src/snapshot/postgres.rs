use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::process::Command;

use crate::error::AegisError;
use crate::snapshot::SnapshotPlugin;

type Result<T> = std::result::Result<T, AegisError>;

const SEP: char = '\t';

/// Snapshot plugin for PostgreSQL databases.
pub struct PostgresPlugin {
    database: String,
    host: String,
    port: u16,
    user: String,
    snapshots_dir: PathBuf,
    pg_dump_bin: String,
    pg_restore_bin: String,
}

impl PostgresPlugin {
    /// Create a new PostgreSQL snapshot plugin.
    pub fn new(
        database: String,
        host: String,
        port: u16,
        user: String,
        snapshots_dir: PathBuf,
    ) -> Self {
        Self {
            database,
            host,
            port,
            user,
            snapshots_dir,
            pg_dump_bin: "pg_dump".to_string(),
            pg_restore_bin: "pg_restore".to_string(),
        }
    }

    fn build_common_args(&self) -> Vec<String> {
        let mut args = vec![
            "-h".to_string(),
            self.host.clone(),
            "-p".to_string(),
            self.port.to_string(),
        ];

        if !self.user.is_empty() {
            args.push("-U".to_string());
            args.push(self.user.clone());
        }

        args
    }

    fn dump_path_candidate(&self, timestamp: u64, suffix: Option<usize>) -> PathBuf {
        let base_name = format!("pg-{}-{timestamp}", self.database);
        let file_name = match suffix {
            Some(suffix) => format!("{base_name}-{suffix}.dump"),
            None => format!("{base_name}.dump"),
        };

        self.snapshots_dir.join(file_name)
    }

    fn reserve_dump_path(&self, timestamp: u64) -> Result<PathBuf> {
        let mut suffix = None;

        loop {
            let dump_path = self.dump_path_candidate(timestamp, suffix);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&dump_path)
            {
                Ok(file) => {
                    drop(file);
                    return Ok(dump_path);
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    suffix = Some(suffix.map_or(1, |current| current + 1));
                }
                Err(err) => return Err(err.into()),
            }
        }
    }
}

#[async_trait]
impl SnapshotPlugin for PostgresPlugin {
    fn name(&self) -> &'static str {
        "postgres"
    }

    fn is_applicable(&self, _cwd: &Path) -> bool {
        if self.database.is_empty() {
            return false;
        }

        std::process::Command::new("which")
            .arg(&self.pg_dump_bin)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    async fn snapshot(&self, _cwd: &Path, _cmd: &str) -> Result<String> {
        std::fs::create_dir_all(&self.snapshots_dir)?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let dump_path = self.reserve_dump_path(timestamp)?;

        let mut args = self.build_common_args();
        args.extend([
            "-Fc".to_string(),
            "-f".to_string(),
            dump_path.display().to_string(),
            self.database.clone(),
        ]);

        let output = Command::new(&self.pg_dump_bin)
            .args(&args)
            .output()
            .await
            .map_err(|e| AegisError::Snapshot(format!("failed to run pg_dump: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AegisError::Snapshot(format!("pg_dump failed: {stderr}")));
        }

        let snapshot_id = format!("{}{SEP}{}", self.database, dump_path.display());
        tracing::info!(%snapshot_id, "postgres snapshot created");
        Ok(snapshot_id)
    }

    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        let (database, dump_str) = snapshot_id.split_once(SEP).ok_or_else(|| {
            AegisError::Snapshot(format!("malformed snapshot_id: {snapshot_id:?}"))
        })?;

        let dump_path = Path::new(dump_str);
        if !dump_path.exists() {
            return Err(AegisError::RollbackDumpNotFound {
                path: dump_str.to_string(),
            });
        }

        let mut args = self.build_common_args();
        args.extend([
            "--clean".to_string(),
            "--if-exists".to_string(),
            "-d".to_string(),
            database.to_string(),
            dump_str.to_string(),
        ]);

        let output = Command::new(&self.pg_restore_bin)
            .args(&args)
            .output()
            .await
            .map_err(|e| AegisError::Snapshot(format!("failed to run pg_restore: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AegisError::Snapshot(format!("pg_restore failed: {stderr}")));
        }

        tracing::info!(snapshot_id = snapshot_id, "postgres snapshot rolled back");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn plugin_with_user(temp_dir: &TempDir, user: &str) -> PostgresPlugin {
        PostgresPlugin::new(
            "app".to_string(),
            "localhost".to_string(),
            5432,
            user.to_string(),
            temp_dir.path().join("snaps"),
        )
    }

    fn stub_bin(dir: &TempDir, name: &str, body: &str) -> PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    #[tokio::test]
    async fn is_not_applicable_when_database_empty() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = PostgresPlugin::new(
            String::new(),
            "localhost".to_string(),
            5432,
            "postgres".to_string(),
            temp_dir.path().join("snaps"),
        );

        assert!(!plugin.is_applicable(temp_dir.path()));
    }

    #[test]
    fn build_common_args_includes_host_and_port() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "postgres");

        assert_eq!(
            plugin.build_common_args(),
            vec![
                "-h".to_string(),
                "localhost".to_string(),
                "-p".to_string(),
                "5432".to_string(),
                "-U".to_string(),
                "postgres".to_string(),
            ]
        );
    }

    #[test]
    fn build_common_args_includes_user_when_set() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "app_user");
        let args = plugin.build_common_args();

        assert!(args.windows(2).any(|pair| pair == ["-U", "app_user"]));
    }

    #[test]
    fn build_common_args_omits_user_when_empty() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "");

        assert_eq!(
            plugin.build_common_args(),
            vec![
                "-h".to_string(),
                "localhost".to_string(),
                "-p".to_string(),
                "5432".to_string(),
            ]
        );
    }

    #[test]
    fn reserve_dump_path_creates_unique_files_atomically() {
        let temp_dir = TempDir::new().unwrap();
        let snapshots_dir = temp_dir.path().join("snaps");
        fs::create_dir_all(&snapshots_dir).unwrap();
        let plugin = plugin_with_user(&temp_dir, "postgres");

        let first = plugin.reserve_dump_path(1_234).unwrap();
        let second = plugin.reserve_dump_path(1_234).unwrap();

        assert_ne!(first, second);
        assert!(first.exists());
        assert!(second.exists());
        assert_eq!(
            first.file_name().unwrap().to_string_lossy(),
            "pg-app-1234.dump"
        );
        assert_eq!(
            second.file_name().unwrap().to_string_lossy(),
            "pg-app-1234-1.dump"
        );
    }

    #[tokio::test]
    async fn rollback_errors_when_dump_file_missing() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "postgres");
        let missing_dump = temp_dir.path().join("missing.dump");
        let snapshot_id = format!("app{SEP}{}", missing_dump.display());

        let err = plugin.rollback(&snapshot_id).await.unwrap_err();

        match err {
            AegisError::RollbackDumpNotFound { path } => {
                assert_eq!(path, missing_dump.to_string_lossy())
            }
            other => panic!("expected RollbackDumpNotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rollback_errors_on_malformed_snapshot_id() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "postgres");

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
    async fn snapshot_uses_pg_dump_and_creates_dump_file() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("pg_dump.args");
        let pg_dump = stub_bin(
            &temp_dir,
            "pg_dump",
            &format!(
                "log='{}'\nout=''\nprev=''\n: > \"$log\"\nfor arg in \"$@\"; do\n  printf '%s\\n' \"$arg\" >> \"$log\"\n  if [ \"$prev\" = '-f' ]; then out=\"$arg\"; fi\n  prev=\"$arg\"\ndone\nprintf 'dump-data' > \"$out\"",
                log_path.display()
            ),
        );
        let mut plugin = plugin_with_user(&temp_dir, "postgres");
        plugin.pg_dump_bin = pg_dump.display().to_string();

        let snapshot_id = plugin
            .snapshot(temp_dir.path(), "dangerous command")
            .await
            .unwrap();
        let (database, dump_path) = snapshot_id.split_once(SEP).unwrap();
        let logged_args = fs::read_to_string(&log_path).unwrap();

        assert_eq!(database, "app");
        assert!(logged_args.lines().any(|line| line == "-Fc"));
        assert!(logged_args.lines().any(|line| line == "-f"));
        assert!(logged_args.lines().any(|line| line == "app"));
        assert_eq!(fs::read_to_string(dump_path).unwrap(), "dump-data");
    }

    #[tokio::test]
    async fn snapshot_returns_stderr_when_pg_dump_fails() {
        let temp_dir = TempDir::new().unwrap();
        let pg_dump = stub_bin(
            &temp_dir,
            "pg_dump",
            "printf 'pg_dump exploded' >&2\nexit 12",
        );
        let mut plugin = plugin_with_user(&temp_dir, "postgres");
        plugin.pg_dump_bin = pg_dump.display().to_string();

        let err = plugin
            .snapshot(temp_dir.path(), "dangerous command")
            .await
            .unwrap_err();

        match err {
            AegisError::Snapshot(msg) => {
                assert!(msg.contains("pg_dump failed"));
                assert!(msg.contains("pg_dump exploded"));
            }
            other => panic!("expected snapshot error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rollback_uses_pg_restore_with_expected_arguments() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("pg_restore.args");
        let dump_path = temp_dir.path().join("snaps").join("existing.dump");
        fs::create_dir_all(dump_path.parent().unwrap()).unwrap();
        fs::write(&dump_path, "dump-data").unwrap();
        let pg_restore = stub_bin(
            &temp_dir,
            "pg_restore",
            &format!(
                "log='{}'\n: > \"$log\"\nfor arg in \"$@\"; do\n  printf '%s\\n' \"$arg\" >> \"$log\"\ndone",
                log_path.display()
            ),
        );
        let mut plugin = plugin_with_user(&temp_dir, "postgres");
        plugin.pg_restore_bin = pg_restore.display().to_string();
        let snapshot_id = format!("app{SEP}{}", dump_path.display());

        plugin.rollback(&snapshot_id).await.unwrap();

        let logged_args = fs::read_to_string(&log_path).unwrap();
        assert!(logged_args.lines().any(|line| line == "--clean"));
        assert!(logged_args.lines().any(|line| line == "--if-exists"));
        assert!(logged_args.lines().any(|line| line == "-d"));
        assert!(logged_args.lines().any(|line| line == "app"));
        assert!(
            logged_args
                .lines()
                .any(|line| line == dump_path.to_string_lossy())
        );
    }

    #[tokio::test]
    async fn rollback_returns_stderr_when_pg_restore_fails() {
        let temp_dir = TempDir::new().unwrap();
        let dump_path = temp_dir.path().join("snaps").join("existing.dump");
        fs::create_dir_all(dump_path.parent().unwrap()).unwrap();
        fs::write(&dump_path, "dump-data").unwrap();
        let pg_restore = stub_bin(
            &temp_dir,
            "pg_restore",
            "printf 'pg_restore exploded' >&2\nexit 23",
        );
        let mut plugin = plugin_with_user(&temp_dir, "postgres");
        plugin.pg_restore_bin = pg_restore.display().to_string();
        let snapshot_id = format!("app{SEP}{}", dump_path.display());

        let err = plugin.rollback(&snapshot_id).await.unwrap_err();

        match err {
            AegisError::Snapshot(msg) => {
                assert!(msg.contains("pg_restore failed"));
                assert!(msg.contains("pg_restore exploded"));
            }
            other => panic!("expected snapshot error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn snapshot_generates_distinct_ids_for_back_to_back_calls() {
        let temp_dir = TempDir::new().unwrap();
        let pg_dump = stub_bin(
            &temp_dir,
            "pg_dump",
            "out=''\nprev=''\nfor arg in \"$@\"; do\n  if [ \"$prev\" = '-f' ]; then out=\"$arg\"; fi\n  prev=\"$arg\"\ndone\nprintf '%s' \"$out\" > \"$out\"",
        );
        let mut plugin = plugin_with_user(&temp_dir, "postgres");
        plugin.pg_dump_bin = pg_dump.display().to_string();

        let first_id = plugin
            .snapshot(temp_dir.path(), "dangerous command")
            .await
            .unwrap();
        let second_id = plugin
            .snapshot(temp_dir.path(), "dangerous command")
            .await
            .unwrap();
        let (_, first_dump) = first_id.split_once(SEP).unwrap();
        let (_, second_dump) = second_id.split_once(SEP).unwrap();

        assert_ne!(first_id, second_id);
        assert_ne!(first_dump, second_dump);
        assert!(Path::new(first_dump).exists());
        assert!(Path::new(second_dump).exists());
    }
}
