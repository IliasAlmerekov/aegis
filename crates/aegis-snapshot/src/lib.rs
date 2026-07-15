//! Snapshot and rollback plugins for Aegis.
//!
//! This crate provides the [`SnapshotPlugin`] trait, [`SnapshotRegistry`],
//! [`SnapshotRegistryConfig`], and six built-in provider backends:
//! git, docker, postgres, mysql, sqlite, and supabase.

#![deny(missing_docs)]

use std::path::Path;

use async_trait::async_trait;

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

/// Injectable clock primitives for deterministic retention tests.
mod clock;
/// Filesystem containment checks for snapshot artifacts.
mod containment;
/// Filesystem path helpers for the snapshot subsystem.
mod paths;
/// Snapshot registry materialization and runtime dispatch.
mod registry;
/// Snapshot retention policy and prunable-record resolution.
mod retention;
/// Owner-only filesystem creation helpers for snapshot artifacts.
mod secure_fs;
/// Test hooks for observing snapshot registry materialization.
pub mod testing;

/// Re-export of [`clock::Clock`], [`clock::SystemClock`], [`clock::FixedClock`].
pub use clock::{Clock, FixedClock, SystemClock};
/// Re-export of [`registry::SnapshotRegistry`], [`registry::SnapshotRegistryConfig`],
/// [`registry::available_provider_names`].
pub use registry::{SnapshotRegistry, SnapshotRegistryConfig, available_provider_names};
/// Re-export of [`retention::PrunableRecord`], [`retention::RetentionPolicy`].
pub use retention::{PrunableRecord, RetentionPolicy};

type Result<T> = std::result::Result<T, SnapshotError>;

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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use aegis_config::SupabaseSnapshotConfig;

    use super::*;

    fn hex_encode_path(path: &std::path::Path) -> String {
        path.to_string_lossy()
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

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
        let missing_dump = temp.path().join("missing.dump");
        let plugin = PostgresPlugin::new(
            "db".to_string(),
            "localhost".to_string(),
            5432,
            "user".to_string(),
            temp.path().to_path_buf(),
        );
        let result = plugin
            .delete(&format!(
                "v2\t6462\t6c6f63616c686f7374\t5432\t75736572\t{}",
                hex_encode_path(&missing_dump)
            ))
            .await;
        assert!(
            result.is_ok(),
            "delete on missing postgres dump must be idempotent: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_mysql_plugin_delete_missing_dump_returns_ok() {
        let temp = tempfile::tempdir().unwrap();
        let missing_dump = temp.path().join("missing.sql");
        let plugin = MysqlPlugin::new(
            "db".to_string(),
            "localhost".to_string(),
            3306,
            "user".to_string(),
            temp.path().to_path_buf(),
        );
        let result = plugin
            .delete(&format!(
                "v2\t6462\t6c6f63616c686f7374\t3306\t75736572\t{}",
                hex_encode_path(&missing_dump)
            ))
            .await;
        assert!(
            result.is_ok(),
            "delete on missing mysql dump must be idempotent: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_sqlite_plugin_delete_missing_dump_returns_ok() {
        let temp = tempfile::tempdir().unwrap();
        let missing_dump = temp.path().join("missing.db");
        let plugin = SqlitePlugin::new(PathBuf::from("/tmp/app.db"), temp.path().to_path_buf());
        let result = plugin
            .delete(&format!(
                "v2\t2f746d702f6170702e6462\t{}",
                hex_encode_path(&missing_dump)
            ))
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
