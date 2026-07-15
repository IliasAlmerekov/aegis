//! PostgreSQL snapshot provider.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::process::Command;

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use crate::SnapshotPlugin;
use crate::containment::contain_artifact;
use crate::error::SnapshotError;

type Result<T> = std::result::Result<T, SnapshotError>;

const SEP: char = '\t';
const SNAPSHOT_ID_VERSION: &str = "v2";
const EXECUTABLE_BUSY_ERRNO: i32 = 26;
const BUSY_RETRY_ATTEMPTS: usize = 12;
const BUSY_RETRY_DELAY_MS: u64 = 25;

struct PostgresRollbackTarget {
    host: String,
    port: u16,
    user: String,
    dump_path: PathBuf,
}

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
        Self::build_common_args_for_target(&self.host, self.port, &self.user)
    }

    fn build_common_args_for_target(host: &str, port: u16, user: &str) -> Vec<String> {
        let mut args = vec![
            "-h".to_string(),
            host.to_string(),
            "-p".to_string(),
            port.to_string(),
        ];

        if !user.is_empty() {
            args.push("-U".to_string());
            args.push(user.to_string());
        }

        args
    }

    fn dump_path_candidate(&self, timestamp: u64, suffix: Option<usize>) -> PathBuf {
        let base_name = format!("pg-{}-{timestamp}", self.sanitized_database_label());
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

    async fn output_with_busy_retry(
        command: &mut Command,
        context: &str,
    ) -> Result<std::process::Output> {
        let mut attempt = 0usize;
        loop {
            match command.output().await {
                Ok(output) => return Ok(output),
                Err(error) if Self::is_executable_busy(&error) && attempt < BUSY_RETRY_ATTEMPTS => {
                    attempt += 1;
                    tracing::warn!(
                        context,
                        attempt,
                        "postgres binary busy during command launch, retrying"
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
        let chars: Vec<_> = encoded.as_bytes().chunks_exact(2).collect();
        for pair in chars {
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

        Ok(path)
    }

    fn is_executable_busy(error: &std::io::Error) -> bool {
        error.raw_os_error() == Some(EXECUTABLE_BUSY_ERRNO)
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

    fn parse_snapshot_id(&self, snapshot_id: &str) -> Result<PostgresRollbackTarget> {
        if snapshot_id.starts_with(&format!("{SNAPSHOT_ID_VERSION}{SEP}")) {
            return Self::parse_v2_snapshot_id(snapshot_id);
        }

        self.parse_legacy_snapshot_id(snapshot_id)
    }

    fn parse_v2_snapshot_id(snapshot_id: &str) -> Result<PostgresRollbackTarget> {
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

        Ok(PostgresRollbackTarget {
            host,
            port,
            user,
            dump_path,
        })
    }

    fn parse_legacy_snapshot_id(&self, snapshot_id: &str) -> Result<PostgresRollbackTarget> {
        let (database_encoded, dump_str) = snapshot_id.split_once(SEP).ok_or_else(|| {
            SnapshotError::Snapshot(format!("malformed snapshot_id: {snapshot_id:?}"))
        })?;
        let _database = Self::decode_database(database_encoded)?;
        Ok(PostgresRollbackTarget {
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            dump_path: PathBuf::from(dump_str),
        })
    }
}

#[async_trait]
impl SnapshotPlugin for PostgresPlugin {
    fn name(&self) -> &'static str {
        "postgres"
    }

    async fn is_applicable(&self, _cwd: &Path) -> bool {
        if self.database.is_empty() {
            return false;
        }

        tokio::process::Command::new("which")
            .arg(&self.pg_dump_bin)
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
            "-Fc".to_string(),
            "-f".to_string(),
            dump_path.display().to_string(),
            self.database.clone(),
        ]);

        let mut command = Command::new(&self.pg_dump_bin);
        command.args(&args);
        let output = Self::output_with_busy_retry(&mut command, "failed to run pg_dump").await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(SnapshotError::Snapshot(format!("pg_dump failed: {stderr}")));
        }

        let dump_path = dump_path.canonicalize()?;
        let snapshot_id = self.build_snapshot_id(&dump_path);
        tracing::info!(%snapshot_id, "postgres snapshot created");
        Ok(snapshot_id)
    }

    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        let mut target = self.parse_snapshot_id(snapshot_id)?;
        target.dump_path = contain_artifact("postgres", &self.snapshots_dir, &target.dump_path)?;
        if !target.dump_path.exists() {
            return Err(SnapshotError::RollbackDumpNotFound {
                path: target.dump_path.to_string_lossy().to_string(),
            });
        }

        let mut args = Self::build_common_args_for_target(&target.host, target.port, &target.user);
        args.extend([
            "--clean".to_string(),
            "--if-exists".to_string(),
            "--create".to_string(),
            "-d".to_string(),
            "postgres".to_string(),
            target.dump_path.to_string_lossy().to_string(),
        ]);

        let mut command = Command::new(&self.pg_restore_bin);
        command.args(&args);
        let output = Self::output_with_busy_retry(&mut command, "failed to run pg_restore").await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(SnapshotError::Snapshot(format!(
                "pg_restore failed: {stderr}"
            )));
        }

        tracing::info!(snapshot_id = snapshot_id, "postgres snapshot rolled back");
        Ok(())
    }

    async fn delete(&self, snapshot_id: &str) -> Result<()> {
        let mut target = self.parse_snapshot_id(snapshot_id)?;
        target.dump_path = contain_artifact("postgres", &self.snapshots_dir, &target.dump_path)?;
        match std::fs::remove_file(&target.dump_path) {
            Ok(()) => {
                tracing::info!(path = %target.dump_path.display(), "postgres dump deleted");
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(path = %target.dump_path.display(), "postgres dump already removed");
                Ok(())
            }
            Err(error) => Err(SnapshotError::DeleteFailed {
                plugin: "postgres".to_string(),
                snapshot_id: snapshot_id.to_string(),
                source: error.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests;
