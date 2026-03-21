pub mod docker;
pub mod git;

pub use docker::DockerPlugin;
pub use git::GitPlugin;

use std::path::Path;

use async_trait::async_trait;

use crate::error::AegisError;

type Result<T> = std::result::Result<T, AegisError>;

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

/// Holds all registered plugins and drives the snapshot lifecycle.
pub struct SnapshotRegistry {
    plugins: Vec<Box<dyn SnapshotPlugin>>,
}

impl Default for SnapshotRegistry {
    fn default() -> Self {
        Self {
            plugins: vec![Box::new(GitPlugin), Box::new(DockerPlugin)],
        }
    }
}

impl SnapshotRegistry {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockPlugin {
        applicable: bool,
        call_count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl SnapshotPlugin for MockPlugin {
        fn name(&self) -> &'static str {
            "mock"
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
                    applicable: true,
                    call_count: Arc::clone(&applicable_calls),
                }),
                Box::new(MockPlugin {
                    applicable: false,
                    call_count: Arc::clone(&skipped_calls),
                }),
                Box::new(MockPlugin {
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
}
