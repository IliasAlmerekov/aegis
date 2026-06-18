//! Manifest atomic-write helpers for the Supabase snapshot runtime.
//!
//! These helpers preserve the manifest atomic-write semantics:
//! write temp file -> sync temp file -> rename -> sync parent directory.
//! The ordering, fsync/sync steps, and the test-only manifest write failure
//! injection hook MUST NOT be reordered, dropped, or simplified.

use std::fs;
use std::path::Path;

use super::super::{Result, SnapshotError, SupabaseManifest, sync_parent_directory};

#[cfg(test)]
use super::super::INJECT_MANIFEST_WRITE_FAILURE_FOR_TESTS;

pub(super) fn write_manifest_atomically(
    manifest_path: &Path,
    manifest: &SupabaseManifest,
) -> Result<()> {
    let temp_path = manifest_path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|error| {
        SnapshotError::Snapshot(format!("failed to serialize supabase manifest: {error}"))
    })?;

    {
        use std::io::Write as _;

        let mut file = fs::File::create(&temp_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }

    #[cfg(test)]
    if INJECT_MANIFEST_WRITE_FAILURE_FOR_TESTS.with(|flag| flag.replace(false)) {
        return Err(SnapshotError::Snapshot(
            "manifest commit injected failure".to_string(),
        ));
    }

    fs::rename(&temp_path, manifest_path)?;
    let parent = manifest_path.parent().ok_or_else(|| {
        SnapshotError::Snapshot("manifest parent directory is required".to_string())
    })?;
    sync_parent_directory(parent)?;
    Ok(())
}
