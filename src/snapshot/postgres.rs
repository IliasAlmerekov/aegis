use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::process::Command;

#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use crate::error::AegisError;
use crate::snapshot::SnapshotPlugin;

type Result<T> = std::result::Result<T, AegisError>;

const SEP: char = '\t';
const SNAPSHOT_ID_VERSION: &str = "v2";

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
        let chars: Vec<_> = encoded.as_bytes().chunks_exact(2).collect();
        for pair in chars {
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

    fn parse_snapshot_id(snapshot_id: &str) -> Result<PostgresRollbackTarget> {
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

        Ok(PostgresRollbackTarget {
            host,
            port,
            user,
            dump_path,
        })
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

        let dump_path = dump_path.canonicalize()?;
        let snapshot_id = self.build_snapshot_id(&dump_path);
        tracing::info!(%snapshot_id, "postgres snapshot created");
        Ok(snapshot_id)
    }

    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        let target = Self::parse_snapshot_id(snapshot_id)?;
        if !target.dump_path.exists() {
            return Err(AegisError::RollbackDumpNotFound {
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

    #[test]
    fn reserve_dump_path_sanitizes_database_name_for_filenames() {
        let temp_dir = TempDir::new().unwrap();
        let snapshots_dir = temp_dir.path().join("snaps");
        fs::create_dir_all(&snapshots_dir).unwrap();
        let plugin = PostgresPlugin::new(
            "app/\tname:prod".to_string(),
            "localhost".to_string(),
            5432,
            "postgres".to_string(),
            snapshots_dir,
        );

        let dump_path = plugin.reserve_dump_path(1_234).unwrap();

        assert_eq!(
            dump_path.file_name().unwrap().to_string_lossy(),
            "pg-app__name_prod-1234.dump"
        );
    }

    #[test]
    fn snapshot_id_v2_round_trips_target_fields_and_dump_path() {
        let dump_path = Path::new("/tmp/pg\tbackup.dump");
        let snapshot_id = snapshot_id_for(
            "app/\tname:prod",
            "db\tprimary",
            5_432,
            "postgres\tadmin",
            dump_path,
        );
        let parts: Vec<_> = snapshot_id.split(SEP).collect();

        assert_eq!(parts.len(), 6);
        assert_eq!(parts[0], "v2");
        assert_eq!(decode_hex(parts[1]), "app/\tname:prod");
        assert_eq!(decode_hex(parts[2]), "db\tprimary");
        assert_eq!(parts[3], "5432");
        assert_eq!(decode_hex(parts[4]), "postgres\tadmin");
        assert_eq!(PathBuf::from(decode_hex(parts[5])), dump_path);
    }

    #[tokio::test]
    async fn rollback_errors_when_dump_file_missing() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "postgres");
        let missing_dump = temp_dir.path().join("missing.dump");
        let snapshot_id = snapshot_id_for("app", "localhost", 5_432, "postgres", &missing_dump);

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
    async fn rollback_errors_on_malformed_database_encoding() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "postgres");

        let err = plugin
            .rollback(
                "v2\txyz\t686f7374\t5432\t706f737467726573\t2f746d702f6578616d706c652e64756d70",
            )
            .await
            .unwrap_err();

        match err {
            AegisError::Snapshot(msg) => assert!(msg.contains("invalid database encoding")),
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
        let parts: Vec<_> = snapshot_id.split(SEP).collect();
        let logged_args = fs::read_to_string(&log_path).unwrap();

        assert_eq!(parts.len(), 6);
        assert_eq!(parts[0], "v2");
        assert_eq!(decode_hex(parts[1]), "app");
        assert_eq!(decode_hex(parts[2]), "localhost");
        assert_eq!(parts[3], "5432");
        assert_eq!(decode_hex(parts[4]), "postgres");
        assert!(logged_args.lines().any(|line| line == "-Fc"));
        assert!(logged_args.lines().any(|line| line == "-f"));
        assert!(logged_args.lines().any(|line| line == "app"));
        let dump_path = PathBuf::from(decode_hex(parts[5]));
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
        let snapshot_id = snapshot_id_for("app", "localhost", 5_432, "postgres", &dump_path);

        plugin.rollback(&snapshot_id).await.unwrap();

        let logged_args = fs::read_to_string(&log_path).unwrap();
        assert!(logged_args.lines().any(|line| line == "--clean"));
        assert!(logged_args.lines().any(|line| line == "--if-exists"));
        assert!(logged_args.lines().any(|line| line == "--create"));
        assert!(logged_args.lines().any(|line| line == "-d"));
        assert!(logged_args.lines().any(|line| line == "postgres"));
        assert!(!logged_args.lines().any(|line| line == "app"));
        assert!(
            logged_args
                .lines()
                .any(|line| line == dump_path.to_string_lossy())
        );
    }

    #[tokio::test]
    async fn rollback_uses_snapshot_time_target_instead_of_current_config() {
        let temp_dir = TempDir::new().unwrap();
        let dump_log_path = temp_dir.path().join("pg_dump.args");
        let restore_log_path = temp_dir.path().join("pg_restore.args");
        let pg_dump = stub_bin(
            &temp_dir,
            "pg_dump",
            &format!(
                "log='{}'\nout=''\nprev=''\n: > \"$log\"\nfor arg in \"$@\"; do\n  printf '%s\\n' \"$arg\" >> \"$log\"\n  if [ \"$prev\" = '-f' ]; then out=\"$arg\"; fi\n  prev=\"$arg\"\ndone\nprintf 'dump-data' > \"$out\"",
                dump_log_path.display()
            ),
        );
        let pg_restore = stub_bin(
            &temp_dir,
            "pg_restore",
            &format!(
                "log='{}'\n: > \"$log\"\nfor arg in \"$@\"; do\n  printf '%s\\n' \"$arg\" >> \"$log\"\ndone",
                restore_log_path.display()
            ),
        );

        let old_snaps = temp_dir.path().join("old-snaps");
        let new_snaps = temp_dir.path().join("new-snaps");
        let mut snapshot_plugin = PostgresPlugin::new(
            "app".to_string(),
            "snapshot-host".to_string(),
            5_543,
            "snapshot-user".to_string(),
            old_snaps,
        );
        snapshot_plugin.pg_dump_bin = pg_dump.display().to_string();

        let snapshot_id = snapshot_plugin
            .snapshot(temp_dir.path(), "dangerous command")
            .await
            .unwrap();

        let mut rollback_plugin = PostgresPlugin::new(
            "app".to_string(),
            "drifted-host".to_string(),
            6_432,
            "drifted-user".to_string(),
            new_snaps,
        );
        rollback_plugin.pg_restore_bin = pg_restore.display().to_string();

        rollback_plugin.rollback(&snapshot_id).await.unwrap();

        let logged_args = fs::read_to_string(&restore_log_path).unwrap();
        assert!(logged_args.lines().any(|line| line == "snapshot-host"));
        assert!(logged_args.lines().any(|line| line == "5543"));
        assert!(logged_args.lines().any(|line| line == "snapshot-user"));
        assert!(!logged_args.lines().any(|line| line == "drifted-host"));
        assert!(!logged_args.lines().any(|line| line == "6432"));
        assert!(!logged_args.lines().any(|line| line == "drifted-user"));
    }

    #[tokio::test]
    async fn rollback_rejects_malformed_dump_path_encoding() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = plugin_with_user(&temp_dir, "postgres");

        let err = plugin
            .rollback("v2\t617070\t6c6f63616c686f7374\t5432\t706f737467726573\txyz")
            .await
            .unwrap_err();

        match err {
            AegisError::Snapshot(msg) => assert!(msg.contains("invalid dump path encoding")),
            other => panic!("expected malformed snapshot snapshot error, got {other:?}"),
        }
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
        let snapshot_id = snapshot_id_for("app", "localhost", 5_432, "postgres", &dump_path);

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
        let first_parts: Vec<_> = first_id.split(SEP).collect();
        let second_parts: Vec<_> = second_id.split(SEP).collect();
        let first_dump = decode_hex(first_parts[5]);
        let second_dump = decode_hex(second_parts[5]);

        assert_ne!(first_id, second_id);
        assert_ne!(first_dump, second_dump);
        assert!(Path::new(&first_dump).exists());
        assert!(Path::new(&second_dump).exists());
    }
}
