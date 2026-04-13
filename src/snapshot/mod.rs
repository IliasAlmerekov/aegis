pub mod docker;
pub mod git;

pub use docker::DockerPlugin;
pub use git::GitPlugin;

use std::path::Path;

use async_trait::async_trait;

use crate::config::Config;
use crate::error::AegisError;

#[cfg(test)]
use std::cell::Cell;

type Result<T> = std::result::Result<T, AegisError>;

const BUILTIN_SNAPSHOT_PROVIDER_NAMES: &[&str] = &["git", "docker"];

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
    pub snapshot_policy: crate::config::SnapshotPolicy,
    pub auto_snapshot_git: bool,
    pub auto_snapshot_docker: bool,
    pub docker_scope: crate::config::DockerScope,
}

impl From<&Config> for SnapshotRegistryConfig {
    fn from(config: &Config) -> Self {
        Self {
            snapshot_policy: config.snapshot_policy,
            auto_snapshot_git: config.auto_snapshot_git,
            auto_snapshot_docker: config.auto_snapshot_docker,
            docker_scope: config.docker_scope.clone(),
        }
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
                if config.auto_snapshot_git {
                    plugins.push(Box::new(GitPlugin));
                }
                if config.auto_snapshot_docker {
                    plugins.push(Box::new(
                        DockerPlugin::new().with_scope(config.docker_scope.clone()),
                    ));
                }
            }
            SnapshotPolicy::Full => {
                plugins.push(Box::new(GitPlugin));
                plugins.push(Box::new(
                    DockerPlugin::new().with_scope(config.docker_scope.clone()),
                ));
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
        Self {
            plugins: vec![Box::new(GitPlugin), Box::new(DockerPlugin::new())],
        }
    }

    /// Return the names of providers materialized by the current runtime config.
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
        let mut config = Config::default();
        config.auto_snapshot_git = false;
        config.auto_snapshot_docker = false;

        let registry = SnapshotRegistry::from_config(&config);

        assert!(registry.plugins.is_empty());
    }

    #[test]
    fn from_config_enables_only_requested_plugins() {
        let mut config = Config::default();
        config.auto_snapshot_git = false;
        config.auto_snapshot_docker = true;

        let registry = SnapshotRegistry::from_config(&config);
        let plugin_names: Vec<_> = registry
            .plugins
            .iter()
            .map(|plugin| plugin.name())
            .collect();

        assert_eq!(plugin_names, vec!["docker"]);
    }

    #[test]
    fn available_provider_names_report_builtins_independent_of_runtime_config() {
        assert_eq!(available_provider_names(), ["git", "docker"]);
    }

    #[test]
    fn configured_provider_names_report_only_materialized_runtime_plugins() {
        let mut config = Config::default();
        config.auto_snapshot_git = false;
        config.auto_snapshot_docker = true;

        let registry = SnapshotRegistry::from_config(&config);

        assert_eq!(registry.configured_provider_names(), vec!["docker"]);
    }

    // ── Snapshot policy tests ───────────────────────────────────────

    #[test]
    fn policy_none_disables_all_plugins() {
        use crate::config::SnapshotPolicy;

        let mut config = Config::default();
        config.snapshot_policy = SnapshotPolicy::None;
        config.auto_snapshot_git = true;
        config.auto_snapshot_docker = true;

        let registry = SnapshotRegistry::from_config(&config);
        assert!(
            registry.plugins.is_empty(),
            "None policy must produce zero plugins"
        );
    }

    #[test]
    fn policy_selective_honours_per_plugin_flags() {
        use crate::config::SnapshotPolicy;

        let mut config = Config::default();
        config.snapshot_policy = SnapshotPolicy::Selective;
        config.auto_snapshot_git = true;
        config.auto_snapshot_docker = false;

        let registry = SnapshotRegistry::from_config(&config);
        let names: Vec<_> = registry.plugins.iter().map(|p| p.name()).collect();
        assert_eq!(names, vec!["git"]);
    }

    #[test]
    fn policy_full_enables_all_plugins() {
        use crate::config::SnapshotPolicy;

        let mut config = Config::default();
        config.snapshot_policy = SnapshotPolicy::Full;
        config.auto_snapshot_git = false;
        config.auto_snapshot_docker = false;

        let registry = SnapshotRegistry::from_config(&config);
        let names: Vec<_> = registry.plugins.iter().map(|p| p.name()).collect();
        assert_eq!(names, vec!["git", "docker"]);
    }
}
