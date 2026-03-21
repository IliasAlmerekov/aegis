use std::path::Path;

use async_trait::async_trait;

use crate::error::AegisError;
use crate::snapshot::SnapshotPlugin;

type Result<T> = std::result::Result<T, AegisError>;

pub struct DockerPlugin;

#[async_trait]
impl SnapshotPlugin for DockerPlugin {
    fn name(&self) -> &'static str {
        "docker"
    }

    fn is_applicable(&self, _cwd: &Path) -> bool {
        // Implemented in T4.3: check Docker CLI availability and running containers.
        false
    }

    async fn snapshot(&self, _cwd: &Path, _cmd: &str) -> Result<String> {
        // Implemented in T4.3
        Err(AegisError::Snapshot("docker snapshot not yet implemented".to_string()))
    }

    async fn rollback(&self, _snapshot_id: &str) -> Result<()> {
        // Implemented in T4.3
        Err(AegisError::Snapshot("docker rollback not yet implemented".to_string()))
    }
}
