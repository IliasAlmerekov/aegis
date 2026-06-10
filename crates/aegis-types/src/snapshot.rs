/// A record of a single successful snapshot created by one plugin.
#[derive(Debug, Clone)]
pub struct SnapshotRecord {
    /// Name of the plugin that created this snapshot.
    pub plugin: &'static str,
    /// Opaque identifier returned by the plugin (e.g. stash ref, image tag).
    pub snapshot_id: String,
}
