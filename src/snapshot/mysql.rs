use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::process::{ChildStderr, Command};
use tokio::task::JoinHandle;

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use crate::error::AegisError;
use crate::snapshot::SnapshotPlugin;

type Result<T> = std::result::Result<T, AegisError>;

const SEP: char = '\t';
const SNAPSHOT_ID_VERSION: &str = "v2";

struct MysqlRollbackTarget {
    host: String,
    port: u16,
    user: String,
    dump_path: PathBuf,
}

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
        Self::build_common_args_for_target(&self.host, self.port, &self.user)
    }

    fn build_common_args_for_target(host: &str, port: u16, user: &str) -> Vec<String> {
        let mut args = vec![format!("--host={host}"), format!("--port={port}")];

        if !user.is_empty() {
            args.push(format!("--user={user}"));
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

    fn encode_component(component: &str) -> String {
        component
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn decode_component(encoded: &str, label: &str) -> Result<String> {
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

        String::from_utf8(bytes).map_err(|_| {
            AegisError::Snapshot(format!(
                "malformed snapshot_id: invalid {label} encoding {encoded:?}"
            ))
        })
    }

    fn encode_database(database: &str) -> String {
        Self::encode_component(database)
    }

    fn decode_database(encoded: &str) -> Result<String> {
        Self::decode_component(encoded, "database")
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
            Self::encode_component(&path.to_string_lossy())
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

    fn build_snapshot_id(&self, dump_path: &Path) -> String {
        format!(
            "{SNAPSHOT_ID_VERSION}{SEP}{}{SEP}{}{SEP}{}{SEP}{}{SEP}{}",
            Self::encode_database(&self.database),
            Self::encode_component(&self.host),
            self.port,
            Self::encode_component(&self.user),
            Self::encode_path(dump_path)
        )
    }

    fn parse_snapshot_id(snapshot_id: &str) -> Result<MysqlRollbackTarget> {
        let parts: Vec<_> = snapshot_id.split(SEP).collect();
        if parts.len() != 6 || parts[0] != SNAPSHOT_ID_VERSION {
            return Err(AegisError::Snapshot(format!(
                "malformed snapshot_id: {snapshot_id:?}"
            )));
        }

        let _database = Self::decode_database(parts[1])?;
        let host = Self::decode_component(parts[2], "host")?;
        let port = parts[3].parse::<u16>().map_err(|_| {
            AegisError::Snapshot(format!(
                "malformed snapshot_id: invalid port {:?}",
                parts[3]
            ))
        })?;
        let user = Self::decode_component(parts[4], "user")?;
        let dump_path = Self::decode_path(parts[5], "dump path")?;

        Ok(MysqlRollbackTarget {
            host,
            port,
            user,
            dump_path,
        })
    }

    async fn kill_and_reap_child(child: &mut tokio::process::Child) {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }

    fn spawn_stderr_drain(stderr: ChildStderr) -> JoinHandle<std::io::Result<Vec<u8>>> {
        tokio::spawn(async move {
            let mut stderr = stderr;
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).await?;
            Ok(bytes)
        })
    }

    async fn collect_stderr(
        stderr_task: JoinHandle<std::io::Result<Vec<u8>>>,
        command_name: &str,
    ) -> Result<Vec<u8>> {
        stderr_task
            .await
            .map_err(|err| {
                AegisError::Snapshot(format!("failed to join {command_name} stderr task: {err}"))
            })?
            .map_err(|err| {
                AegisError::Snapshot(format!("failed to read {command_name} stderr: {err}"))
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

        let mut child = Command::new(&self.mysqldump_bin)
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                let _ = std::fs::remove_file(&dump_path);
                AegisError::Snapshot(format!("failed to run mysqldump: {e}"))
            })?;

        let mut stdout = child.stdout.take().ok_or_else(|| {
            let _ = std::fs::remove_file(&dump_path);
            AegisError::Snapshot("failed to capture mysqldump stdout".to_string())
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            let _ = std::fs::remove_file(&dump_path);
            AegisError::Snapshot("failed to capture mysqldump stderr".to_string())
        })?;
        let stderr_task = Self::spawn_stderr_drain(stderr);
        let mut dump_file = tokio::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&dump_path)
            .await?;
        if let Err(err) = io::copy(&mut stdout, &mut dump_file).await {
            Self::kill_and_reap_child(&mut child).await;
            let _ = Self::collect_stderr(stderr_task, "mysqldump").await;
            let _ = std::fs::remove_file(&dump_path);
            return Err(AegisError::Snapshot(format!(
                "failed to stream mysqldump output: {err}"
            )));
        }
        drop(stdout);
        dump_file.flush().await?;
        drop(dump_file);

        let status = child
            .wait()
            .await
            .map_err(|e| AegisError::Snapshot(format!("failed to wait for mysqldump: {e}")))?;
        let stderr = Self::collect_stderr(stderr_task, "mysqldump").await?;

        if !status.success() {
            let _ = std::fs::remove_file(&dump_path);
            let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
            return Err(AegisError::Snapshot(format!("mysqldump failed: {stderr}")));
        }

        let dump_path = dump_path.canonicalize()?;
        let snapshot_id = self.build_snapshot_id(&dump_path);
        tracing::info!(%snapshot_id, "mysql snapshot created");
        Ok(snapshot_id)
    }

    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        let target = Self::parse_snapshot_id(snapshot_id)?;
        if !target.dump_path.exists() {
            return Err(AegisError::RollbackDumpNotFound {
                path: target.dump_path.to_string_lossy().to_string(),
            });
        }

        let args = Self::build_common_args_for_target(&target.host, target.port, &target.user);
        let mut child = Command::new(&self.mysql_bin)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| AegisError::Snapshot(format!("failed to run mysql: {e}")))?;

        let mut dump_file = tokio::fs::File::open(&target.dump_path).await?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| AegisError::Snapshot("failed to capture mysql stderr".to_string()))?;
        let stderr_task = Self::spawn_stderr_drain(stderr);
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AegisError::Snapshot("failed to open mysql stdin".to_string()))?;
        if let Err(err) = io::copy(&mut dump_file, &mut stdin).await {
            drop(stdin);
            Self::kill_and_reap_child(&mut child).await;
            let _ = Self::collect_stderr(stderr_task, "mysql").await;
            return Err(AegisError::Snapshot(format!(
                "failed to write mysql stdin: {err}"
            )));
        }
        drop(stdin);

        let status = child
            .wait()
            .await
            .map_err(|e| AegisError::Snapshot(format!("failed to wait for mysql: {e}")))?;
        let stderr = Self::collect_stderr(stderr_task, "mysql").await?;

        if !status.success() {
            let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
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

    fn hex_encode(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn encode_str(value: &str) -> String {
        hex_encode(value.as_bytes())
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

    fn encode_path(path: &Path) -> String {
        encode_str(&path.to_string_lossy())
    }

    fn snapshot_id_for(
        database: &str,
        host: &str,
        port: u16,
        user: &str,
        dump_path: &Path,
    ) -> String {
        format!(
            "v2{SEP}{}{SEP}{}{SEP}{}{SEP}{}{SEP}{}",
            encode_str(database),
            encode_str(host),
            port,
            encode_str(user),
            encode_path(dump_path)
        )
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
    fn snapshot_id_v2_round_trips_target_fields_and_dump_path() {
        let dump_path = Path::new("/tmp/mysql\tbackup.sql");
        let snapshot_id = snapshot_id_for(
            "app/\tname:prod",
            "db\tprimary",
            3_307,
            "root\tadmin",
            dump_path,
        );
        let parts: Vec<_> = snapshot_id.split(SEP).collect();

        assert_eq!(parts.len(), 6);
        assert_eq!(parts[0], "v2");
        assert_eq!(decode_hex(parts[1]), "app/\tname:prod");
        assert_eq!(decode_hex(parts[2]), "db\tprimary");
        assert_eq!(parts[3], "3307");
        assert_eq!(decode_hex(parts[4]), "root\tadmin");
        assert_eq!(PathBuf::from(decode_hex(parts[5])), dump_path);
    }

    #[tokio::test]
    async fn rollback_errors_on_malformed_dump_path_encoding() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "root");

        let err = plugin
            .rollback("v2\t617070\t6c6f63616c686f7374\t3306\t726f6f74\txyz")
            .await
            .unwrap_err();

        match err {
            AegisError::Snapshot(msg) => assert!(msg.contains("invalid dump path encoding")),
            other => panic!("expected malformed snapshot error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rollback_errors_when_dump_file_missing() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "root");
        let missing_dump = plugin.snapshots_dir.join("missing.sql");
        let snapshot_id = snapshot_id_for("app", "localhost", 3_306, "root", &missing_dump);

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

        let err = plugin
            .rollback(
                "v2\txyz\t6c6f63616c686f7374\t3306\t726f6f74\t2f746d702f6578616d706c652e73716c",
            )
            .await
            .unwrap_err();

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
        let parts: Vec<_> = snapshot_id.split(SEP).collect();
        let logged_args = fs::read_to_string(&log_path).unwrap();

        assert_eq!(parts.len(), 6);
        assert_eq!(parts[0], "v2");
        assert_eq!(decode_hex(parts[1]), "app");
        assert_eq!(decode_hex(parts[2]), "localhost");
        assert_eq!(parts[3], "3306");
        assert_eq!(decode_hex(parts[4]), "root");
        assert!(logged_args.lines().any(|line| line == "--databases"));
        assert!(
            logged_args
                .lines()
                .any(|line| line == "--add-drop-database")
        );
        assert!(logged_args.lines().any(|line| line == "app"));
        let dump_path = PathBuf::from(decode_hex(parts[5]));
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
    async fn snapshot_removes_reserved_dump_when_mysqldump_spawn_fails() {
        let temp_dir = TempDir::new().unwrap();
        let snapshots_dir = temp_dir.path().join("snaps");
        let mut plugin = plugin_with_user(&temp_dir, "root");
        plugin.mysqldump_bin = temp_dir
            .path()
            .join("missing-mysqldump")
            .display()
            .to_string();

        let err = plugin
            .snapshot(temp_dir.path(), "dangerous command")
            .await
            .unwrap_err();

        match err {
            AegisError::Snapshot(msg) => assert!(msg.contains("failed to run mysqldump")),
            other => panic!("expected snapshot error, got {other:?}"),
        }
        assert!(snapshots_dir.exists());
        assert!(fs::read_dir(&snapshots_dir).unwrap().next().is_none());
    }

    #[tokio::test]
    async fn snapshot_drains_large_stderr_without_deadlocking() {
        let temp_dir = TempDir::new().unwrap();
        let mysqldump = stub_bin(
            &temp_dir,
            "mysqldump",
            "i=0\nwhile [ \"$i\" -lt 5000 ]; do\n  printf '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\\n' >&2\n  i=$((i + 1))\ndone\nprintf 'dump-data'",
        );
        let mut plugin = plugin_with_user(&temp_dir, "root");
        plugin.mysqldump_bin = mysqldump.display().to_string();

        let snapshot_id = plugin
            .snapshot(temp_dir.path(), "dangerous command")
            .await
            .unwrap();

        let parts: Vec<_> = snapshot_id.split(SEP).collect();
        let dump_path = PathBuf::from(decode_hex(parts[5]));
        assert_eq!(fs::read_to_string(dump_path).unwrap(), "dump-data");
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
        let snapshot_id = snapshot_id_for("app", "localhost", 3_306, "root", &dump_path);

        plugin.rollback(&snapshot_id).await.unwrap();

        let logged_args = fs::read_to_string(&log_path).unwrap();
        assert!(logged_args.lines().any(|line| line == "--host=localhost"));
        assert!(logged_args.lines().any(|line| line == "--port=3306"));
        assert!(logged_args.lines().any(|line| line == "--user=root"));
        assert!(!logged_args.lines().any(|line| line == "app"));
        assert_eq!(fs::read_to_string(&stdin_path).unwrap(), "dump-data");
    }

    #[tokio::test]
    async fn rollback_uses_snapshot_time_target_and_dump_path_instead_of_current_config() {
        let temp_dir = TempDir::new().unwrap();
        let stdin_path = temp_dir.path().join("mysql.stdin");
        let restore_log_path = temp_dir.path().join("mysql.args");
        let mysqldump = stub_bin(&temp_dir, "mysqldump", "printf 'dump-data'");
        let mysql = stub_bin(
            &temp_dir,
            "mysql",
            &format!(
                "log='{}'\nstdin_file='{}'\n: > \"$log\"\nfor arg in \"$@\"; do\n  printf '%s\\n' \"$arg\" >> \"$log\"\ndone\ncat > \"$stdin_file\"",
                restore_log_path.display(),
                stdin_path.display()
            ),
        );

        let mut snapshot_plugin = MysqlPlugin::new(
            "app".to_string(),
            "snapshot-host".to_string(),
            3_307,
            "snapshot-user".to_string(),
            temp_dir.path().join("old-snaps"),
        );
        snapshot_plugin.mysqldump_bin = mysqldump.display().to_string();

        let snapshot_id = snapshot_plugin
            .snapshot(temp_dir.path(), "dangerous command")
            .await
            .unwrap();

        let mut rollback_plugin = MysqlPlugin::new(
            "app".to_string(),
            "drifted-host".to_string(),
            4_321,
            "drifted-user".to_string(),
            temp_dir.path().join("new-snaps"),
        );
        rollback_plugin.mysql_bin = mysql.display().to_string();

        rollback_plugin.rollback(&snapshot_id).await.unwrap();

        let logged_args = fs::read_to_string(&restore_log_path).unwrap();
        assert!(
            logged_args
                .lines()
                .any(|line| line == "--host=snapshot-host")
        );
        assert!(logged_args.lines().any(|line| line == "--port=3307"));
        assert!(
            logged_args
                .lines()
                .any(|line| line == "--user=snapshot-user")
        );
        assert!(
            !logged_args
                .lines()
                .any(|line| line == "--host=drifted-host")
        );
        assert!(!logged_args.lines().any(|line| line == "--port=4321"));
        assert!(
            !logged_args
                .lines()
                .any(|line| line == "--user=drifted-user")
        );
        assert_eq!(fs::read_to_string(&stdin_path).unwrap(), "dump-data");
    }

    #[tokio::test]
    async fn rollback_returns_stderr_when_mysql_fails() {
        let temp_dir = TempDir::new().unwrap();
        let dump_path = temp_dir.path().join("snaps").join("existing.sql");
        fs::create_dir_all(dump_path.parent().unwrap()).unwrap();
        fs::write(&dump_path, "dump-data").unwrap();
        let mysql = stub_bin(
            &temp_dir,
            "mysql",
            "cat > /dev/null\nprintf 'mysql exploded' >&2\nexit 23",
        );
        let mut plugin = plugin_with_user(&temp_dir, "root");
        plugin.mysql_bin = mysql.display().to_string();
        let snapshot_id = snapshot_id_for("app", "localhost", 3_306, "root", &dump_path);

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
    async fn rollback_errors_when_mysql_stdin_streaming_fails() {
        let temp_dir = TempDir::new().unwrap();
        let dump_path = temp_dir.path().join("snaps").join("existing.sql");
        fs::create_dir_all(dump_path.parent().unwrap()).unwrap();
        fs::write(&dump_path, vec![b'x'; 512 * 1024]).unwrap();
        let mysql = stub_bin(&temp_dir, "mysql", "exit 0");
        let mut plugin = plugin_with_user(&temp_dir, "root");
        plugin.mysql_bin = mysql.display().to_string();
        let snapshot_id = snapshot_id_for("app", "localhost", 3_306, "root", &dump_path);

        let err = plugin.rollback(&snapshot_id).await.unwrap_err();

        match err {
            AegisError::Snapshot(msg) => assert!(msg.contains("failed to write mysql stdin")),
            other => panic!("expected snapshot error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rollback_drains_large_stderr_without_deadlocking() {
        let temp_dir = TempDir::new().unwrap();
        let dump_path = temp_dir.path().join("snaps").join("existing.sql");
        fs::create_dir_all(dump_path.parent().unwrap()).unwrap();
        fs::write(&dump_path, "dump-data").unwrap();
        let mysql = stub_bin(
            &temp_dir,
            "mysql",
            "i=0\nwhile [ \"$i\" -lt 5000 ]; do\n  printf '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\\n' >&2\n  i=$((i + 1))\ndone\ncat > /dev/null",
        );
        let mut plugin = plugin_with_user(&temp_dir, "root");
        plugin.mysql_bin = mysql.display().to_string();
        let snapshot_id = snapshot_id_for("app", "localhost", 3_306, "root", &dump_path);

        plugin.rollback(&snapshot_id).await.unwrap();
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
        let first_parts: Vec<_> = first_id.split(SEP).collect();
        let second_parts: Vec<_> = second_id.split(SEP).collect();
        let first_dump = PathBuf::from(decode_hex(first_parts[5]));
        let second_dump = PathBuf::from(decode_hex(second_parts[5]));

        assert_ne!(first_id, second_id);
        assert_ne!(first_dump, second_dump);
        assert!(first_dump.exists());
        assert!(second_dump.exists());
    }
}
