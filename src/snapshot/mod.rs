use std::env;
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::config::{
    AegisConfig, DockerScope, MysqlSnapshotConfig, PostgresSnapshotConfig, SnapshotPolicy,
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

type Result<T> = std::result::Result<T, AegisError>;

const BUILTIN_SNAPSHOT_PROVIDER_NAMES: &[&str] =
    &["git", "docker", "postgres", "mysql", "sqlite", "supabase"];

/// Return the names of snapshot providers built into this binary/runtime.
pub fn available_provider_names() -> &'static [&'static str] {
    BUILTIN_SNAPSHOT_PROVIDER_NAMES
}

fn resolve_snapshots_dir() -> Result<PathBuf> {
    let home = home_dir().ok_or_else(|| {
        AegisError::Config(
            "HOME is not set; cannot determine snapshot storage directory".to_string(),
        )
    })?;
    Ok(home.join(".aegis").join("snapshots"))
}

/// Return the user's home directory, checking `HOME` first and falling back to
/// `USERPROFILE` (Windows). Returns `None` when neither is set.
fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
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

pub use aegis_types::SnapshotRecord;

/// A plugin that knows how to snapshot and roll back one kind of state.
#[async_trait]
pub trait SnapshotPlugin: Send + Sync {
    /// Short human-readable name used in logs and the TUI.
    fn name(&self) -> &'static str;

    /// Return `true` when this plugin can act on the given working directory.
    async fn is_applicable(&self, cwd: &Path) -> bool;

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
    /// Global snapshot policy (None, Selective, or Full).
    pub snapshot_policy: SnapshotPolicy,
    /// Enable Git snapshots when true.
    pub auto_snapshot_git: bool,
    /// Enable Docker snapshots when true.
    pub auto_snapshot_docker: bool,
    /// Enable PostgreSQL snapshots when true.
    pub auto_snapshot_postgres: bool,
    /// Connection details for PostgreSQL snapshots.
    pub postgres_snapshot: PostgresSnapshotConfig,
    /// Enable MySQL snapshots when true.
    pub auto_snapshot_mysql: bool,
    /// Connection details for MySQL snapshots.
    pub mysql_snapshot: MysqlSnapshotConfig,
    /// Enable Supabase snapshots when true.
    pub auto_snapshot_supabase: bool,
    /// Connection details for Supabase snapshots.
    pub supabase_snapshot: SupabaseSnapshotConfig,
    /// Enable SQLite snapshots when true.
    pub auto_snapshot_sqlite: bool,
    /// Path to the SQLite database file to snapshot.
    pub sqlite_snapshot_path: String,
    /// Directory where snapshot artifacts are stored.
    pub snapshots_dir: PathBuf,
    /// Docker snapshot scope (image vs container).
    pub docker_scope: DockerScope,
}

fn registry_config_from_parts(
    config: &AegisConfig,
    snapshots_dir: PathBuf,
) -> SnapshotRegistryConfig {
    SnapshotRegistryConfig {
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
        snapshots_dir,
        docker_scope: config.docker_scope.clone(),
    }
}

impl SnapshotRegistryConfig {
    /// Fallible constructor — propagates `HOME`-unset error.
    pub fn try_new(config: &AegisConfig) -> Result<Self> {
        let snapshots_dir = resolve_snapshots_dir()?;
        Ok(registry_config_from_parts(config, snapshots_dir))
    }

    /// Build a rollback runtime config that preserves effective provider
    /// settings while forcing all built-in providers to be available.
    pub fn for_rollback_from_config(config: &AegisConfig) -> Result<Self> {
        let mut runtime_config = Self::try_new(config)?;
        runtime_config.snapshot_policy = crate::config::SnapshotPolicy::Full;
        runtime_config.auto_snapshot_git = true;
        runtime_config.auto_snapshot_docker = true;
        runtime_config.auto_snapshot_postgres = true;
        runtime_config.auto_snapshot_mysql = true;
        runtime_config.auto_snapshot_supabase = true;
        runtime_config.auto_snapshot_sqlite = true;
        Ok(runtime_config)
    }
}

impl SnapshotRegistry {
    /// Fallible constructor that honours the effective runtime config.
    pub fn try_from_config(config: &AegisConfig) -> Result<Self> {
        Ok(Self::from_runtime_config(&SnapshotRegistryConfig::try_new(
            config,
        )?))
    }

    /// Build a snapshot registry from the eager runtime config.
    ///
    /// This materializes the config-filtered set of available snapshot
    /// providers. Applicability remains a later concern evaluated by each
    /// provider for a specific working directory or command.
    pub fn from_runtime_config(config: &SnapshotRegistryConfig) -> Self {
        #[cfg(test)]
        tests::increment_build_count();

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
    pub fn for_rollback() -> Result<Self> {
        Ok(Self::from_runtime_config(
            &SnapshotRegistryConfig::for_rollback_from_config(&AegisConfig::default())?,
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
            if !plugin.is_applicable(cwd).await {
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
    pub async fn applicable_plugins(&self, cwd: &Path) -> Vec<&'static str> {
        let mut names = Vec::new();
        for plugin in &self.plugins {
            if plugin.is_applicable(cwd).await {
                names.push(plugin.name());
            }
        }
        names
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
mod tests;

#[cfg(test)]
pub(crate) use tests::reset_snapshot_registry_build_count_for_tests;
#[cfg(test)]
pub(crate) use tests::snapshot_registry_build_count_for_tests;
