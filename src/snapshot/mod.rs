use std::env;
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::config::{
    Config, DockerScope, MysqlSnapshotConfig, PostgresSnapshotConfig, SnapshotPolicy,
    SupabaseSnapshotConfig,
};
use crate::error::AegisError;

/// Built-in Docker snapshot provider implementation.
pub mod docker;
/// Built-in Git snapshot provider implementation.
pub mod git;
/// Built-in MySQL snapshot provider implementation.
pub mod mysql;
/// Built-in PostgreSQL snapshot provider implementation.
pub mod postgres;
/// Built-in SQLite snapshot provider implementation.
pub mod sqlite;
/// Built-in Supabase snapshot provider implementation.
pub mod supabase;

/// Built-in Docker snapshot provider.
pub use docker::DockerPlugin;
/// Built-in Git snapshot provider.
pub use git::GitPlugin;
/// Built-in MySQL snapshot provider.
pub use mysql::MysqlPlugin;
/// Built-in PostgreSQL snapshot provider.
pub use postgres::PostgresPlugin;
/// Built-in SQLite snapshot provider.
pub use sqlite::SqlitePlugin;
/// Built-in Supabase snapshot provider.
pub use supabase::SupabasePlugin;

#[cfg(test)]
use std::cell::Cell;

type Result<T> = std::result::Result<T, AegisError>;

const BUILTIN_SNAPSHOT_PROVIDER_NAMES: &[&str] =
    &["git", "docker", "postgres", "mysql", "sqlite", "supabase"];

#[cfg(test)]
thread_local! {
    static SNAPSHOT_REGISTRY_BUILD_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_snapshot_registry_build_count_for_tests() {
    SNAPSHOT_REGISTRY_BUILD_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn snapshot_registry_build_count_for_tests() -> usize {
    SNAPSHOT_REGISTRY_BUILD_COUNT.with(Cell::get)
}

/// Return the names of snapshot providers built into this binary/runtime.
pub fn available_provider_names() -> &'static [&'static str] {
    BUILTIN_SNAPSHOT_PROVIDER_NAMES
}

fn default_snapshots_dir() -> PathBuf {
    let home = env::var_os("HOME").unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".aegis").join("snapshots")
}

fn materialize_builtin_plugin(
    name: &str,
    config: &SnapshotRegistryConfig,
) -> Box<dyn SnapshotPlugin> {
    match name {
        "git" => Box::new(GitPlugin),
        "docker" => Box::new(DockerPlugin::new().with_scope(config.docker_scope.clone())),
        "postgres" => Box::new(PostgresPlugin::new(
            config.postgres_snapshot.database.clone(),
            config.postgres_snapshot.host.clone(),
            config.postgres_snapshot.port,
            config.postgres_snapshot.user.clone(),
            config.snapshots_dir.clone(),
        )),
        "mysql" => Box::new(MysqlPlugin::new(
            config.mysql_snapshot.database.clone(),
            config.mysql_snapshot.host.clone(),
            config.mysql_snapshot.port,
            config.mysql_snapshot.user.clone(),
            config.snapshots_dir.clone(),
        )),
        "sqlite" => Box::new(SqlitePlugin::new(
            PathBuf::from(&config.sqlite_snapshot_path),
            config.snapshots_dir.clone(),
        )),
        "supabase" => Box::new(SupabasePlugin::new(
            config.supabase_snapshot.clone(),
            config.snapshots_dir.clone(),
        )),
        _ => panic!("unknown built-in snapshot provider {name:?}"),
    }
}

fn materialize_builtin_plugins(
    names: &[&str],
    config: &SnapshotRegistryConfig,
) -> Vec<Box<dyn SnapshotPlugin>> {
    names
        .iter()
        .map(|name| materialize_builtin_plugin(name, config))
        .collect()
}

/// A record of a single successful snapshot created by one plugin.
#[derive(Debug, Clone)]
pub struct SnapshotRecord {
    /// Name of the plugin that created this snapshot.
    pub plugin: &'static str,
    /// Opaque identifier returned by the plugin (e.g. stash ref, image tag).
    pub snapshot_id: String,
}

/// A plugin that knows how to snapshot and roll back one kind of state.
#[async_trait]
pub trait SnapshotPlugin: Send + Sync {
    /// Short human-readable name used in logs and the TUI.
    fn name(&self) -> &'static str;

    /// Return `true` when this plugin can act on the given working directory.
    fn is_applicable(&self, cwd: &Path) -> bool;

    /// Create a snapshot and return its identifier.
    async fn snapshot(&self, cwd: &Path, cmd: &str) -> Result<String>;

    /// Revert to a previously created snapshot.
    async fn rollback(&self, snapshot_id: &str) -> Result<()>;
}

/// Holds the runtime snapshot provider set used for snapshot and rollback flows.
///
/// Entries may be materialized from the effective runtime config or assembled
/// for broader recovery operations such as rollback. A provider being present
/// here means it is available for later applicability checks, not that it will
/// snapshot every command or in every working directory.
pub struct SnapshotRegistry {
    plugins: Vec<Box<dyn SnapshotPlugin>>,
}

/// Eager runtime snapshot config used to materialize a [`SnapshotRegistry`].
///
/// This captures the config boundary between "which built-in providers are
/// available at runtime" and the later per-command/per-directory applicability
/// checks performed by each provider.
#[derive(Debug, Clone)]
pub struct SnapshotRegistryConfig {
    pub snapshot_policy: SnapshotPolicy,
    pub auto_snapshot_git: bool,
    pub auto_snapshot_docker: bool,
    pub auto_snapshot_postgres: bool,
    pub postgres_snapshot: PostgresSnapshotConfig,
    pub auto_snapshot_mysql: bool,
    pub mysql_snapshot: MysqlSnapshotConfig,
    pub auto_snapshot_supabase: bool,
    pub supabase_snapshot: SupabaseSnapshotConfig,
    pub auto_snapshot_sqlite: bool,
    pub sqlite_snapshot_path: String,
    pub snapshots_dir: PathBuf,
    pub docker_scope: DockerScope,
}

impl From<&Config> for SnapshotRegistryConfig {
    fn from(config: &Config) -> Self {
        Self {
            snapshot_policy: config.snapshot_policy,
            auto_snapshot_git: config.auto_snapshot_git,
            auto_snapshot_docker: config.auto_snapshot_docker,
            auto_snapshot_postgres: config.auto_snapshot_postgres,
            postgres_snapshot: config.postgres_snapshot.clone(),
            auto_snapshot_mysql: config.auto_snapshot_mysql,
            mysql_snapshot: config.mysql_snapshot.clone(),
            auto_snapshot_supabase: config.auto_snapshot_supabase,
            supabase_snapshot: config.supabase_snapshot.clone(),
            auto_snapshot_sqlite: config.auto_snapshot_sqlite,
            sqlite_snapshot_path: config.sqlite_snapshot_path.clone(),
            snapshots_dir: default_snapshots_dir(),
            docker_scope: config.docker_scope.clone(),
        }
    }
}

impl SnapshotRegistryConfig {
    /// Build a rollback runtime config that preserves effective provider
    /// settings while forcing all built-in providers to be available.
    pub fn for_rollback_from_config(config: &Config) -> Self {
        let mut runtime_config = Self::from(config);
        runtime_config.snapshot_policy = crate::config::SnapshotPolicy::Full;
        runtime_config.auto_snapshot_git = true;
        runtime_config.auto_snapshot_docker = true;
        runtime_config.auto_snapshot_postgres = true;
        runtime_config.auto_snapshot_mysql = true;
        runtime_config.auto_snapshot_supabase = true;
        runtime_config.auto_snapshot_sqlite = true;
        runtime_config
    }
}

impl Default for SnapshotRegistry {
    fn default() -> Self {
        Self::from_config(&Config::default())
    }
}

impl SnapshotRegistry {
    /// Build a snapshot registry that honours the effective runtime config.
    pub fn from_config(config: &Config) -> Self {
        Self::from_runtime_config(&SnapshotRegistryConfig::from(config))
    }

    /// Build a snapshot registry from the eager runtime config.
    ///
    /// This materializes the config-filtered set of available snapshot
    /// providers. Applicability remains a later concern evaluated by each
    /// provider for a specific working directory or command.
    pub fn from_runtime_config(config: &SnapshotRegistryConfig) -> Self {
        #[cfg(test)]
        SNAPSHOT_REGISTRY_BUILD_COUNT.with(|count| count.set(count.get() + 1));

        use crate::config::SnapshotPolicy;

        let mut plugins: Vec<Box<dyn SnapshotPlugin>> = Vec::new();

        match config.snapshot_policy {
            SnapshotPolicy::None => { /* no plugins */ }
            SnapshotPolicy::Selective => {
                let enabled_names: Vec<_> = available_provider_names()
                    .iter()
                    .copied()
                    .filter(|name| match *name {
                        "git" => config.auto_snapshot_git,
                        "docker" => config.auto_snapshot_docker,
                        "postgres" => config.auto_snapshot_postgres,
                        "mysql" => config.auto_snapshot_mysql,
                        "supabase" => config.auto_snapshot_supabase,
                        "sqlite" => config.auto_snapshot_sqlite,
                        _ => false,
                    })
                    .collect();
                plugins = materialize_builtin_plugins(&enabled_names, config);
            }
            SnapshotPolicy::Full => {
                plugins = materialize_builtin_plugins(available_provider_names(), config);
            }
        }

        Self { plugins }
    }

    /// Build a registry that can roll back any built-in snapshot type.
    ///
    /// This intentionally ignores per-plugin snapshot flags: operators must be
    /// able to restore previously recorded snapshots even if snapshot creation
    /// is disabled in the current config.
    pub fn for_rollback() -> Self {
        Self::from_runtime_config(&SnapshotRegistryConfig::for_rollback_from_config(
            &Config::default(),
        ))
    }

    /// Return the names of providers materialized into this registry instance.
    ///
    /// For registries built from runtime config, this reports the
    /// config-filtered materialized provider set. For registries built for other
    /// purposes, such as [`SnapshotRegistry::for_rollback`], it reports the
    /// providers materialized for that registry's use.
    pub fn configured_provider_names(&self) -> Vec<&'static str> {
        self.plugins.iter().map(|plugin| plugin.name()).collect()
    }

    /// Call every applicable plugin and collect snapshot records.
    ///
    /// Plugins that are not applicable for `cwd` are skipped silently.
    /// Plugin failures are logged as warnings and do not abort the loop.
    pub async fn snapshot_all(&self, cwd: &Path, cmd: &str) -> Vec<SnapshotRecord> {
        let mut records = Vec::new();
        for plugin in &self.plugins {
            if !plugin.is_applicable(cwd) {
                continue;
            }
            match plugin.snapshot(cwd, cmd).await {
                Ok(snapshot_id) => records.push(SnapshotRecord {
                    plugin: plugin.name(),
                    snapshot_id,
                }),
                Err(e) => {
                    tracing::warn!(plugin = plugin.name(), error = %e, "snapshot failed, continuing");
                }
            }
        }
        records
    }

    /// Return the subset of configured providers that are applicable to `cwd`.
    ///
    /// This is a later-stage runtime-use check than either
    /// [`available_provider_names`] (providers known to the binary/runtime) or
    /// [`SnapshotRegistry::configured_provider_names`] (providers materialized
    /// by the current runtime config). No snapshots are created.
    pub fn applicable_plugins(&self, cwd: &Path) -> Vec<&'static str> {
        self.plugins
            .iter()
            .filter(|plugin| plugin.is_applicable(cwd))
            .map(|plugin| plugin.name())
            .collect()
    }

    /// Roll back one snapshot using the named plugin.
    pub async fn rollback(&self, plugin_name: &str, snapshot_id: &str) -> Result<()> {
        let plugin = self
            .plugins
            .iter()
            .find(|plugin| plugin.name() == plugin_name)
            .ok_or_else(|| {
                AegisError::Snapshot(format!(
                    "snapshot plugin {plugin_name:?} is not available for rollback"
                ))
            })?;

        plugin.rollback(snapshot_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

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

        fn is_applicable(&self, _cwd: &Path) -> bool {
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
            fn is_applicable(&self, _cwd: &Path) -> bool {
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

    #[test]
    fn applicable_plugins_reports_only_applicable_plugin_names() {
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

        let names = registry.applicable_plugins(std::path::Path::new("/tmp"));
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

            fn is_applicable(&self, _cwd: &Path) -> bool {
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
        let config = Config {
            auto_snapshot_git: false,
            auto_snapshot_docker: false,
            ..Config::default()
        };

        let registry = SnapshotRegistry::from_config(&config);

        assert!(registry.plugins.is_empty());
    }

    #[test]
    fn from_config_enables_only_requested_plugins() {
        let config = Config {
            auto_snapshot_git: false,
            auto_snapshot_docker: true,
            ..Config::default()
        };

        let registry = SnapshotRegistry::from_config(&config);
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
        let config = Config {
            auto_snapshot_git: false,
            auto_snapshot_docker: true,
            ..Config::default()
        };

        let registry = SnapshotRegistry::from_config(&config);

        assert_eq!(registry.configured_provider_names(), vec!["docker"]);
    }

    #[test]
    fn for_rollback_materializes_all_builtin_providers() {
        let registry = SnapshotRegistry::for_rollback();

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
            &SnapshotRegistryConfig::from(&Config::default()),
        );
    }

    // ── Snapshot policy tests ───────────────────────────────────────

    #[test]
    fn policy_none_disables_all_plugins() {
        use crate::config::SnapshotPolicy;

        let config = Config {
            snapshot_policy: SnapshotPolicy::None,
            auto_snapshot_git: true,
            auto_snapshot_docker: true,
            ..Config::default()
        };

        let registry = SnapshotRegistry::from_config(&config);
        assert!(
            registry.plugins.is_empty(),
            "None policy must produce zero plugins"
        );
    }

    #[test]
    fn policy_selective_honours_per_plugin_flags() {
        use crate::config::SnapshotPolicy;

        let config = Config {
            snapshot_policy: SnapshotPolicy::Selective,
            auto_snapshot_git: true,
            auto_snapshot_docker: false,
            ..Config::default()
        };

        let registry = SnapshotRegistry::from_config(&config);
        let names: Vec<_> = registry.plugins.iter().map(|p| p.name()).collect();
        assert_eq!(names, vec!["git"]);
    }

    #[test]
    fn selective_policy_enables_postgres_when_configured() {
        use crate::config::SnapshotPolicy;

        let config = Config {
            snapshot_policy: SnapshotPolicy::Selective,
            auto_snapshot_git: false,
            auto_snapshot_docker: false,
            auto_snapshot_postgres: true,
            auto_snapshot_mysql: false,
            auto_snapshot_sqlite: false,
            postgres_snapshot: crate::config::PostgresSnapshotConfig {
                database: "app".to_string(),
                ..crate::config::PostgresSnapshotConfig::default()
            },
            ..Config::default()
        };

        let registry = SnapshotRegistry::from_config(&config);
        let names: Vec<_> = registry.plugins.iter().map(|p| p.name()).collect();

        assert_eq!(names, vec!["postgres"]);
    }

    #[test]
    fn selective_policy_disables_postgres_when_flag_off() {
        use crate::config::SnapshotPolicy;

        let config = Config {
            snapshot_policy: SnapshotPolicy::Selective,
            auto_snapshot_git: false,
            auto_snapshot_docker: false,
            auto_snapshot_postgres: false,
            auto_snapshot_mysql: false,
            auto_snapshot_sqlite: false,
            postgres_snapshot: crate::config::PostgresSnapshotConfig {
                database: "app".to_string(),
                ..crate::config::PostgresSnapshotConfig::default()
            },
            ..Config::default()
        };

        let registry = SnapshotRegistry::from_config(&config);
        let names: Vec<_> = registry.plugins.iter().map(|p| p.name()).collect();

        assert!(!names.contains(&"postgres"));
    }

    #[test]
    fn policy_full_enables_supabase_plugin() {
        use crate::config::SnapshotPolicy;

        let config = Config {
            snapshot_policy: SnapshotPolicy::Full,
            auto_snapshot_git: false,
            auto_snapshot_docker: false,
            auto_snapshot_postgres: false,
            auto_snapshot_mysql: false,
            auto_snapshot_sqlite: false,
            auto_snapshot_supabase: false,
            ..Config::default()
        };

        let registry = SnapshotRegistry::from_config(&config);
        let names: Vec<_> = registry.plugins.iter().map(|p| p.name()).collect();
        assert_eq!(
            names,
            vec!["git", "docker", "postgres", "mysql", "sqlite", "supabase"]
        );
    }

    #[test]
    fn for_rollback_includes_supabase_plugin() {
        let registry = SnapshotRegistry::for_rollback();

        assert_eq!(
            registry.configured_provider_names(),
            vec!["git", "docker", "postgres", "mysql", "sqlite", "supabase"]
        );
    }

    #[test]
    fn rollback_runtime_config_preserves_supabase_settings_and_forces_provider() {
        let config = Config {
            auto_snapshot_git: false,
            auto_snapshot_docker: false,
            auto_snapshot_postgres: false,
            auto_snapshot_mysql: false,
            auto_snapshot_supabase: false,
            auto_snapshot_sqlite: false,
            postgres_snapshot: crate::config::PostgresSnapshotConfig {
                database: "pg-app".to_string(),
                host: "pg.internal".to_string(),
                port: 5544,
                user: "pguser".to_string(),
            },
            mysql_snapshot: crate::config::MysqlSnapshotConfig {
                database: "mysql-app".to_string(),
                host: "mysql.internal".to_string(),
                port: 4407,
                user: "mysqluser".to_string(),
            },
            supabase_snapshot: crate::config::SupabaseSnapshotConfig {
                project_ref: "proj_123".to_string(),
                db: crate::config::PostgresSnapshotConfig {
                    database: "postgres".to_string(),
                    host: "db.supabase.co".to_string(),
                    port: 6543,
                    user: "postgres".to_string(),
                },
                ..crate::config::SupabaseSnapshotConfig::default()
            },
            sqlite_snapshot_path: "db/app.sqlite".to_string(),
            ..Config::default()
        };

        let runtime_config = SnapshotRegistryConfig::for_rollback_from_config(&config);

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

        let config = Config {
            snapshot_policy: SnapshotPolicy::Selective,
            auto_snapshot_git: false,
            auto_snapshot_docker: false,
            auto_snapshot_postgres: false,
            auto_snapshot_mysql: false,
            auto_snapshot_sqlite: false,
            auto_snapshot_supabase: true,
            supabase_snapshot: crate::config::SupabaseSnapshotConfig {
                db: crate::config::PostgresSnapshotConfig {
                    database: "postgres".to_string(),
                    ..crate::config::PostgresSnapshotConfig::default()
                },
                ..crate::config::SupabaseSnapshotConfig::default()
            },
            ..Config::default()
        };

        let registry = SnapshotRegistry::from_config(&config);

        assert_eq!(registry.configured_provider_names(), vec!["supabase"]);
    }

    #[test]
    fn sqlite_relative_snapshot_path_is_applicable_from_command_cwd() {
        let temp_dir = TempDir::new().unwrap();
        let db_dir = temp_dir.path().join("db");
        std::fs::create_dir_all(&db_dir).unwrap();
        std::fs::write(db_dir.join("app.db"), b"sqlite-data").unwrap();

        let config = Config {
            auto_snapshot_git: false,
            auto_snapshot_docker: false,
            auto_snapshot_postgres: false,
            auto_snapshot_mysql: false,
            auto_snapshot_sqlite: true,
            sqlite_snapshot_path: "db/app.db".to_string(),
            ..Config::default()
        };

        let registry = SnapshotRegistry::from_config(&config);

        assert_eq!(registry.applicable_plugins(temp_dir.path()), vec!["sqlite"]);
    }
}
