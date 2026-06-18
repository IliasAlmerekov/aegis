//! Rollback support for the Supabase snapshot runtime.
//!
//! Contains artifact-path resolution (with bundle-root containment checks
//! that reject absolute paths, parent traversal, and symlink escape) and the
//! config-target-match checks used to gate rollback when the operator opted
//! into `require_config_target_match_on_rollback`. These invariants MUST be
//! preserved exactly.

use std::path::{Component, Path, PathBuf};

use aegis_config::SupabaseSnapshotConfig;

use super::super::{Result, SnapshotError, SupabaseManifest};

impl SupabaseManifest {
    pub(super) fn resolve_db_artifact_path(&self, manifest_path: &Path) -> Result<PathBuf> {
        let bundle_root = manifest_path.parent().ok_or_else(|| {
            SnapshotError::Snapshot("manifest bundle root is required".to_string())
        })?;
        let relative_path = self.artifacts.db.path.as_deref().ok_or_else(|| {
            SnapshotError::Snapshot("manifest artifacts.db.path is required".to_string())
        })?;
        let artifact_relative = Path::new(relative_path);

        if artifact_relative.is_absolute()
            || artifact_relative.as_os_str().is_empty()
            || artifact_relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(SnapshotError::Snapshot(format!(
                "manifest artifacts.db.path must stay within bundle root: {relative_path}"
            )));
        }

        let bundle_root_canonical = bundle_root.canonicalize()?;
        let resolved = bundle_root.join(artifact_relative);

        let parent = resolved.parent().ok_or_else(|| {
            SnapshotError::Snapshot(format!(
                "manifest artifacts.db.path has no parent under bundle root: {relative_path}"
            ))
        })?;
        let parent_canonical = parent.canonicalize().map_err(|error| {
            SnapshotError::Snapshot(format!(
                "failed to resolve artifact parent for rollback: {error}"
            ))
        })?;

        if !parent_canonical.starts_with(&bundle_root_canonical) {
            return Err(SnapshotError::Snapshot(format!(
                "manifest artifacts.db.path escapes bundle root: {relative_path}"
            )));
        }

        if resolved.exists() {
            let canonical_resolved = resolved.canonicalize().map_err(|error| {
                SnapshotError::Snapshot(format!(
                    "failed to canonicalize rollback artifact path: {error}"
                ))
            })?;

            if !canonical_resolved.starts_with(&bundle_root_canonical) {
                return Err(SnapshotError::Snapshot(format!(
                    "manifest artifacts.db.path resolves outside bundle root: {relative_path}"
                )));
            }

            return Ok(canonical_resolved);
        }

        Ok(resolved)
    }

    pub(super) fn ensure_config_target_matches(
        &self,
        config: &SupabaseSnapshotConfig,
    ) -> Result<()> {
        let target_db = self.target.db.as_ref().ok_or_else(|| {
            SnapshotError::Snapshot("manifest target.db is required for rollback".to_string())
        })?;

        let mut mismatches = Vec::new();

        if target_db.database != config.db.database {
            mismatches.push(format!(
                "database manifest={} current={}",
                target_db.database, config.db.database
            ));
        }
        if target_db.host != config.db.host {
            mismatches.push(format!(
                "host manifest={} current={}",
                target_db.host, config.db.host
            ));
        }
        if target_db.port != config.db.port {
            mismatches.push(format!(
                "port manifest={} current={}",
                target_db.port, config.db.port
            ));
        }
        if target_db.user != config.db.user {
            mismatches.push(format!(
                "user manifest={} current={}",
                target_db.user, config.db.user
            ));
        }

        if mismatches.is_empty() {
            Ok(())
        } else {
            Err(SnapshotError::Snapshot(format!(
                "rollback target mismatch: current config differs from manifest target: {}",
                mismatches.join(", ")
            )))
        }
    }
}
