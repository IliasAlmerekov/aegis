use std::path::Path;

use async_trait::async_trait;

use crate::error::AegisError;
use crate::snapshot::SnapshotPlugin;

type Result<T> = std::result::Result<T, AegisError>;

pub struct GitPlugin;

#[async_trait]
impl SnapshotPlugin for GitPlugin {
    fn name(&self) -> &'static str {
        "git"
    }

    fn is_applicable(&self, cwd: &Path) -> bool {
        cwd.join(".git").exists()
    }

    async fn snapshot(&self, _cwd: &Path, _cmd: &str) -> Result<String> {
        // Implemented in T4.2
        Err(AegisError::Snapshot(
            "git snapshot not yet implemented".to_string(),
        ))
    }

    async fn rollback(&self, _snapshot_id: &str) -> Result<()> {
        // Implemented in T4.2
        Err(AegisError::Snapshot(
            "git rollback not yet implemented".to_string(),
        ))
    }
}
