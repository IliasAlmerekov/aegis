//! Manifest atomic-write helpers for the Supabase snapshot runtime.
//!
//! These helpers preserve the manifest atomic-write semantics:
//! write temp file -> sync temp file -> rename -> sync parent directory.
//! The ordering, fsync/sync steps, and the test-only manifest write failure
//! injection hook MUST NOT be reordered, dropped, or simplified.

use std::fs;
use std::path::Path;

use super::super::{Result, SnapshotError, SupabaseManifest, sync_parent_directory};
use crate::containment::contain_artifact;
use crate::secure_fs::create_artifact_file;

#[cfg(test)]
use super::super::INJECT_MANIFEST_WRITE_FAILURE_FOR_TESTS;

pub(super) fn write_manifest_atomically(
    manifest_path: &Path,
    manifest: &SupabaseManifest,
) -> Result<()> {
    let parent = manifest_path.parent().ok_or_else(|| {
        SnapshotError::Snapshot("manifest parent directory is required".to_string())
    })?;
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|error| {
        SnapshotError::Snapshot(format!("failed to serialize supabase manifest: {error}"))
    })?;

    {
        use std::io::Write as _;

        let mut suffix = None;
        let (temp_path, mut file) = loop {
            let extension = match suffix {
                Some(suffix) => format!("json.tmp-{suffix}"),
                None => "json.tmp".to_string(),
            };
            let temp_path =
                contain_artifact("supabase", parent, &manifest_path.with_extension(extension))?;
            match create_artifact_file("supabase", &temp_path) {
                Ok(file) => break (temp_path, file),
                Err(SnapshotError::Io(error))
                    if error.kind() == std::io::ErrorKind::AlreadyExists =>
                {
                    suffix = Some(suffix.map_or(1, |current| current + 1));
                }
                Err(error) => return Err(error),
            }
        };
        file.write_all(&bytes)?;
        file.sync_all()?;

        #[cfg(test)]
        if INJECT_MANIFEST_WRITE_FAILURE_FOR_TESTS.with(|flag| flag.replace(false)) {
            return Err(SnapshotError::Snapshot(
                "manifest commit injected failure".to_string(),
            ));
        }

        drop(file);
        fs::rename(&temp_path, manifest_path)?;
    }
    sync_parent_directory(parent)?;
    Ok(())
}
