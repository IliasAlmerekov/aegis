use std::env;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::process::Command;

use crate::config::SupabaseSnapshotConfig;
use crate::error::AegisError;
use crate::snapshot::SnapshotPlugin;

type Result<T> = std::result::Result<T, AegisError>;

const SNAPSHOT_ID_VERSION: &str = "supabase-v1";
const SNAPSHOT_ID_SEP: char = '\t';
const MANIFEST_FILE_NAME: &str = "manifest.json";

#[cfg(test)]
thread_local! {
    static INJECT_MANIFEST_WRITE_FAILURE_FOR_TESTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SupabaseOverallStatus {
    Complete,
    Partial,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SupabaseCapabilities {
    db: bool,
    storage: bool,
    functions: bool,
    project_config: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SupabaseTarget {
    project_ref: String,
    db: Option<SupabaseTargetDb>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SupabaseTargetDb {
    database: String,
    host: String,
    port: u16,
    user: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SupabaseDbArtifact {
    present: bool,
    complete: bool,
    path: Option<String>,
    format: Option<String>,
    checksum_sha256: Option<String>,
    size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SupabaseArtifacts {
    db: SupabaseDbArtifact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
struct SupabaseRollback {
    db_supported: bool,
    allowed: bool,
    config_target_match_required: bool,
    reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SupabaseStrictEligibility {
    db_supported: bool,
    allowed: bool,
    overall_status: SupabaseOverallStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SupabaseManifest {
    manifest_version: u32,
    provider: String,
    created_at: String,
    capabilities: SupabaseCapabilities,
    target: SupabaseTarget,
    artifacts: SupabaseArtifacts,
    rollback: SupabaseRollback,
    partial: bool,
    degraded: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
    overall_status: SupabaseOverallStatus,
}

/// Snapshot plugin for Supabase project snapshots.
pub struct SupabasePlugin {
    config: SupabaseSnapshotConfig,
    snapshots_dir: PathBuf,
    pg_dump_bin: String,
    pg_restore_bin: String,
    #[cfg(test)]
    inject_manifest_write_failure_for_tests: bool,
}

impl SupabasePlugin {
    /// Create a new Supabase snapshot plugin.
    pub fn new(config: SupabaseSnapshotConfig, snapshots_dir: PathBuf) -> Self {
        Self {
            config,
            snapshots_dir,
            pg_dump_bin: "pg_dump".to_string(),
            pg_restore_bin: "pg_restore".to_string(),
            #[cfg(test)]
            inject_manifest_write_failure_for_tests: false,
        }
    }

    async fn binary_available(binary: &str) -> bool {
        let binary_path = Path::new(binary);
        if binary_path.components().count() > 1 || binary_path.is_absolute() {
            return Self::is_runnable_file_async(binary_path).await;
        }

        let Some(path_env) = env::var_os("PATH") else {
            return false;
        };

        for path_dir in env::split_paths(&path_env) {
            let candidate = path_dir.join(binary);
            if Self::is_runnable_file_async(&candidate).await {
                return true;
            }

            #[cfg(windows)]
            {
                let pathext = env::var_os("PATHEXT")
                    .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into())
                    .to_string_lossy()
                    .to_string();

                for ext in pathext.split(';') {
                    let ext = ext.trim();
                    if ext.is_empty() {
                        continue;
                    }

                    let candidate = path_dir.join(format!("{binary}{ext}"));
                    if Self::is_runnable_file_async(&candidate).await {
                        return true;
                    }
                }
            }
        }

        false
    }

    async fn is_runnable_file_async(path: &Path) -> bool {
        let Ok(metadata) = tokio::fs::metadata(path).await else {
            return false;
        };

        Self::metadata_is_runnable_file(metadata)
    }

    fn metadata_is_runnable_file(metadata: std::fs::Metadata) -> bool {
        if !metadata.is_file() {
            return false;
        }

        #[cfg(unix)]
        {
            metadata.permissions().mode() & 0o111 != 0
        }

        #[cfg(not(unix))]
        {
            true
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

        Self::validate_manifest_path(&path, label)?;
        Ok(path)
    }

    fn validate_manifest_path(path: &Path, label: &str) -> Result<()> {
        if !path.is_absolute()
            || path.file_name() != Some(MANIFEST_FILE_NAME.as_ref())
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

    fn build_snapshot_id(manifest_path: &Path) -> String {
        format!(
            "{SNAPSHOT_ID_VERSION}{SNAPSHOT_ID_SEP}{}",
            Self::encode_path(manifest_path)
        )
    }

    fn parse_snapshot_id(snapshot_id: &str) -> Result<PathBuf> {
        let (version, encoded_path) = snapshot_id.split_once(SNAPSHOT_ID_SEP).ok_or_else(|| {
            AegisError::Snapshot(format!("malformed snapshot_id: {snapshot_id:?}"))
        })?;

        if version != SNAPSHOT_ID_VERSION {
            return Err(AegisError::Snapshot(format!(
                "unsupported snapshot_id version {version:?}"
            )));
        }

        Self::decode_path(encoded_path, "manifest path")
    }

    async fn validate_preflight(&self) -> Result<()> {
        if self.config.db.database.trim().is_empty() {
            return Err(AegisError::Snapshot(
                "supabase_snapshot.db.database is required".to_string(),
            ));
        }
        if !Self::binary_available(&self.pg_dump_bin).await {
            return Err(AegisError::Snapshot(
                "pg_dump is required for supabase snapshots".to_string(),
            ));
        }
        if !Self::binary_available(&self.pg_restore_bin).await {
            return Err(AegisError::Snapshot(
                "pg_restore is required for strict supabase rollback".to_string(),
            ));
        }
        Ok(())
    }

    fn create_bundle_dir(&self) -> Result<PathBuf> {
        fs::create_dir_all(&self.snapshots_dir)?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);

        let mut suffix = 0usize;
        loop {
            let dir_name = if suffix == 0 {
                format!("supabase-{timestamp}")
            } else {
                format!("supabase-{timestamp}-{suffix}")
            };
            let bundle_dir = self.snapshots_dir.join(dir_name);

            match fs::create_dir(&bundle_dir) {
                Ok(()) => return Ok(bundle_dir),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    suffix += 1;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn cleanup_failed_bundle(&self, bundle_dir: &Path, dump_path: Option<&Path>) -> Result<()> {
        let mut failures = Vec::new();

        if let Some(path) = dump_path {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    failures.push(format!("failed to remove dump {}: {error}", path.display()))
                }
            }
        }

        match fs::remove_dir_all(bundle_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => failures.push(format!(
                "failed to remove bundle {}: {error}",
                bundle_dir.display()
            )),
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(AegisError::Snapshot(failures.join("; ")))
        }
    }

    fn fail_closed_after_cleanup(
        &self,
        original_error: AegisError,
        bundle_dir: &Path,
        dump_path: Option<&Path>,
    ) -> AegisError {
        match self.cleanup_failed_bundle(bundle_dir, dump_path) {
            Ok(()) => original_error,
            Err(cleanup_error) => {
                AegisError::Snapshot(format!("{original_error}; cleanup failed: {cleanup_error}"))
            }
        }
    }

    async fn run_pg_dump(&self, dump_path: &Path) -> Result<()> {
        let output = Command::new(&self.pg_dump_bin)
            .arg("-Fc")
            .arg("-h")
            .arg(&self.config.db.host)
            .arg("-p")
            .arg(self.config.db.port.to_string())
            .arg("-U")
            .arg(&self.config.db.user)
            .arg("-f")
            .arg(dump_path)
            .arg(&self.config.db.database)
            .output()
            .await
            .map_err(|error| AegisError::Snapshot(format!("failed to run pg_dump: {error}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AegisError::Snapshot(format!("pg_dump failed: {stderr}")));
        }

        Ok(())
    }

    async fn run_pg_restore(&self, target: &SupabaseTargetDb, dump_path: &Path) -> Result<()> {
        let output = Command::new(&self.pg_restore_bin)
            .arg("--clean")
            .arg("--if-exists")
            .arg("--create")
            .arg("-h")
            .arg(&target.host)
            .arg("-p")
            .arg(target.port.to_string())
            .arg("-U")
            .arg(&target.user)
            .arg("-d")
            .arg("postgres")
            .arg(dump_path)
            .output()
            .await
            .map_err(|error| AegisError::Snapshot(format!("failed to run pg_restore: {error}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AegisError::Snapshot(format!("pg_restore failed: {stderr}")));
        }

        Ok(())
    }
}

fn sha256_hex(path: &Path) -> Result<String> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn sync_parent_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let dir = fs::File::open(path)?;
        dir.sync_all()?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

mod runtime;
