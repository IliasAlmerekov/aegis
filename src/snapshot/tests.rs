use std::cell::Cell;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use tempfile::TempDir;

use super::*;

thread_local! {
    pub(crate) static SNAPSHOT_REGISTRY_BUILD_COUNT: Cell<usize> = const { Cell::new(0) };
}

pub(crate) fn reset_snapshot_registry_build_count_for_tests() {
    SNAPSHOT_REGISTRY_BUILD_COUNT.with(|count| count.set(0));
}

pub(crate) fn snapshot_registry_build_count_for_tests() -> usize {
    SNAPSHOT_REGISTRY_BUILD_COUNT.with(Cell::get)
}

pub(crate) fn increment_build_count() {
    SNAPSHOT_REGISTRY_BUILD_COUNT.with(|count| count.set(count.get() + 1));
}

// Serialises tests that read or mutate HOME/USERPROFILE so they don't race.
// tokio::sync::Mutex is used so async tests can hold the guard across .await points
// without triggering the clippy::await_holding_lock lint.
static HOME_ENV: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

struct MockPlugin {
    name: &'static str,
    applicable: bool,
    call_count: Arc<AtomicUsize>,
}

#[async_trait]
impl SnapshotPlugin for MockPlugin {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn is_applicable(&self, _cwd: &Path) -> bool {
        self.applicable
    }

    async fn snapshot(&self, _cwd: &Path, _cmd: &str) -> Result<String> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok("mock-snap-001".to_string())
    }

    async fn rollback(&self, _snapshot_id: &str) -> Result<()> {
        Ok(())
    }
}

/// Registry must only call `snapshot` on plugins where `is_applicable` returns true.
#[tokio::test]
async fn snapshot_all_skips_non_applicable_plugins() {
    let applicable_calls = Arc::new(AtomicUsize::new(0));
    let skipped_calls = Arc::new(AtomicUsize::new(0));

    let registry = SnapshotRegistry {
        plugins: vec![
            Box::new(MockPlugin {
                name: "applicable-1",
                applicable: true,
                call_count: Arc::clone(&applicable_calls),
            }),
            Box::new(MockPlugin {
                name: "skipped",
                applicable: false,
                call_count: Arc::clone(&skipped_calls),
            }),
            Box::new(MockPlugin {
                name: "applicable-2",
                applicable: true,
                call_count: Arc::clone(&applicable_calls),
            }),
        ],
    };

    let cwd = std::path::PathBuf::from("/tmp");
    let records = registry.snapshot_all(&cwd, "rm -rf /").await;

    // Both applicable plugins should have produced a record.
    assert_eq!(records.len(), 2);
    assert_eq!(applicable_calls.load(Ordering::SeqCst), 2);

    // The non-applicable plugin must never have its snapshot called.
    assert_eq!(skipped_calls.load(Ordering::SeqCst), 0);
}

/// A plugin that returns an error should not abort other plugins.
#[tokio::test]
async fn snapshot_all_continues_after_plugin_error() {
    struct FailingPlugin;

    #[async_trait]
    impl SnapshotPlugin for FailingPlugin {
        fn name(&self) -> &'static str {
            "failing"
        }
        async fn is_applicable(&self, _cwd: &Path) -> bool {
            true
        }
        async fn snapshot(&self, _cwd: &Path, _cmd: &str) -> Result<String> {
            Err(AegisError::Snapshot("simulated failure".to_string()))
        }
        async fn rollback(&self, _snapshot_id: &str) -> Result<()> {
            Ok(())
        }
    }

    let success_calls = Arc::new(AtomicUsize::new(0));

    let registry = SnapshotRegistry {
        plugins: vec![
            Box::new(FailingPlugin),
            Box::new(MockPlugin {
                name: "success",
                applicable: true,
                call_count: Arc::clone(&success_calls),
            }),
        ],
    };

    let cwd = std::path::PathBuf::from("/tmp");
    let records = registry.snapshot_all(&cwd, "rm -rf /").await;

    // Only the successful plugin produces a record.
    assert_eq!(records.len(), 1);
    assert_eq!(success_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn applicable_plugins_reports_only_applicable_plugin_names() {
    let registry = SnapshotRegistry {
        plugins: vec![
            Box::new(MockPlugin {
                name: "git",
                applicable: true,
                call_count: Arc::new(AtomicUsize::new(0)),
            }),
            Box::new(MockPlugin {
                name: "docker",
                applicable: false,
                call_count: Arc::new(AtomicUsize::new(0)),
            }),
        ],
    };

    let names = registry
        .applicable_plugins(std::path::Path::new("/tmp"))
        .await;
    assert_eq!(names, vec!["git"]);
}

#[tokio::test]
async fn rollback_routes_to_named_plugin() {
    let rollback_calls = Arc::new(AtomicUsize::new(0));

    struct RollbackOnlyPlugin {
        name: &'static str,
        rollback_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl SnapshotPlugin for RollbackOnlyPlugin {
        fn name(&self) -> &'static str {
            self.name
        }

        async fn is_applicable(&self, _cwd: &Path) -> bool {
            true
        }

        async fn snapshot(&self, _cwd: &Path, _cmd: &str) -> Result<String> {
            Ok("unused".to_string())
        }

        async fn rollback(&self, _snapshot_id: &str) -> Result<()> {
            self.rollback_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    let registry = SnapshotRegistry {
        plugins: vec![
            Box::new(RollbackOnlyPlugin {
                name: "git",
                rollback_calls: Arc::clone(&rollback_calls),
            }),
            Box::new(RollbackOnlyPlugin {
                name: "docker",
                rollback_calls: Arc::new(AtomicUsize::new(0)),
            }),
        ],
    };

    registry.rollback("git", "snap-001").await.unwrap();
    assert_eq!(rollback_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn rollback_errors_for_unknown_plugin() {
    let registry = SnapshotRegistry {
        plugins: Vec::new(),
    };

    let err = registry
        .rollback("missing-plugin", "snap-001")
        .await
        .expect_err("unknown plugin must fail");

    assert!(
        err.to_string().contains("missing-plugin"),
        "error should name the missing plugin: {err}"
    );
}

#[test]
fn from_config_disables_all_plugins_when_snapshots_are_off() {
    let mut config = AegisConfig::default();
    config.auto_snapshot_git = false;
    config.auto_snapshot_docker = false;

    let registry = SnapshotRegistry::try_from_config(&config).unwrap();

    assert!(registry.plugins.is_empty());
}

#[test]
fn from_config_enables_only_requested_plugins() {
    let mut config = AegisConfig::default();
    config.auto_snapshot_git = false;
    config.auto_snapshot_docker = true;

    let registry = SnapshotRegistry::try_from_config(&config).unwrap();
    let plugin_names: Vec<_> = registry
        .plugins
        .iter()
        .map(|plugin| plugin.name())
        .collect();

    assert_eq!(plugin_names, vec!["docker"]);
}

#[test]
fn available_provider_names_include_db_plugins() {
    assert_eq!(
        available_provider_names(),
        ["git", "docker", "postgres", "mysql", "sqlite", "supabase"]
    );
}

#[test]
fn configured_provider_names_report_only_materialized_runtime_plugins() {
    let mut config = AegisConfig::default();
    config.auto_snapshot_git = false;
    config.auto_snapshot_docker = true;

    let registry = SnapshotRegistry::try_from_config(&config).unwrap();

    assert_eq!(registry.configured_provider_names(), vec!["docker"]);
}

#[test]
fn for_rollback_materializes_all_builtin_providers() {
    let registry = SnapshotRegistry::for_rollback().unwrap();

    assert_eq!(
        registry.configured_provider_names(),
        available_provider_names().to_vec()
    );
}

#[test]
#[should_panic(expected = "unknown built-in snapshot provider")]
fn materialize_builtin_plugins_fails_closed_for_unknown_builtin_name() {
    let _ = materialize_builtin_plugins(
        &["git", "unknown-provider"],
        &SnapshotRegistryConfig::try_new(&AegisConfig::default()).unwrap(),
    );
}

// ── Snapshot policy tests ───────────────────────────────────────

#[test]
fn policy_none_disables_all_plugins() {
    use crate::config::SnapshotPolicy;

    let mut config = AegisConfig::default();
    config.snapshot_policy = SnapshotPolicy::None;
    config.auto_snapshot_git = true;
    config.auto_snapshot_docker = true;

    let registry = SnapshotRegistry::try_from_config(&config).unwrap();
    assert!(
        registry.plugins.is_empty(),
        "None policy must produce zero plugins"
    );
}

#[test]
fn policy_selective_honours_per_plugin_flags() {
    use crate::config::SnapshotPolicy;

    let mut config = AegisConfig::default();
    config.snapshot_policy = SnapshotPolicy::Selective;
    config.auto_snapshot_git = true;
    config.auto_snapshot_docker = false;

    let registry = SnapshotRegistry::try_from_config(&config).unwrap();
    let names: Vec<_> = registry.plugins.iter().map(|p| p.name()).collect();
    assert_eq!(names, vec!["git"]);
}

#[test]
fn selective_policy_enables_postgres_when_configured() {
    use crate::config::SnapshotPolicy;

    let mut config = AegisConfig::default();
    config.snapshot_policy = SnapshotPolicy::Selective;
    config.auto_snapshot_git = false;
    config.auto_snapshot_docker = false;
    config.auto_snapshot_postgres = true;
    config.auto_snapshot_mysql = false;
    config.auto_snapshot_sqlite = false;
    config.postgres_snapshot = crate::config::PostgresSnapshotConfig {
        database: "app".to_string(),
        ..crate::config::PostgresSnapshotConfig::default()
    };

    let registry = SnapshotRegistry::try_from_config(&config).unwrap();
    let names: Vec<_> = registry.plugins.iter().map(|p| p.name()).collect();

    assert_eq!(names, vec!["postgres"]);
}

#[test]
fn selective_policy_disables_postgres_when_flag_off() {
    use crate::config::SnapshotPolicy;

    let mut config = AegisConfig::default();
    config.snapshot_policy = SnapshotPolicy::Selective;
    config.auto_snapshot_git = false;
    config.auto_snapshot_docker = false;
    config.auto_snapshot_postgres = false;
    config.auto_snapshot_mysql = false;
    config.auto_snapshot_sqlite = false;
    config.postgres_snapshot = crate::config::PostgresSnapshotConfig {
        database: "app".to_string(),
        ..crate::config::PostgresSnapshotConfig::default()
    };

    let registry = SnapshotRegistry::try_from_config(&config).unwrap();
    let names: Vec<_> = registry.plugins.iter().map(|p| p.name()).collect();

    assert!(!names.contains(&"postgres"));
}

#[test]
fn policy_full_enables_supabase_plugin() {
    use crate::config::SnapshotPolicy;

    let mut config = AegisConfig::default();
    config.snapshot_policy = SnapshotPolicy::Full;
    config.auto_snapshot_git = false;
    config.auto_snapshot_docker = false;
    config.auto_snapshot_postgres = false;
    config.auto_snapshot_mysql = false;
    config.auto_snapshot_sqlite = false;
    config.auto_snapshot_supabase = false;

    let registry = SnapshotRegistry::try_from_config(&config).unwrap();
    let names: Vec<_> = registry.plugins.iter().map(|p| p.name()).collect();
    assert_eq!(
        names,
        vec!["git", "docker", "postgres", "mysql", "sqlite", "supabase"]
    );
}

#[test]
fn for_rollback_includes_supabase_plugin() {
    let registry = SnapshotRegistry::for_rollback().unwrap();

    assert_eq!(
        registry.configured_provider_names(),
        vec!["git", "docker", "postgres", "mysql", "sqlite", "supabase"]
    );
}

#[test]
fn rollback_runtime_config_preserves_supabase_settings_and_forces_provider() {
    let mut config = AegisConfig::default();
    config.auto_snapshot_git = false;
    config.auto_snapshot_docker = false;
    config.auto_snapshot_postgres = false;
    config.auto_snapshot_mysql = false;
    config.auto_snapshot_supabase = false;
    config.auto_snapshot_sqlite = false;
    config.postgres_snapshot = crate::config::PostgresSnapshotConfig {
        database: "pg-app".to_string(),
        host: "pg.internal".to_string(),
        port: 5544,
        user: "pguser".to_string(),
    };
    config.mysql_snapshot = crate::config::MysqlSnapshotConfig {
        database: "mysql-app".to_string(),
        host: "mysql.internal".to_string(),
        port: 4407,
        user: "mysqluser".to_string(),
    };
    config.supabase_snapshot = crate::config::SupabaseSnapshotConfig {
        project_ref: "proj_123".to_string(),
        db: crate::config::PostgresSnapshotConfig {
            database: "postgres".to_string(),
            host: "db.supabase.co".to_string(),
            port: 6543,
            user: "postgres".to_string(),
        },
        ..crate::config::SupabaseSnapshotConfig::default()
    };
    config.sqlite_snapshot_path = "db/app.sqlite".to_string();

    let runtime_config = SnapshotRegistryConfig::for_rollback_from_config(&config).unwrap();

    assert_eq!(
        runtime_config.snapshot_policy,
        crate::config::SnapshotPolicy::Full
    );
    assert!(runtime_config.auto_snapshot_git);
    assert!(runtime_config.auto_snapshot_docker);
    assert!(runtime_config.auto_snapshot_postgres);
    assert!(runtime_config.auto_snapshot_mysql);
    assert!(runtime_config.auto_snapshot_supabase);
    assert!(runtime_config.auto_snapshot_sqlite);
    assert_eq!(runtime_config.postgres_snapshot, config.postgres_snapshot);
    assert_eq!(runtime_config.mysql_snapshot, config.mysql_snapshot);
    assert_eq!(runtime_config.supabase_snapshot, config.supabase_snapshot);
    assert_eq!(
        runtime_config.sqlite_snapshot_path,
        config.sqlite_snapshot_path
    );
}

#[test]
fn selective_policy_enables_supabase_when_configured() {
    use crate::config::SnapshotPolicy;

    let mut config = AegisConfig::default();
    config.snapshot_policy = SnapshotPolicy::Selective;
    config.auto_snapshot_git = false;
    config.auto_snapshot_docker = false;
    config.auto_snapshot_postgres = false;
    config.auto_snapshot_mysql = false;
    config.auto_snapshot_sqlite = false;
    config.auto_snapshot_supabase = true;
    config.supabase_snapshot = crate::config::SupabaseSnapshotConfig {
        db: crate::config::PostgresSnapshotConfig {
            database: "postgres".to_string(),
            ..crate::config::PostgresSnapshotConfig::default()
        },
        ..crate::config::SupabaseSnapshotConfig::default()
    };

    let registry = SnapshotRegistry::try_from_config(&config).unwrap();

    assert_eq!(registry.configured_provider_names(), vec!["supabase"]);
}

#[test]
fn try_from_config_fails_when_home_is_unset() {
    // SAFETY: this test mutates env vars. Remove both HOME and USERPROFILE
    // (the Windows equivalent) so that home_dir() returns None on all
    // platforms. Restore both before asserting so a failure doesn't poison
    // the environment for parallel tests.
    let _guard = HOME_ENV.blocking_lock();
    let saved_home = std::env::var_os("HOME");
    let saved_userprofile = std::env::var_os("USERPROFILE");
    unsafe {
        std::env::remove_var("HOME");
        std::env::remove_var("USERPROFILE");
    }

    let result = SnapshotRegistryConfig::try_new(&AegisConfig::default());

    unsafe {
        if let Some(val) = saved_home {
            std::env::set_var("HOME", val);
        }
        if let Some(val) = saved_userprofile {
            std::env::set_var("USERPROFILE", val);
        }
    }

    let err = result.expect_err("TryFrom must fail when HOME is unset");
    assert!(
        err.to_string().contains("HOME is not set"),
        "error must name the missing variable: {err}"
    );
}

#[tokio::test]
async fn sqlite_relative_snapshot_path_is_applicable_from_command_cwd() {
    let _guard = HOME_ENV.lock().await;
    let temp_dir = TempDir::new().unwrap();
    let db_dir = temp_dir.path().join("db");
    std::fs::create_dir_all(&db_dir).unwrap();
    std::fs::write(db_dir.join("app.db"), b"sqlite-data").unwrap();

    let mut config = AegisConfig::default();
    config.auto_snapshot_git = false;
    config.auto_snapshot_docker = false;
    config.auto_snapshot_postgres = false;
    config.auto_snapshot_mysql = false;
    config.auto_snapshot_sqlite = true;
    config.sqlite_snapshot_path = "db/app.db".to_string();

    let registry = SnapshotRegistry::try_from_config(&config).unwrap();

    assert_eq!(
        registry.applicable_plugins(temp_dir.path()).await,
        vec!["sqlite"]
    );
}
