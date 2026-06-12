/// Delegate to the `aegis-snapshot` crate's thread-local build counter.
pub(crate) fn reset_snapshot_registry_build_count_for_tests() {
    aegis_snapshot::testing::reset_registry_build_count();
}

/// Delegate to the `aegis-snapshot` crate's thread-local build counter.
pub(crate) fn snapshot_registry_build_count_for_tests() -> usize {
    aegis_snapshot::testing::registry_build_count()
}
