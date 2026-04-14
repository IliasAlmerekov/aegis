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
        let dump_path = self
            .snapshots_dir
            .join(format!("pg-{}-{timestamp}.dump", self.database));

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
}
