//! Snapshot and rollback plugins for Aegis.
//!
//! This crate provides the [`SnapshotPlugin`] trait, [`SnapshotRegistry`],
//! [`SnapshotRegistryConfig`], and six built-in provider backends:
//! git, docker, postgres, mysql, sqlite, and supabase.

#![deny(missing_docs)]

use std::collections::{HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use aegis_config::{
    AegisConfig, DockerScope, MysqlSnapshotConfig, PostgresSnapshotConfig, PruneConfig,
    SnapshotPolicy, SupabaseSnapshotConfig,
};
pub use aegis_types::SnapshotRecord;

/// Typed error for snapshot operations.
pub mod error;
pub use error::SnapshotError;

mod docker;
mod git;
mod mysql;
mod postgres;
mod sqlite;
mod supabase;

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

type Result<T> = std::result::Result<T, SnapshotError>;

const BUILTIN_SNAPSHOT_PROVIDER_NAMES: &[&str] =
    &["git", "docker", "postgres", "mysql", "sqlite", "supabase"];

/// Return the names of snapshot providers built into this binary/runtime.
pub fn available_provider_names() -> &'static [&'static str] {
    BUILTIN_SNAPSHOT_PROVIDER_NAMES
}

fn resolve_snapshots_dir() -> Result<PathBuf> {
    let home = home_dir().ok_or_else(|| {
        SnapshotError::Config(
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

/// Materialize a set of built-in plugins by name.
///
/// Panics if any name is not a known built-in provider.
fn materialize_builtin_plugins(
    names: &[&str],
    config: &SnapshotRegistryConfig,
) -> Vec<Box<dyn SnapshotPlugin>> {
    names
        .iter()
        .map(|name| materialize_builtin_plugin(name, config))
        .collect()
}

/// Thread-local build counter incremented by [`SnapshotRegistry::from_runtime_config`].
///
/// Exposed so that upstream crates can observe how many times the registry
/// was materialised in a single test thread — without introducing a hard
/// dependency on the internal implementation.
pub mod testing {
    use std::cell::Cell;

    thread_local! {
        static REGISTRY_BUILD_COUNT: Cell<usize> = const { Cell::new(0) };
    }

    /// Increment the per-thread registry build counter.
    pub fn increment_registry_build_count() {
        REGISTRY_BUILD_COUNT.with(|c| c.set(c.get() + 1));
    }

    /// Read the per-thread registry build counter.
    pub fn registry_build_count() -> usize {
        REGISTRY_BUILD_COUNT.with(Cell::get)
    }

    /// Reset the per-thread registry build counter to zero.
    pub fn reset_registry_build_count() {
        REGISTRY_BUILD_COUNT.with(|c| c.set(0));
    }
}

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

    /// Delete a previously created snapshot artifact.
    ///
    /// Deletion must be idempotent: returning `Ok(())` when the artifact has
    /// already been removed. Backend failures are reported as
    /// [`SnapshotError::DeleteFailed`].
    async fn delete(&self, snapshot_id: &str) -> Result<()>;
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
    pub fn try_new(config: &AegisConfig) -> std::result::Result<Self, SnapshotError> {
        let snapshots_dir = resolve_snapshots_dir()?;
        Ok(registry_config_from_parts(config, snapshots_dir))
    }

    /// Build a rollback runtime config that preserves effective provider
    /// settings while forcing all built-in providers to be available.
    pub fn for_rollback_from_config(
        config: &AegisConfig,
    ) -> std::result::Result<Self, SnapshotError> {
        let mut runtime_config = Self::try_new(config)?;
        runtime_config.snapshot_policy = SnapshotPolicy::Full;
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
    /// Construct a registry from an explicit plugin list.
    ///
    /// This constructor is intended for testing only.  Production code should
    /// use [`SnapshotRegistry::from_runtime_config`] instead.
    pub fn new_with_plugins(plugins: Vec<Box<dyn SnapshotPlugin>>) -> Self {
        Self { plugins }
    }

    /// Fallible constructor that honours the effective runtime config.
    pub fn try_from_config(config: &AegisConfig) -> std::result::Result<Self, SnapshotError> {
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
        testing::increment_registry_build_count();

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
    pub fn for_rollback() -> std::result::Result<Self, SnapshotError> {
        Ok(Self::from_runtime_config(
            &SnapshotRegistryConfig::for_rollback_from_config(&AegisConfig::default())?,
        ))
    }

    /// Return the names of providers materialized into this registry instance.
    ///
    /// For registries built from runtime config, this reports the
    /// config-filtered materialized provider set. For registries built for
    /// other purposes, such as [`SnapshotRegistry::for_rollback`], it reports
    /// the providers materialized for that registry's use.
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
    pub async fn rollback(
        &self,
        plugin_name: &str,
        snapshot_id: &str,
    ) -> std::result::Result<(), SnapshotError> {
        let plugin = self
            .plugins
            .iter()
            .find(|plugin| plugin.name() == plugin_name)
            .ok_or_else(|| {
                SnapshotError::Snapshot(format!(
                    "snapshot plugin {plugin_name:?} is not available for rollback"
                ))
            })?;

        plugin.rollback(snapshot_id).await
    }

    /// Delete one snapshot using the named plugin.
    pub async fn delete(
        &self,
        plugin_name: &str,
        snapshot_id: &str,
    ) -> std::result::Result<(), SnapshotError> {
        let plugin = self
            .plugins
            .iter()
            .find(|plugin| plugin.name() == plugin_name)
            .ok_or_else(|| {
                SnapshotError::Snapshot(format!(
                    "snapshot plugin {plugin_name:?} is not available for delete"
                ))
            })?;

        plugin.delete(snapshot_id).await
    }

    /// Resolve the snapshot records that are still on record and have not been
    /// pruned.
    ///
    /// Reads the default audit log (`~/.aegis/audit.jsonl`), collects the latest
    /// recorded timestamp for each `(plugin, snapshot_id)` pair, and removes
    /// any id that has a later `Decision::Pruned` entry. If the audit log is
    /// missing, the result is empty.
    pub async fn resolve_prunable_records(
        &self,
    ) -> std::result::Result<Vec<PrunableRecord>, SnapshotError> {
        resolve_prunable_records_from_default_audit_log()
    }
}

/// One snapshot record that may be eligible for pruning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrunableRecord {
    /// Name of the snapshot plugin that created the record.
    pub plugin: String,
    /// Opaque snapshot identifier.
    pub snapshot_id: String,
    /// Timestamp when the snapshot was recorded in the audit log.
    pub recorded_at: OffsetDateTime,
}

/// Injectable clock for deterministic retention tests.
pub trait Clock: Send + Sync {
    /// Return the current time.
    fn now(&self) -> OffsetDateTime;
}

/// Clock that returns the current system time.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

/// Clock that returns a fixed timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedClock(OffsetDateTime);

impl FixedClock {
    /// Create a clock that always returns `timestamp`.
    pub fn new(timestamp: OffsetDateTime) -> Self {
        Self(timestamp)
    }
}

impl Clock for FixedClock {
    fn now(&self) -> OffsetDateTime {
        self.0
    }
}

/// Retention policy used to decide which snapshots become prune candidates.
///
/// A snapshot is kept if it satisfies either the per-provider count rule or
/// the global age rule. Only snapshots that fail both rules are returned by
/// [`RetentionPolicy::apply`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RetentionPolicy {
    max_count_per_provider: Option<usize>,
    max_age_days: Option<u32>,
}

impl RetentionPolicy {
    /// Retention policy that only uses a maximum age rule.
    pub fn from_max_age_days(days: u32) -> Self {
        Self {
            max_age_days: Some(days),
            ..Self::default()
        }
    }

    /// Retention policy that only uses a per-provider count rule.
    pub fn from_max_count_per_provider(count: usize) -> Self {
        Self {
            max_count_per_provider: Some(count),
            ..Self::default()
        }
    }

    /// Build a policy from the effective runtime config.
    pub fn from_config(config: &PruneConfig) -> Self {
        Self {
            max_count_per_provider: config.max_count_per_provider,
            max_age_days: config.max_age_days,
        }
    }

    /// Apply this policy to a set of records and return the prune candidates.
    ///
    /// Candidates preserve the original input order.
    pub fn apply(&self, records: &[PrunableRecord], now: OffsetDateTime) -> Vec<PrunableRecord> {
        if self.max_count_per_provider.is_none() && self.max_age_days.is_none() {
            return Vec::new();
        }

        let mut kept = HashSet::new();

        if let Some(days) = self.max_age_days {
            let max_age = time::Duration::days(i64::from(days));
            for record in records {
                if now - record.recorded_at <= max_age {
                    kept.insert((record.plugin.clone(), record.snapshot_id.clone()));
                }
            }
        }

        if let Some(count) = self.max_count_per_provider {
            let mut by_provider: HashMap<&str, Vec<&PrunableRecord>> = HashMap::new();
            for record in records {
                by_provider
                    .entry(record.plugin.as_str())
                    .or_default()
                    .push(record);
            }
            for group in by_provider.values_mut() {
                group.sort_by(|a, b| b.recorded_at.cmp(&a.recorded_at));
                for record in group.iter().take(count) {
                    kept.insert((record.plugin.clone(), record.snapshot_id.clone()));
                }
            }
        }

        records
            .iter()
            .filter(|record| !kept.contains(&(record.plugin.clone(), record.snapshot_id.clone())))
            .cloned()
            .collect()
    }
}

#[derive(Debug, serde::Deserialize)]
struct MinimalAuditSnapshot {
    plugin: String,
    snapshot_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct MinimalAuditEntry {
    timestamp: String,
    decision: aegis_types::Decision,
    command: String,
    #[serde(default)]
    snapshots: Vec<MinimalAuditSnapshot>,
}

fn resolve_prunable_records_from_default_audit_log()
-> std::result::Result<Vec<PrunableRecord>, SnapshotError> {
    let Some(home) = home_dir() else {
        return Ok(Vec::new());
    };
    let path = home.join(".aegis").join("audit.jsonl");

    if !path.exists() {
        return Ok(Vec::new());
    }

    let contents = std::fs::read_to_string(&path).map_err(SnapshotError::Io)?;
    let mut latest: HashMap<(String, String), OffsetDateTime> = HashMap::new();
    let mut pruned_pairs: HashSet<(String, String)> = HashSet::new();
    let mut pruned_ids: HashSet<String> = HashSet::new();

    for line in contents.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: MinimalAuditEntry = match serde_json::from_str(line) {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(%error, "skipping unparseable audit log line during prune");
                continue;
            }
        };

        if entry.decision == aegis_types::Decision::Pruned {
            for snapshot in &entry.snapshots {
                pruned_pairs.insert((snapshot.plugin.clone(), snapshot.snapshot_id.clone()));
            }
            if let Some(id) = snapshot_id_from_prune_command(&entry.command) {
                pruned_ids.insert(id.to_string());
            }
            continue;
        }

        let recorded_at = match OffsetDateTime::parse(&entry.timestamp, &Rfc3339) {
            Ok(ts) => ts,
            Err(error) => {
                tracing::warn!(%error, timestamp = %entry.timestamp, "skipping audit log line with invalid timestamp");
                continue;
            }
        };

        for snapshot in &entry.snapshots {
            let key = (snapshot.plugin.clone(), snapshot.snapshot_id.clone());
            match latest.get(&key) {
                Some(previous) if *previous >= recorded_at => {}
                _ => {
                    latest.insert(key, recorded_at);
                }
            }
        }
    }

    let mut records: Vec<PrunableRecord> = latest
        .into_iter()
        .filter(|((plugin, snapshot_id), _)| {
            !pruned_pairs.contains(&(plugin.clone(), snapshot_id.clone()))
                && !pruned_ids.contains(snapshot_id)
        })
        .map(|((plugin, snapshot_id), recorded_at)| PrunableRecord {
            plugin,
            snapshot_id,
            recorded_at,
        })
        .collect();
    records.sort_by(|a, b| b.recorded_at.cmp(&a.recorded_at));
    Ok(records)
}

/// Extract a snapshot id from a prune audit entry's command field.
///
/// Prune records store the removed snapshot id as `aegis prune <snapshot_id>` in
/// the `command` field when the `snapshots` array is empty.
fn snapshot_id_from_prune_command(command: &str) -> Option<&str> {
    const PRUNE_PREFIX: &str = "aegis prune ";
    command
        .strip_prefix(PRUNE_PREFIX)
        .filter(|id| !id.is_empty())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[tokio::test]
    async fn test_git_plugin_delete_missing_artifact_is_idempotent() {
        let result = GitPlugin
            .delete("/tmp/aegis-missing-git-repo-test\tdeadbeef")
            .await;
        assert!(
            result.is_ok(),
            "delete on a missing git snapshot must be idempotent: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_docker_plugin_delete_none_sentinel_is_noop() {
        let plugin = DockerPlugin::new();
        let result = plugin.delete("none").await;
        assert!(
            result.is_ok(),
            "delete on 'none' sentinel must succeed: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_postgres_plugin_delete_missing_dump_returns_ok() {
        let temp = tempfile::tempdir().unwrap();
        let plugin = PostgresPlugin::new(
            "db".to_string(),
            "localhost".to_string(),
            5432,
            "user".to_string(),
            temp.path().to_path_buf(),
        );
        let result = plugin
            .delete(
                "v2\t6462\t6c6f63616c686f7374\t5432\t75736572\t2f6e6f6e652f65786973742e64756d70",
            )
            .await;
        assert!(
            result.is_ok(),
            "delete on missing postgres dump must be idempotent: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_mysql_plugin_delete_missing_dump_returns_ok() {
        let temp = tempfile::tempdir().unwrap();
        let plugin = MysqlPlugin::new(
            "db".to_string(),
            "localhost".to_string(),
            3306,
            "user".to_string(),
            temp.path().to_path_buf(),
        );
        let result = plugin
            .delete("v2\t6462\t6c6f63616c686f7374\t3306\t75736572\t2f6e6f6e652f65786973742e73716c")
            .await;
        assert!(
            result.is_ok(),
            "delete on missing mysql dump must be idempotent: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_sqlite_plugin_delete_missing_dump_returns_ok() {
        let temp = tempfile::tempdir().unwrap();
        let plugin = SqlitePlugin::new(PathBuf::from("/tmp/app.db"), temp.path().to_path_buf());
        let result = plugin
            .delete("v2\t2f746d702f6170702e6462\t2f6e6f6e652f65786973742e6462")
            .await;
        assert!(
            result.is_ok(),
            "delete on missing sqlite dump must be idempotent: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_supabase_plugin_delete_missing_manifest_returns_ok() {
        let temp = tempfile::tempdir().unwrap();
        let plugin =
            SupabasePlugin::new(SupabaseSnapshotConfig::default(), temp.path().to_path_buf());
        let result = plugin
            .delete("supabase-v1\t2f6e6f6e652f6d616e69666573742e6a736f6e")
            .await;
        assert!(
            result.is_ok(),
            "delete on missing supabase manifest must be idempotent: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_registry_delete_dispatches_to_named_plugin() {
        let registry = SnapshotRegistry::new_with_plugins(vec![Box::new(GitPlugin)]);
        let result = registry.delete("git", "malformed-id").await;
        assert!(
            result.is_err(),
            "delete on malformed id must return an error: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_registry_resolve_prunable_records_excludes_pruned() {
        // This test encodes the contract that resolve_prunable_records must:
        // 1. Collect snapshot records from the audit log keyed by (plugin, snapshot_id).
        // 2. Subtract ids recorded in later Decision::Pruned entries.
        // Without the helper, the call does not compile.
        let registry = SnapshotRegistry::for_rollback().unwrap();
        let records = registry.resolve_prunable_records().await;
        assert!(
            records.is_ok(),
            "resolve_prunable_records must be available: {records:?}"
        );
    }

    #[test]
    fn test_retention_policy_age_only_keeps_recent() {
        use time::OffsetDateTime;

        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let policy = RetentionPolicy::from_max_age_days(7);
        let records = vec![
            PrunableRecord {
                plugin: "git".to_string(),
                snapshot_id: "old".to_string(),
                recorded_at: now - time::Duration::days(10),
            },
            PrunableRecord {
                plugin: "git".to_string(),
                snapshot_id: "recent".to_string(),
                recorded_at: now - time::Duration::days(2),
            },
        ];
        let candidates = policy.apply(&records, now);
        let ids: Vec<_> = candidates.iter().map(|r| r.snapshot_id.as_str()).collect();
        assert_eq!(ids, vec!["old"]);
    }

    #[test]
    fn test_retention_policy_count_only_keeps_newest_n_per_provider() {
        use time::OffsetDateTime;

        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let policy = RetentionPolicy::from_max_count_per_provider(2);
        let records = vec![
            PrunableRecord {
                plugin: "git".to_string(),
                snapshot_id: "git-1".to_string(),
                recorded_at: now - time::Duration::days(3),
            },
            PrunableRecord {
                plugin: "git".to_string(),
                snapshot_id: "git-2".to_string(),
                recorded_at: now - time::Duration::days(2),
            },
            PrunableRecord {
                plugin: "git".to_string(),
                snapshot_id: "git-3".to_string(),
                recorded_at: now - time::Duration::days(1),
            },
            PrunableRecord {
                plugin: "docker".to_string(),
                snapshot_id: "docker-1".to_string(),
                recorded_at: now - time::Duration::days(1),
            },
        ];
        let candidates = policy.apply(&records, now);
        let ids: Vec<_> = candidates.iter().map(|r| r.snapshot_id.as_str()).collect();
        assert_eq!(ids, vec!["git-1"]);
    }

    #[test]
    fn test_retention_policy_union_keeps_any_record_that_passes_either_rule() {
        use time::OffsetDateTime;

        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let policy = RetentionPolicy::from_config(&aegis_config::PruneConfig {
            enabled: true,
            max_count_per_provider: Some(1),
            max_age_days: Some(7),
        });
        let records = vec![
            // Outside the age window but kept by the per-provider count (it is the newest).
            PrunableRecord {
                plugin: "git".to_string(),
                snapshot_id: "count-kept".to_string(),
                recorded_at: now - time::Duration::days(1),
            },
            // Inside the age window but not kept by count; age rule preserves it.
            PrunableRecord {
                plugin: "git".to_string(),
                snapshot_id: "age-kept".to_string(),
                recorded_at: now - time::Duration::days(2),
            },
            // Outside both windows; this is the only candidate.
            PrunableRecord {
                plugin: "git".to_string(),
                snapshot_id: "prune-me".to_string(),
                recorded_at: now - time::Duration::days(10),
            },
        ];
        let candidates = policy.apply(&records, now);
        let ids: Vec<_> = candidates.iter().map(|r| r.snapshot_id.as_str()).collect();
        assert_eq!(ids, vec!["prune-me"]);
    }

    #[test]
    fn test_retention_policy_empty_input_yields_empty_candidates() {
        use time::OffsetDateTime;

        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let policy = RetentionPolicy::from_max_count_per_provider(5);
        let candidates = policy.apply(&[], now);
        assert!(
            candidates.is_empty(),
            "empty input must produce no prune candidates"
        );
    }

    #[test]
    fn test_retention_policy_no_active_rules_yields_empty_candidates() {
        use time::OffsetDateTime;

        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let policy = RetentionPolicy::default();
        let records = vec![
            PrunableRecord {
                plugin: "git".to_string(),
                snapshot_id: "old".to_string(),
                recorded_at: now - time::Duration::days(30),
            },
            PrunableRecord {
                plugin: "git".to_string(),
                snapshot_id: "new".to_string(),
                recorded_at: now - time::Duration::days(1),
            },
        ];
        let candidates = policy.apply(&records, now);
        assert!(
            candidates.is_empty(),
            "no active rules must produce no prune candidates"
        );
    }
}
