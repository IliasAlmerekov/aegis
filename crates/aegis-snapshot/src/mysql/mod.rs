//! MySQL snapshot provider.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::process::{ChildStderr, Command};
use tokio::task::JoinHandle;

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use crate::SnapshotPlugin;
use crate::error::SnapshotError;

type Result<T> = std::result::Result<T, SnapshotError>;

const SEP: char = '\t';
const SNAPSHOT_ID_VERSION: &str = "v2";
const EXECUTABLE_BUSY_ERRNO: i32 = 26;
const BUSY_RETRY_ATTEMPTS: usize = 12;
const BUSY_RETRY_DELAY_MS: u64 = 25;

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

    async fn spawn_with_busy_retry(
        command: &mut Command,
        context: &str,
    ) -> Result<tokio::process::Child> {
        let mut attempt = 0usize;
        loop {
            match command.spawn() {
                Ok(child) => return Ok(child),
                Err(error) if Self::is_executable_busy(&error) && attempt < BUSY_RETRY_ATTEMPTS => {
                    attempt += 1;
                    tracing::warn!(
                        context,
                        attempt,
                        "mysql binary busy during command launch, retrying"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(BUSY_RETRY_DELAY_MS)).await;
                }
                Err(error) => {
                    return Err(SnapshotError::Snapshot(format!("{context}: {error}")));
                }
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

        String::from_utf8(bytes).map_err(|_| {
            SnapshotError::Snapshot(format!(
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

    fn validate_dump_file_name(dump_file_name: &str) -> Result<()> {
        let path = Path::new(dump_file_name);
        let mut components = path.components();
        let is_plain_file_name = matches!(
            (components.next(), components.next()),
            (Some(std::path::Component::Normal(name)), None)
                if name.to_str() == Some(dump_file_name)
        );
        if !is_plain_file_name {
            return Err(SnapshotError::Snapshot(format!(
                "malformed snapshot_id: invalid dump reference {dump_file_name:?}"
            )));
        }

        Ok(())
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

        Self::validate_snapshot_path(&path, label)?;
        Ok(path)
    }

    fn is_executable_busy(error: &std::io::Error) -> bool {
        error.raw_os_error() == Some(EXECUTABLE_BUSY_ERRNO)
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
            return Err(SnapshotError::Snapshot(format!(
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

    fn parse_snapshot_id(&self, snapshot_id: &str) -> Result<MysqlRollbackTarget> {
        if snapshot_id.starts_with(&format!("{SNAPSHOT_ID_VERSION}{SEP}")) {
            return Self::parse_v2_snapshot_id(snapshot_id);
        }

        self.parse_legacy_snapshot_id(snapshot_id)
    }

    fn parse_v2_snapshot_id(snapshot_id: &str) -> Result<MysqlRollbackTarget> {
        let parts: Vec<_> = snapshot_id.split(SEP).collect();
        if parts.len() != 6 || parts[0] != SNAPSHOT_ID_VERSION {
            return Err(SnapshotError::Snapshot(format!(
                "malformed snapshot_id: {snapshot_id:?}"
            )));
        }

        let _database = Self::decode_database(parts[1])?;
        let host = Self::decode_component(parts[2], "host")?;
        let port = parts[3].parse::<u16>().map_err(|_| {
            SnapshotError::Snapshot(format!(
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

    fn parse_legacy_snapshot_id(&self, snapshot_id: &str) -> Result<MysqlRollbackTarget> {
        let (database_encoded, dump_ref_encoded) =
            snapshot_id.split_once(SEP).ok_or_else(|| {
                SnapshotError::Snapshot(format!("malformed snapshot_id: {snapshot_id:?}"))
            })?;
        let _database = Self::decode_database(database_encoded)?;
        let dump_file_name = Self::decode_component(dump_ref_encoded, "dump reference")?;
        Self::validate_dump_file_name(&dump_file_name)?;

        Err(SnapshotError::Snapshot(
            "legacy mysql snapshot IDs cannot be safely restored after v2 hardening because the original target server/account was not recorded".to_string(),
        ))
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
                SnapshotError::Snapshot(format!("failed to join {command_name} stderr task: {err}"))
            })?
            .map_err(|err| {
                SnapshotError::Snapshot(format!("failed to read {command_name} stderr: {err}"))
            })
    }
}

#[async_trait]
impl SnapshotPlugin for MysqlPlugin {
    fn name(&self) -> &'static str {
        "mysql"
    }

    async fn is_applicable(&self, _cwd: &Path) -> bool {
        if self.database.is_empty() {
            return false;
        }

        tokio::process::Command::new("which")
            .arg(&self.mysqldump_bin)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
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

        let mut command = Command::new(&self.mysqldump_bin);
        command
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child =
            match Self::spawn_with_busy_retry(&mut command, "failed to run mysqldump").await {
                Ok(child) => child,
                Err(err) => {
                    let _ = std::fs::remove_file(&dump_path);
                    return Err(err);
                }
            };

        let mut stdout = child.stdout.take().ok_or_else(|| {
            let _ = std::fs::remove_file(&dump_path);
            SnapshotError::Snapshot("failed to capture mysqldump stdout".to_string())
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            let _ = std::fs::remove_file(&dump_path);
            SnapshotError::Snapshot("failed to capture mysqldump stderr".to_string())
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
            return Err(SnapshotError::Snapshot(format!(
                "failed to stream mysqldump output: {err}"
            )));
        }
        drop(stdout);
        dump_file.flush().await?;
        drop(dump_file);

        let status = child
            .wait()
            .await
            .map_err(|e| SnapshotError::Snapshot(format!("failed to wait for mysqldump: {e}")))?;
        let stderr = Self::collect_stderr(stderr_task, "mysqldump").await?;

        if !status.success() {
            let _ = std::fs::remove_file(&dump_path);
            let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
            return Err(SnapshotError::Snapshot(format!(
                "mysqldump failed: {stderr}"
            )));
        }

        let dump_path = dump_path.canonicalize()?;
        let snapshot_id = self.build_snapshot_id(&dump_path);
        tracing::info!(%snapshot_id, "mysql snapshot created");
        Ok(snapshot_id)
    }

    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        let target = self.parse_snapshot_id(snapshot_id)?;
        if !target.dump_path.exists() {
            return Err(SnapshotError::RollbackDumpNotFound {
                path: target.dump_path.to_string_lossy().to_string(),
            });
        }

        let args = Self::build_common_args_for_target(&target.host, target.port, &target.user);
        let mut command = Command::new(&self.mysql_bin);
        command
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        let mut child = Self::spawn_with_busy_retry(&mut command, "failed to run mysql").await?;

        let mut dump_file = tokio::fs::File::open(&target.dump_path).await?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| SnapshotError::Snapshot("failed to capture mysql stderr".to_string()))?;
        let stderr_task = Self::spawn_stderr_drain(stderr);
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| SnapshotError::Snapshot("failed to open mysql stdin".to_string()))?;
        if let Err(err) = io::copy(&mut dump_file, &mut stdin).await {
            drop(stdin);
            Self::kill_and_reap_child(&mut child).await;
            let _ = Self::collect_stderr(stderr_task, "mysql").await;
            return Err(SnapshotError::Snapshot(format!(
                "failed to write mysql stdin: {err}"
            )));
        }
        drop(stdin);

        let status = child
            .wait()
            .await
            .map_err(|e| SnapshotError::Snapshot(format!("failed to wait for mysql: {e}")))?;
        let stderr = Self::collect_stderr(stderr_task, "mysql").await?;

        if !status.success() {
            let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
            return Err(SnapshotError::Snapshot(format!("mysql failed: {stderr}")));
        }

        tracing::info!(snapshot_id = snapshot_id, "mysql snapshot rolled back");
        Ok(())
    }
}

#[cfg(test)]
mod tests;
