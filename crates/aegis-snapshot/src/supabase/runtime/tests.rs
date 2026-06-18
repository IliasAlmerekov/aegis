//! Unit tests for the Supabase snapshot runtime.
//!
//! Covers the atomic-write, rollback eligibility, config-target-match, and
//! snapshot-id encoding invariants. The test-only manifest write failure
//! injection hook (`INJECT_MANIFEST_WRITE_FAILURE_FOR_TESTS`) is exercised by
//! `snapshot_fails_when_manifest_commit_fails_and_removes_dump`.

use super::super::*;
use super::manifest_io::write_manifest_atomically;
use super::*;
use tempfile::TempDir;

fn stub_bin(dir: &TempDir, name: &str, body: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
    }

    path
}

fn manifest_path(temp_dir: &TempDir) -> PathBuf {
    temp_dir.path().join("bundle").join(MANIFEST_FILE_NAME)
}

fn valid_db_dump_checksum() -> String {
    format!("{:x}", Sha256::digest(b"dump-data"))
}

fn configured_supabase_snapshot_config(
    project_ref: &str,
    database: &str,
    host: &str,
    port: u16,
    user: &str,
) -> SupabaseSnapshotConfig {
    SupabaseSnapshotConfig {
        project_ref: project_ref.to_string(),
        db: aegis_config::PostgresSnapshotConfig {
            database: database.to_string(),
            host: host.to_string(),
            port,
            user: user.to_string(),
        },
        ..SupabaseSnapshotConfig::default()
    }
}

fn database_only_supabase_snapshot_config(database: &str) -> SupabaseSnapshotConfig {
    SupabaseSnapshotConfig {
        db: aegis_config::PostgresSnapshotConfig {
            database: database.to_string(),
            ..aegis_config::PostgresSnapshotConfig::default()
        },
        ..SupabaseSnapshotConfig::default()
    }
}

fn write_phase1_manifest_fixture(temp_dir: &TempDir, checksum: &str) -> PathBuf {
    let manifest_path = manifest_path(temp_dir);
    let artifacts_dir = manifest_path.parent().unwrap().join("artifacts");
    fs::create_dir_all(&artifacts_dir).unwrap();

    let dump_path = artifacts_dir.join("db.dump");
    fs::write(&dump_path, "dump-data").unwrap();

    let mut manifest = SupabaseManifest::phase1_fixture();
    manifest.artifacts.db.checksum_sha256 = Some(checksum.to_string());
    manifest.artifacts.db.size_bytes = Some(fs::metadata(&dump_path).unwrap().len());
    write_manifest_atomically(&manifest_path, &manifest).unwrap();
    manifest_path
}

#[test]
fn snapshot_id_v1_round_trips_manifest_path() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = manifest_path(&temp_dir);
    fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
    fs::write(&manifest_path, "{}").unwrap();

    let snapshot_id = SupabasePlugin::build_snapshot_id(&manifest_path);
    let decoded = SupabasePlugin::parse_snapshot_id(&snapshot_id).unwrap();

    assert_eq!(decoded, manifest_path);
}

#[test]
fn manifest_requires_target_db_for_v1() {
    let manifest = SupabaseManifest {
        manifest_version: 1,
        provider: "supabase".to_string(),
        created_at: "2026-04-15T12:34:56Z".to_string(),
        capabilities: SupabaseCapabilities::phase1(),
        target: SupabaseTarget {
            project_ref: "proj_123".to_string(),
            db: None,
        },
        artifacts: SupabaseArtifacts::phase1_empty(),
        rollback: SupabaseRollback::default(),
        partial: false,
        degraded: false,
        warnings: Vec::new(),
        errors: Vec::new(),
        overall_status: SupabaseOverallStatus::Failed,
    };

    let err = manifest.validate_schema_v1().unwrap_err();

    match err {
        SnapshotError::Snapshot(msg) => assert!(msg.contains("target.db")),
        other => panic!("expected snapshot error, got {other:?}"),
    }
}

#[test]
fn recompute_rollback_denies_partial_and_degraded_manifests() {
    let mut manifest = SupabaseManifest::phase1_fixture();
    manifest.partial = true;
    assert!(!manifest.recompute_strict_eligibility().unwrap().allowed);

    manifest.partial = false;
    manifest.degraded = true;
    assert!(!manifest.recompute_strict_eligibility().unwrap().allowed);
}

#[tokio::test]
async fn is_applicable_requires_explicit_config_and_both_tools() {
    let temp_dir = TempDir::new().unwrap();
    let pg_dump = stub_bin(&temp_dir, "pg_dump", "exit 0");
    let pg_restore = stub_bin(&temp_dir, "pg_restore", "exit 0");

    let config = database_only_supabase_snapshot_config("postgres");

    let mut plugin = SupabasePlugin::new(config.clone(), temp_dir.path().join("snapshots"));
    plugin.pg_dump_bin = pg_dump.display().to_string();
    plugin.pg_restore_bin = pg_restore.display().to_string();

    assert!(plugin.is_applicable(temp_dir.path()).await);

    let mut missing_db = SupabasePlugin::new(
        SupabaseSnapshotConfig::default(),
        temp_dir.path().join("snapshots"),
    );
    missing_db.pg_dump_bin = pg_dump.display().to_string();
    missing_db.pg_restore_bin = pg_restore.display().to_string();
    assert!(!missing_db.is_applicable(temp_dir.path()).await);

    let mut missing_restore =
        SupabasePlugin::new(config, temp_dir.path().join("snapshots-missing-restore"));
    missing_restore.pg_dump_bin = pg_dump.display().to_string();
    missing_restore.pg_restore_bin = temp_dir
        .path()
        .join("missing-pg_restore")
        .display()
        .to_string();
    assert!(!missing_restore.is_applicable(temp_dir.path()).await);
}

#[tokio::test]
async fn snapshot_uses_pg_dump_and_writes_manifest_bundle() {
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
    let pg_restore = stub_bin(&temp_dir, "pg_restore", "exit 0");

    let config = configured_supabase_snapshot_config(
        "proj_123",
        "postgres",
        "db.supabase.co",
        6543,
        "postgres",
    );

    // Canonicalize the snapshot root so the dump path the runtime logs (built
    // from the bundle dir) matches the canonical path encoded in the snapshot id
    // (runtime canonicalizes the manifest path). On macOS `/var` is a symlink to
    // `/private/var`, so without this the two paths differ.
    let snapshot_root = temp_dir.path().canonicalize().unwrap().join("snaps");
    let mut plugin = SupabasePlugin::new(config, snapshot_root);
    plugin.pg_dump_bin = pg_dump.display().to_string();
    plugin.pg_restore_bin = pg_restore.display().to_string();

    let snapshot_id = plugin
        .snapshot(temp_dir.path(), "terraform destroy")
        .await
        .unwrap();
    let manifest_path = SupabasePlugin::parse_snapshot_id(&snapshot_id).unwrap();
    let dump_path = manifest_path.parent().unwrap().join("artifacts/db.dump");
    let manifest: SupabaseManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let logged_args = fs::read_to_string(&log_path).unwrap();
    let logged_args: Vec<_> = logged_args.lines().map(str::to_string).collect();
    let expected_checksum = format!("{:x}", Sha256::digest(b"dump-data"));

    assert_eq!(manifest.provider, "supabase");
    assert_eq!(manifest.target.project_ref, "proj_123");
    assert_eq!(manifest.target.db.as_ref().unwrap().host, "db.supabase.co");
    assert!(logged_args.iter().any(|arg| arg == "-Fc"));
    assert!(
        logged_args
            .windows(2)
            .any(|window| window[0] == "-h" && window[1] == "db.supabase.co")
    );
    assert!(
        logged_args
            .windows(2)
            .any(|window| window[0] == "-p" && window[1] == "6543")
    );
    assert!(
        logged_args
            .windows(2)
            .any(|window| window[0] == "-U" && window[1] == "postgres")
    );
    assert!(
        logged_args
            .windows(2)
            .any(|window| window[0] == "-f" && window[1] == dump_path.display().to_string())
    );
    assert_eq!(logged_args.last().map(String::as_str), Some("postgres"));
    assert_eq!(
        manifest.artifacts.db.path.as_deref(),
        Some("artifacts/db.dump")
    );
    assert_eq!(
        manifest.artifacts.db.checksum_sha256.as_ref().unwrap(),
        &expected_checksum
    );
    assert_eq!(fs::read_to_string(&dump_path).unwrap(), "dump-data");
}

#[tokio::test]
async fn snapshot_fails_when_manifest_commit_fails_and_removes_dump() {
    let temp_dir = TempDir::new().unwrap();
    let pg_dump = stub_bin(
        &temp_dir,
        "pg_dump",
        "out=''\nprev=''\nfor arg in \"$@\"; do\n  if [ \"$prev\" = '-f' ]; then out=\"$arg\"; fi\n  prev=\"$arg\"\ndone\nprintf 'dump-data' > \"$out\"",
    );
    let pg_restore = stub_bin(&temp_dir, "pg_restore", "exit 0");

    let config = database_only_supabase_snapshot_config("postgres");

    let mut plugin = SupabasePlugin::new(config, temp_dir.path().join("snaps"));
    plugin.pg_dump_bin = pg_dump.display().to_string();
    plugin.pg_restore_bin = pg_restore.display().to_string();
    plugin.inject_manifest_write_failure_for_tests = true;

    let err = plugin
        .snapshot(temp_dir.path(), "terraform destroy")
        .await
        .unwrap_err();

    assert!(
        err.to_string().contains("manifest"),
        "expected manifest failure, got: {err}"
    );

    let bundle_root = temp_dir.path().join("snaps");
    let leftover_entries: Vec<PathBuf> = fs::read_dir(&bundle_root)
        .map(|entries| {
            entries
                .filter_map(std::result::Result::ok)
                .map(|entry| entry.path())
                .collect()
        })
        .unwrap_or_default();
    let orphan_dump_paths: Vec<PathBuf> = leftover_entries
        .iter()
        .map(|path| path.join("artifacts/db.dump"))
        .filter(|path| path.exists())
        .collect();
    let leftover_tmp_paths: Vec<PathBuf> = leftover_entries
        .iter()
        .map(|path| path.join("manifest.json.tmp"))
        .filter(|path| path.exists())
        .collect();

    assert!(
        orphan_dump_paths.is_empty(),
        "orphan db dump must be removed when manifest commit fails: {orphan_dump_paths:?}"
    );
    assert!(
        leftover_tmp_paths.is_empty(),
        "manifest temp file must be removed when manifest commit fails: {leftover_tmp_paths:?}"
    );
    assert!(
        leftover_entries.is_empty(),
        "bundle directories must be removed when manifest commit fails: {leftover_entries:?}"
    );
}

#[tokio::test]
async fn rollback_denies_when_config_target_mismatch_is_required() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_phase1_manifest_fixture(&temp_dir, &valid_db_dump_checksum());
    let pg_dump = stub_bin(&temp_dir, "pg_dump", "exit 0");
    let pg_restore = stub_bin(&temp_dir, "pg_restore", "exit 0");

    let config = configured_supabase_snapshot_config(
        "proj_123",
        "postgres",
        "drifted.supabase.co",
        5432,
        "postgres",
    );

    let mut plugin = SupabasePlugin::new(config, temp_dir.path().join("snapshots"));
    plugin.pg_dump_bin = pg_dump.display().to_string();
    plugin.pg_restore_bin = pg_restore.display().to_string();

    let snapshot_id = SupabasePlugin::build_snapshot_id(&manifest_path);
    let err = plugin.rollback(&snapshot_id).await.unwrap_err();

    match err {
        SnapshotError::Snapshot(msg) => assert!(msg.contains("rollback target mismatch")),
        other => panic!("expected target mismatch snapshot error, got {other:?}"),
    }
}

#[tokio::test]
async fn rollback_ignores_project_ref_mismatch_for_target_match_checks() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_phase1_manifest_fixture(&temp_dir, &valid_db_dump_checksum());
    let restore_log_path = temp_dir.path().join("pg_restore.args");
    let pg_dump = stub_bin(&temp_dir, "pg_dump", "exit 0");
    let pg_restore = stub_bin(
        &temp_dir,
        "pg_restore",
        &format!(
            "log='{}'\n: > \"$log\"\nfor arg in \"$@\"; do\n  printf '%s\\n' \"$arg\" >> \"$log\"\ndone",
            restore_log_path.display()
        ),
    );

    let config = configured_supabase_snapshot_config(
        "different-project-ref",
        "postgres",
        "db.supabase.co",
        5432,
        "postgres",
    );

    let mut plugin = SupabasePlugin::new(config, temp_dir.path().join("snapshots"));
    plugin.pg_dump_bin = pg_dump.display().to_string();
    plugin.pg_restore_bin = pg_restore.display().to_string();

    let snapshot_id = SupabasePlugin::build_snapshot_id(&manifest_path);
    plugin.rollback(&snapshot_id).await.unwrap();

    let logged_args = fs::read_to_string(&restore_log_path).unwrap();
    assert!(logged_args.contains("db.supabase.co"));
}

#[tokio::test]
async fn rollback_rejects_malformed_snapshot_id() {
    let temp_dir = TempDir::new().unwrap();
    let pg_dump = stub_bin(&temp_dir, "pg_dump", "exit 0");
    let pg_restore = stub_bin(&temp_dir, "pg_restore", "exit 0");

    let config = configured_supabase_snapshot_config(
        "proj_123",
        "postgres",
        "db.supabase.co",
        5432,
        "postgres",
    );

    let mut plugin = SupabasePlugin::new(config, temp_dir.path().join("snaps"));
    plugin.pg_dump_bin = pg_dump.display().to_string();
    plugin.pg_restore_bin = pg_restore.display().to_string();

    let err = plugin
        .rollback("v1\x00invalid")
        .await
        .expect_err("malformed snapshot id should fail");

    match err {
        SnapshotError::Snapshot(msg) => assert!(msg.contains("malformed snapshot_id")),
        other => panic!("expected snapshot error, got {other:?}"),
    }
}

#[tokio::test]
async fn rollback_denies_when_manifest_dump_is_missing() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_phase1_manifest_fixture(&temp_dir, &valid_db_dump_checksum());
    let pg_dump = stub_bin(&temp_dir, "pg_dump", "exit 0");
    let pg_restore = stub_bin(&temp_dir, "pg_restore", "exit 0");

    let dump_path = manifest_path.parent().unwrap().join("artifacts/db.dump");
    fs::remove_file(&dump_path).unwrap();

    let config = configured_supabase_snapshot_config(
        "proj_123",
        "postgres",
        "db.supabase.co",
        5432,
        "postgres",
    );

    let mut plugin = SupabasePlugin::new(config, temp_dir.path().join("snapshots"));
    plugin.pg_dump_bin = pg_dump.display().to_string();
    plugin.pg_restore_bin = pg_restore.display().to_string();

    let snapshot_id = SupabasePlugin::build_snapshot_id(&manifest_path);
    let err = plugin.rollback(&snapshot_id).await.unwrap_err();

    match err {
        SnapshotError::RollbackDumpNotFound { path } => {
            assert!(path.ends_with("artifacts/db.dump"));
        }
        other => panic!("expected rollback dump missing error, got {other:?}"),
    }
}

#[tokio::test]
async fn rollback_denies_when_checksum_mismatch() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_phase1_manifest_fixture(&temp_dir, &"0".repeat(64));
    let pg_dump = stub_bin(&temp_dir, "pg_dump", "exit 0");
    let pg_restore = stub_bin(&temp_dir, "pg_restore", "exit 0");

    let config = configured_supabase_snapshot_config(
        "proj_123",
        "postgres",
        "db.supabase.co",
        5432,
        "postgres",
    );

    let mut plugin = SupabasePlugin::new(config, temp_dir.path().join("snapshots"));
    plugin.pg_dump_bin = pg_dump.display().to_string();
    plugin.pg_restore_bin = pg_restore.display().to_string();

    let snapshot_id = SupabasePlugin::build_snapshot_id(&manifest_path);
    let err = plugin.rollback(&snapshot_id).await.unwrap_err();

    match err {
        SnapshotError::RollbackIntegrityCheckFailed {
            path,
            expected_sha256,
            actual_sha256,
        } => {
            assert!(path.ends_with("artifacts/db.dump"));
            assert_eq!(expected_sha256, "0".repeat(64));
            assert_eq!(actual_sha256, valid_db_dump_checksum());
        }
        other => panic!("expected RollbackIntegrityCheckFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn rollback_denies_when_recomputed_fields_disagree_with_manifest_summary() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_phase1_manifest_fixture(&temp_dir, &valid_db_dump_checksum());
    let pg_dump = stub_bin(&temp_dir, "pg_dump", "exit 0");
    let pg_restore = stub_bin(&temp_dir, "pg_restore", "exit 0");

    let mut manifest: SupabaseManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest.rollback.allowed = false;
    write_manifest_atomically(&manifest_path, &manifest).unwrap();

    let config = configured_supabase_snapshot_config(
        "proj_123",
        "postgres",
        "db.supabase.co",
        5432,
        "postgres",
    );

    let mut plugin = SupabasePlugin::new(config, temp_dir.path().join("snapshots"));
    plugin.pg_dump_bin = pg_dump.display().to_string();
    plugin.pg_restore_bin = pg_restore.display().to_string();

    let snapshot_id = SupabasePlugin::build_snapshot_id(&manifest_path);
    let err = plugin.rollback(&snapshot_id).await.unwrap_err();

    match err {
        SnapshotError::Snapshot(msg) => {
            assert!(msg.contains("summary"));
            assert!(msg.contains("recomputed"));
        }
        other => panic!("expected snapshot error, got {other:?}"),
    }
}

#[tokio::test]
async fn rollback_denies_when_persisted_db_supported_disagrees_with_recomputed_support() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_phase1_manifest_fixture(&temp_dir, &valid_db_dump_checksum());
    let pg_dump = stub_bin(&temp_dir, "pg_dump", "exit 0");
    let pg_restore = stub_bin(&temp_dir, "pg_restore", "exit 0");

    let mut manifest: SupabaseManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest.rollback.db_supported = false;
    write_manifest_atomically(&manifest_path, &manifest).unwrap();

    let config = configured_supabase_snapshot_config(
        "proj_123",
        "postgres",
        "db.supabase.co",
        5432,
        "postgres",
    );

    let mut plugin = SupabasePlugin::new(config, temp_dir.path().join("snapshots"));
    plugin.pg_dump_bin = pg_dump.display().to_string();
    plugin.pg_restore_bin = pg_restore.display().to_string();

    let snapshot_id = SupabasePlugin::build_snapshot_id(&manifest_path);
    let err = plugin.rollback(&snapshot_id).await.unwrap_err();

    match err {
        SnapshotError::Snapshot(msg) => {
            assert!(msg.contains("db_supported"));
            assert!(msg.contains("recomputed"));
        }
        other => panic!("expected snapshot error, got {other:?}"),
    }
}

#[test]
fn resolve_db_artifact_path_denies_absolute_artifact_path() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_phase1_manifest_fixture(&temp_dir, &valid_db_dump_checksum());
    let mut manifest: SupabaseManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();

    manifest.artifacts.db.path = Some("/tmp/evil.dump".to_string());

    let err = manifest
        .resolve_db_artifact_path(&manifest_path)
        .unwrap_err();
    match err {
        SnapshotError::Snapshot(msg) => assert!(msg.contains("bundle root")),
        other => panic!("expected snapshot error, got {other:?}"),
    }
}

#[test]
fn resolve_db_artifact_path_denies_parent_traversal() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_phase1_manifest_fixture(&temp_dir, &valid_db_dump_checksum());
    let mut manifest: SupabaseManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();

    manifest.artifacts.db.path = Some("../outside.dump".to_string());

    let err = manifest
        .resolve_db_artifact_path(&manifest_path)
        .unwrap_err();
    match err {
        SnapshotError::Snapshot(msg) => assert!(msg.contains("bundle root")),
        other => panic!("expected snapshot error, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn resolve_db_artifact_path_denies_symlink_escape_outside_bundle_root() {
    use std::os::unix::fs::symlink;

    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_phase1_manifest_fixture(&temp_dir, &valid_db_dump_checksum());
    let mut manifest: SupabaseManifest =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();

    let bundle_root = manifest_path.parent().unwrap();
    let outside_dir = temp_dir.path().join("outside");
    fs::create_dir_all(&outside_dir).unwrap();
    fs::write(outside_dir.join("escaped.dump"), "escape").unwrap();

    let linked_dir = bundle_root.join("linked");
    symlink(&outside_dir, &linked_dir).unwrap();
    manifest.artifacts.db.path = Some("linked/escaped.dump".to_string());

    let err = manifest
        .resolve_db_artifact_path(&manifest_path)
        .unwrap_err();
    match err {
        SnapshotError::Snapshot(msg) => assert!(msg.contains("bundle root")),
        other => panic!("expected snapshot error, got {other:?}"),
    }
}

#[tokio::test]
async fn rollback_uses_manifest_target_as_source_of_truth() {
    let temp_dir = TempDir::new().unwrap();
    let manifest_path = write_phase1_manifest_fixture(&temp_dir, &valid_db_dump_checksum());
    let restore_log_path = temp_dir.path().join("pg_restore.args");
    let pg_dump = stub_bin(&temp_dir, "pg_dump", "exit 0");
    let pg_restore = stub_bin(
        &temp_dir,
        "pg_restore",
        &format!(
            "log='{}'\n: > \"$log\"\nfor arg in \"$@\"; do\n  printf '%s\\n' \"$arg\" >> \"$log\"\ndone",
            restore_log_path.display()
        ),
    );

    let drifted_config = SupabaseSnapshotConfig {
        require_config_target_match_on_rollback: false,
        ..configured_supabase_snapshot_config(
            "proj_drifted",
            "drifted-db",
            "drifted.supabase.co",
            7777,
            "drifted-user",
        )
    };

    let mut plugin = SupabasePlugin::new(drifted_config, temp_dir.path().join("snapshots"));
    plugin.pg_dump_bin = pg_dump.display().to_string();
    plugin.pg_restore_bin = pg_restore.display().to_string();

    let snapshot_id = SupabasePlugin::build_snapshot_id(&manifest_path);
    plugin.rollback(&snapshot_id).await.unwrap();

    let logged_args = fs::read_to_string(&restore_log_path).unwrap();
    let logged_args: Vec<_> = logged_args.lines().map(str::to_string).collect();

    assert!(logged_args.iter().any(|arg| arg == "--clean"));
    assert!(logged_args.iter().any(|arg| arg == "--if-exists"));
    assert!(logged_args.iter().any(|arg| arg == "--create"));
    assert!(
        logged_args
            .windows(2)
            .any(|window| window[0] == "-h" && window[1] == "db.supabase.co")
    );
    assert!(
        logged_args
            .windows(2)
            .any(|window| window[0] == "-p" && window[1] == "5432")
    );
    assert!(
        logged_args
            .windows(2)
            .any(|window| window[0] == "-U" && window[1] == "postgres")
    );
    assert!(
        logged_args
            .windows(2)
            .any(|window| window[0] == "-d" && window[1] == "postgres")
    );
    // The runtime canonicalizes the resolved artifact path before invoking
    // pg_restore; canonicalize the expected path too so the comparison holds on
    // macOS where `/var` is a symlink to `/private/var`.
    let expected_dump = manifest_path
        .parent()
        .unwrap()
        .join("artifacts/db.dump")
        .canonicalize()
        .unwrap();
    assert_eq!(
        logged_args.last().map(String::as_str),
        Some(expected_dump.to_string_lossy().as_ref())
    );
    assert!(
        !logged_args.iter().any(|arg| arg == "drifted.supabase.co"),
        "pg_restore must not use drifted config target values"
    );
    assert!(
        !logged_args.iter().any(|arg| arg == "7777"),
        "pg_restore must not use drifted config target values"
    );
    assert!(
        !logged_args.iter().any(|arg| arg == "drifted-user"),
        "pg_restore must not use drifted config target values"
    );
}
