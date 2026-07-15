//! Snapshot artifact containment checks for filesystem-backed plugins.

use std::path::{Component, Path, PathBuf};

use crate::{Result, SnapshotError};

/// Return an artifact path only when it is provably inside `store`.
///
/// `candidate` must be an absolute artifact path. The artifact itself may be
/// absent so deletion can remain idempotent; its parent must still resolve
/// beneath the canonical snapshot store.
pub(crate) fn contain_artifact(
    plugin: &'static str,
    store: &Path,
    candidate: &Path,
) -> Result<PathBuf> {
    let reject = || SnapshotError::PathEscapesSnapshotStore {
        plugin,
        store: store.to_string_lossy().into_owned(),
        candidate: candidate.to_string_lossy().into_owned(),
    };

    let store_canonical = store.canonicalize().map_err(|_| reject())?;
    if !candidate.is_absolute()
        || candidate.file_name().is_none()
        || candidate.as_os_str().is_empty()
        || candidate
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(reject());
    }

    let parent = candidate.parent().ok_or_else(reject)?;
    let parent_canonical = parent.canonicalize().map_err(|_| reject())?;
    if !parent_canonical.starts_with(&store_canonical) {
        return Err(reject());
    }

    if candidate.exists() {
        let artifact_canonical = candidate.canonicalize().map_err(|_| reject())?;
        if !artifact_canonical.starts_with(&store_canonical) {
            return Err(reject());
        }
        return Ok(artifact_canonical);
    }

    Ok(candidate.to_path_buf())
}
