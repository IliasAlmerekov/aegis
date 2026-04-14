use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::AegisError;
use crate::snapshot::SnapshotPlugin;

type Result<T> = std::result::Result<T, AegisError>;

const SEP: char = '\t';

/// Snapshot plugin for MySQL databases.
pub struct MysqlPlugin {
    database: String,
    host: String,
    port: u16,
    user: String,
    snapshots_dir: PathBuf,
    mysqldump_bin: String,
    mysql_bin: String,
}

impl MysqlPlugin {
    /// Create a new MySQL snapshot plugin.
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
            mysqldump_bin: "mysqldump".to_string(),
            mysql_bin: "mysql".to_string(),
        }
    }

    fn build_common_args(&self) -> Vec<String> {
        let mut args = vec![
            format!("--host={}", self.host),
            format!("--port={}", self.port),
        ];

        if !self.user.is_empty() {
            args.push(format!("--user={}", self.user));
        }

        args
    }

    fn dump_path_candidate(&self, timestamp: u64, suffix: Option<usize>) -> PathBuf {
        let base_name = format!("mysql-{}-{timestamp}", self.sanitized_database_label());
        let file_name = match suffix {
            Some(suffix) => format!("{base_name}-{suffix}.sql"),
            None => format!("{base_name}.sql"),
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

    fn sanitized_database_label(&self) -> String {
        self.database
            .chars()
            .map(|ch| match ch {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
                _ => '_',
            })
            .collect()
    }

    fn encode_database(database: &str) -> String {
        database
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn decode_database(encoded: &str) -> Result<String> {
        if !encoded.len().is_multiple_of(2) {
            return Err(AegisError::Snapshot(format!(
                "malformed snapshot_id: invalid database encoding {encoded:?}"
            )));
        }

        let mut bytes = Vec::with_capacity(encoded.len() / 2);
        for pair in encoded.as_bytes().chunks_exact(2) {
            let hex = std::str::from_utf8(pair).map_err(|_| {
                AegisError::Snapshot(format!(
                    "malformed snapshot_id: invalid database encoding {encoded:?}"
                ))
            })?;
            let byte = u8::from_str_radix(hex, 16).map_err(|_| {
                AegisError::Snapshot(format!(
                    "malformed snapshot_id: invalid database encoding {encoded:?}"
                ))
            })?;
            bytes.push(byte);
        }

        String::from_utf8(bytes).map_err(|_| {
            AegisError::Snapshot(format!(
                "malformed snapshot_id: invalid database encoding {encoded:?}"
            ))
        })
    }
}

#[async_trait]
impl SnapshotPlugin for MysqlPlugin {
    fn name(&self) -> &'static str {
        "mysql"
    }

    fn is_applicable(&self, _cwd: &Path) -> bool {
        if self.database.is_empty() {
            return false;
        }

        std::process::Command::new("which")
            .arg(&self.mysqldump_bin)
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
            "--databases".to_string(),
            self.database.clone(),
            "--add-drop-database".to_string(),
        ]);

        let output = Command::new(&self.mysqldump_bin)
            .args(&args)
            .output()
            .await
            .map_err(|e| AegisError::Snapshot(format!("failed to run mysqldump: {e}")))?;

        if !output.status.success() {
            let _ = std::fs::remove_file(&dump_path);
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AegisError::Snapshot(format!("mysqldump failed: {stderr}")));
        }

        if let Err(err) = std::fs::write(&dump_path, &output.stdout) {
            let _ = std::fs::remove_file(&dump_path);
            return Err(err.into());
        }

        let snapshot_id = format!(
            "{}{SEP}{}",
            Self::encode_database(&self.database),
            dump_path.display()
        );
        tracing::info!(%snapshot_id, "mysql snapshot created");
        Ok(snapshot_id)
    }

    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        let (database_encoded, dump_str) = snapshot_id.split_once(SEP).ok_or_else(|| {
            AegisError::Snapshot(format!("malformed snapshot_id: {snapshot_id:?}"))
        })?;
        let _database = Self::decode_database(database_encoded)?;

        let dump_path = Path::new(dump_str);
        if !dump_path.exists() {
            return Err(AegisError::RollbackDumpNotFound {
                path: dump_str.to_string(),
            });
        }

        let dump_bytes = std::fs::read(dump_path)?;

        let args = self.build_common_args();
        let mut child = Command::new(&self.mysql_bin)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| AegisError::Snapshot(format!("failed to run mysql: {e}")))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AegisError::Snapshot("failed to open mysql stdin".to_string()))?;
        stdin
            .write_all(&dump_bytes)
            .await
            .map_err(|e| AegisError::Snapshot(format!("failed to write mysql stdin: {e}")))?;
        drop(stdin);

        let output = child
            .wait_with_output()
            .await
            .map_err(|e| AegisError::Snapshot(format!("failed to wait for mysql: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AegisError::Snapshot(format!("mysql failed: {stderr}")));
        }

        tracing::info!(snapshot_id = snapshot_id, "mysql snapshot rolled back");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn plugin_with_user(temp_dir: &TempDir, user: &str) -> MysqlPlugin {
        MysqlPlugin::new(
            "app".to_string(),
            "localhost".to_string(),
            3306,
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
        let plugin = MysqlPlugin::new(
            String::new(),
            "localhost".to_string(),
            3306,
            "root".to_string(),
            temp_dir.path().join("snaps"),
        );

        assert!(!plugin.is_applicable(temp_dir.path()));
    }

    #[test]
    fn build_common_args_includes_host_port() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "");

        assert_eq!(
            plugin.build_common_args(),
            vec!["--host=localhost".to_string(), "--port=3306".to_string(),]
        );
    }

    #[test]
    fn build_common_args_includes_user_when_set() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "app_user");
        let args = plugin.build_common_args();

        assert!(args.iter().any(|arg| arg == "--user=app_user"));
    }

    #[test]
    fn build_common_args_omits_user_when_empty() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "");

        assert_eq!(
            plugin.build_common_args(),
            vec!["--host=localhost".to_string(), "--port=3306".to_string(),]
        );
    }

    #[test]
    fn reserve_dump_path_creates_unique_files_atomically() {
        let temp_dir = TempDir::new().unwrap();
        let snapshots_dir = temp_dir.path().join("snaps");
        fs::create_dir_all(&snapshots_dir).unwrap();
        let plugin = plugin_with_user(&temp_dir, "root");

        let first = plugin.reserve_dump_path(1_234).unwrap();
        let second = plugin.reserve_dump_path(1_234).unwrap();

        assert_ne!(first, second);
        assert!(first.exists());
        assert!(second.exists());
        assert_eq!(
            first.file_name().unwrap().to_string_lossy(),
            "mysql-app-1234.sql"
        );
        assert_eq!(
            second.file_name().unwrap().to_string_lossy(),
            "mysql-app-1234-1.sql"
        );
    }

    #[test]
    fn reserve_dump_path_sanitizes_database_name_for_filenames() {
        let temp_dir = TempDir::new().unwrap();
        let snapshots_dir = temp_dir.path().join("snaps");
        fs::create_dir_all(&snapshots_dir).unwrap();
        let plugin = MysqlPlugin::new(
            "app/\tname:prod".to_string(),
            "localhost".to_string(),
            3306,
            "root".to_string(),
            snapshots_dir,
        );

        let dump_path = plugin.reserve_dump_path(1_234).unwrap();

        assert_eq!(
            dump_path.file_name().unwrap().to_string_lossy(),
            "mysql-app__name_prod-1234.sql"
        );
    }

    #[test]
    fn database_name_encoding_round_trips_for_snapshot_ids() {
        let database = "app/\tname:prod";
        let encoded = MysqlPlugin::encode_database(database);
        let snapshot_id = format!("{encoded}{SEP}/tmp/example.sql");
        let (encoded_database, dump_path) = snapshot_id.split_once(SEP).unwrap();

        assert_eq!(
            MysqlPlugin::decode_database(encoded_database).unwrap(),
            database
        );
        assert_eq!(dump_path, "/tmp/example.sql");
    }

    #[tokio::test]
    async fn rollback_errors_when_dump_file_missing() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "root");
        let missing_dump = temp_dir.path().join("missing.sql");
        let snapshot_id = format!(
            "{}{SEP}{}",
            MysqlPlugin::encode_database("app"),
            missing_dump.display()
        );

        let err = plugin.rollback(&snapshot_id).await.unwrap_err();

        match err {
            AegisError::RollbackDumpNotFound { path } => {
                assert_eq!(path, missing_dump.to_string_lossy())
            }
            other => panic!("expected RollbackDumpNotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rollback_errors_on_malformed_id() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "root");

        let err = plugin
            .rollback("not-a-valid-snapshot-id")
            .await
            .unwrap_err();

        match err {
            AegisError::Snapshot(msg) => assert!(msg.contains("malformed snapshot_id")),
            other => panic!("expected malformed snapshot error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rollback_errors_on_malformed_database_encoding() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "root");

        let err = plugin.rollback("xyz\t/tmp/example.sql").await.unwrap_err();

        match err {
            AegisError::Snapshot(msg) => assert!(msg.contains("invalid database encoding")),
            other => panic!("expected malformed snapshot error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn snapshot_uses_mysqldump_and_creates_dump_file() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("mysqldump.args");
        let mysqldump = stub_bin(
            &temp_dir,
            "mysqldump",
            &format!(
                "log='{}'\n: > \"$log\"\nfor arg in \"$@\"; do\n  printf '%s\\n' \"$arg\" >> \"$log\"\ndone\nprintf 'dump-data'",
                log_path.display()
            ),
        );
        let mut plugin = plugin_with_user(&temp_dir, "root");
        plugin.mysqldump_bin = mysqldump.display().to_string();

        let snapshot_id = plugin
            .snapshot(temp_dir.path(), "dangerous command")
            .await
            .unwrap();
        let (database, dump_path) = snapshot_id.split_once(SEP).unwrap();
        let logged_args = fs::read_to_string(&log_path).unwrap();

        assert_eq!(MysqlPlugin::decode_database(database).unwrap(), "app");
        assert!(logged_args.lines().any(|line| line == "--databases"));
        assert!(
            logged_args
                .lines()
                .any(|line| line == "--add-drop-database")
        );
        assert!(logged_args.lines().any(|line| line == "app"));
        assert_eq!(fs::read_to_string(dump_path).unwrap(), "dump-data");
    }

    #[tokio::test]
    async fn snapshot_returns_stderr_when_mysqldump_fails() {
        let temp_dir = TempDir::new().unwrap();
        let mysqldump = stub_bin(
            &temp_dir,
            "mysqldump",
            "printf 'mysqldump exploded' >&2\nexit 12",
        );
        let mut plugin = plugin_with_user(&temp_dir, "root");
        plugin.mysqldump_bin = mysqldump.display().to_string();

        let err = plugin
            .snapshot(temp_dir.path(), "dangerous command")
            .await
            .unwrap_err();

        match err {
            AegisError::Snapshot(msg) => {
                assert!(msg.contains("mysqldump failed"));
                assert!(msg.contains("mysqldump exploded"));
            }
            other => panic!("expected snapshot error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rollback_uses_mysql_with_expected_arguments_and_dump_on_stdin() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("mysql.args");
        let stdin_path = temp_dir.path().join("mysql.stdin");
        let dump_path = temp_dir.path().join("snaps").join("existing.sql");
        fs::create_dir_all(dump_path.parent().unwrap()).unwrap();
        fs::write(&dump_path, "dump-data").unwrap();
        let mysql = stub_bin(
            &temp_dir,
            "mysql",
            &format!(
                "log='{}'\nstdin_file='{}'\n: > \"$log\"\nfor arg in \"$@\"; do\n  printf '%s\\n' \"$arg\" >> \"$log\"\ndone\ncat > \"$stdin_file\"",
                log_path.display(),
                stdin_path.display()
            ),
        );
        let mut plugin = plugin_with_user(&temp_dir, "root");
        plugin.mysql_bin = mysql.display().to_string();
        let snapshot_id = format!(
            "{}{SEP}{}",
            MysqlPlugin::encode_database("app"),
            dump_path.display()
        );

        plugin.rollback(&snapshot_id).await.unwrap();

        let logged_args = fs::read_to_string(&log_path).unwrap();
        assert!(logged_args.lines().any(|line| line == "--host=localhost"));
        assert!(logged_args.lines().any(|line| line == "--port=3306"));
        assert!(logged_args.lines().any(|line| line == "--user=root"));
        assert!(!logged_args.lines().any(|line| line == "app"));
        assert_eq!(fs::read_to_string(&stdin_path).unwrap(), "dump-data");
    }

    #[tokio::test]
    async fn rollback_returns_stderr_when_mysql_fails() {
        let temp_dir = TempDir::new().unwrap();
        let dump_path = temp_dir.path().join("snaps").join("existing.sql");
        fs::create_dir_all(dump_path.parent().unwrap()).unwrap();
        fs::write(&dump_path, "dump-data").unwrap();
        let mysql = stub_bin(&temp_dir, "mysql", "printf 'mysql exploded' >&2\nexit 23");
        let mut plugin = plugin_with_user(&temp_dir, "root");
        plugin.mysql_bin = mysql.display().to_string();
        let snapshot_id = format!(
            "{}{SEP}{}",
            MysqlPlugin::encode_database("app"),
            dump_path.display()
        );

        let err = plugin.rollback(&snapshot_id).await.unwrap_err();

        match err {
            AegisError::Snapshot(msg) => {
                assert!(msg.contains("mysql failed"));
                assert!(msg.contains("mysql exploded"));
            }
            other => panic!("expected snapshot error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn snapshot_generates_distinct_ids_for_back_to_back_calls() {
        let temp_dir = TempDir::new().unwrap();
        let mysqldump = stub_bin(&temp_dir, "mysqldump", "printf 'dump-data'");
        let mut plugin = plugin_with_user(&temp_dir, "root");
        plugin.mysqldump_bin = mysqldump.display().to_string();

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
