//! Re-export shim — all snapshot logic lives in the `aegis-snapshot` crate.
pub use aegis_snapshot::{
    DockerPlugin, GitPlugin, MysqlPlugin, PostgresPlugin, SnapshotError, SnapshotPlugin,
    SnapshotRecord, SnapshotRegistry, SnapshotRegistryConfig, SqlitePlugin, SupabasePlugin,
    available_provider_names,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use tests::reset_snapshot_registry_build_count_for_tests;
#[cfg(test)]
pub(crate) use tests::snapshot_registry_build_count_for_tests;
