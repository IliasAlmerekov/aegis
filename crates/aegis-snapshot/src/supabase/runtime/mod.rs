//! Supabase snapshot runtime coordinator.
//!
//! This module wires the `SnapshotPlugin` implementation for `SupabasePlugin`
//! and delegates the security-critical helpers to focused submodules:
//!
//! - [`manifest_io`]: atomic manifest write (temp file -> sync -> rename ->
//!   sync parent dir) plus the test-only manifest write failure injection hook.
//! - [`manifest_state`]: manifest construction, schema validation, and strict
//!   rollback-eligibility recomputation.
//! - [`rollback`]: artifact path resolution (bundle-root containment) and
//!   config-target-match checks.
//! - [`tests`]: unit tests covering the atomic-write and rollback invariants.
//!
//! The trait impl lives here because Rust requires a single `impl` block for a
//! trait; the helper logic is split out to keep every file under 800 LoC.

use super::*;

use manifest_io::write_manifest_atomically;
use manifest_state::phase1_complete;

mod manifest_io;
mod manifest_state;
mod rollback;

#[cfg(test)]
mod tests;

#[async_trait]
impl SnapshotPlugin for SupabasePlugin {
    fn name(&self) -> &'static str {
        "supabase"
    }

    async fn is_applicable(&self, _cwd: &Path) -> bool {
        !self.config.db.database.trim().is_empty()
            && Self::binary_available(&self.pg_dump_bin).await
            && Self::binary_available(&self.pg_restore_bin).await
    }

    async fn snapshot(&self, _cwd: &Path, _cmd: &str) -> Result<String> {
        self.validate_preflight().await?;

        let bundle_dir = self.create_bundle_dir()?;
        let artifacts_dir = bundle_dir.join("artifacts");
        if let Err(error) = fs::create_dir_all(&artifacts_dir) {
            return Err(self.fail_closed_after_cleanup(error.into(), &bundle_dir, None));
        }

        let dump_path = artifacts_dir.join("db.dump");
        if let Err(error) = self.run_pg_dump(&dump_path).await {
            return Err(self.fail_closed_after_cleanup(error, &bundle_dir, Some(&dump_path)));
        }

        let checksum_sha256 = match sha256_hex(&dump_path) {
            Ok(checksum_sha256) => checksum_sha256,
            Err(error) => {
                return Err(self.fail_closed_after_cleanup(error, &bundle_dir, Some(&dump_path)));
            }
        };

        let size_bytes = match fs::metadata(&dump_path) {
            Ok(metadata) => metadata.len(),
            Err(error) => {
                return Err(self.fail_closed_after_cleanup(
                    error.into(),
                    &bundle_dir,
                    Some(&dump_path),
                ));
            }
        };

        let manifest = match phase1_complete(
            &self.config,
            "artifacts/db.dump",
            checksum_sha256,
            size_bytes,
        ) {
            Ok(manifest) => manifest,
            Err(error) => {
                return Err(self.fail_closed_after_cleanup(error, &bundle_dir, Some(&dump_path)));
            }
        };

        let manifest_path = bundle_dir.join(MANIFEST_FILE_NAME);
        #[cfg(test)]
        INJECT_MANIFEST_WRITE_FAILURE_FOR_TESTS
            .with(|flag| flag.set(self.inject_manifest_write_failure_for_tests));

        if let Err(error) = write_manifest_atomically(&manifest_path, &manifest) {
            return Err(self.fail_closed_after_cleanup(error, &bundle_dir, Some(&dump_path)));
        }

        let canonical_manifest_path = match manifest_path.canonicalize() {
            Ok(canonical_manifest_path) => canonical_manifest_path,
            Err(error) => {
                return Err(self.fail_closed_after_cleanup(
                    error.into(),
                    &bundle_dir,
                    Some(&dump_path),
                ));
            }
        };

        let snapshot_id = Self::build_snapshot_id(&canonical_manifest_path);
        tracing::info!(%snapshot_id, "supabase snapshot created");
        Ok(snapshot_id)
    }

    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        let manifest_path = Self::parse_snapshot_id(snapshot_id)?;
        let manifest_bytes = fs::read(&manifest_path).map_err(|error| {
            SnapshotError::Snapshot(format!(
                "failed to read supabase manifest {}: {error}",
                manifest_path.display()
            ))
        })?;
        let manifest: SupabaseManifest =
            serde_json::from_slice(&manifest_bytes).map_err(|error| {
                SnapshotError::Snapshot(format!(
                    "failed to deserialize supabase manifest {}: {error}",
                    manifest_path.display()
                ))
            })?;

        manifest.validate_schema_v1()?;

        let strict = manifest.recompute_strict_eligibility()?;
        if !strict.allowed {
            return Err(SnapshotError::Snapshot(format!(
                "rollback denied: recomputed manifest eligibility is not allowed (status: {:?})",
                strict.overall_status
            )));
        }
        manifest.ensure_summary_matches_recomputed(&strict)?;

        let dump_path = manifest.resolve_db_artifact_path(&manifest_path)?;
        if !dump_path.exists() {
            return Err(SnapshotError::RollbackDumpNotFound {
                path: dump_path.to_string_lossy().to_string(),
            });
        }

        let expected_sha256 = manifest
            .artifacts
            .db
            .checksum_sha256
            .as_ref()
            .ok_or_else(|| {
                SnapshotError::Snapshot(
                    "manifest artifacts.db.checksum_sha256 is required for rollback".to_string(),
                )
            })?
            .clone();
        let actual_sha256 = sha256_hex(&dump_path)?;
        if actual_sha256 != expected_sha256 {
            return Err(SnapshotError::RollbackIntegrityCheckFailed {
                path: dump_path.to_string_lossy().to_string(),
                expected_sha256,
                actual_sha256,
            });
        }

        if self.config.require_config_target_match_on_rollback {
            manifest.ensure_config_target_matches(&self.config)?;
        }

        let target = manifest.target.db.as_ref().ok_or_else(|| {
            SnapshotError::Snapshot("manifest target.db is required for rollback".to_string())
        })?;
        self.run_pg_restore(target, &dump_path).await?;

        tracing::info!(snapshot_id = snapshot_id, "supabase snapshot rolled back");
        Ok(())
    }

    async fn delete(&self, snapshot_id: &str) -> Result<()> {
        let manifest_path = Self::parse_snapshot_id(snapshot_id)?;

        match fs::metadata(&manifest_path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(path = %manifest_path.display(), "supabase manifest already removed");
                return Ok(());
            }
            Err(error) => {
                return Err(SnapshotError::DeleteFailed {
                    plugin: "supabase".to_string(),
                    snapshot_id: snapshot_id.to_string(),
                    source: error.to_string(),
                });
            }
            Ok(_) => {}
        }

        let manifest_bytes = match fs::read(&manifest_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(path = %manifest_path.display(), "supabase manifest already removed");
                return Ok(());
            }
            Err(error) => {
                return Err(SnapshotError::DeleteFailed {
                    plugin: "supabase".to_string(),
                    snapshot_id: snapshot_id.to_string(),
                    source: error.to_string(),
                });
            }
        };

        let manifest: SupabaseManifest = match serde_json::from_slice(&manifest_bytes) {
            Ok(m) => m,
            Err(error) => {
                return Err(SnapshotError::DeleteFailed {
                    plugin: "supabase".to_string(),
                    snapshot_id: snapshot_id.to_string(),
                    source: format!("failed to parse manifest: {error}"),
                });
            }
        };

        if let Ok(dump_path) = manifest.resolve_db_artifact_path(&manifest_path) {
            match fs::remove_file(&dump_path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(SnapshotError::DeleteFailed {
                        plugin: "supabase".to_string(),
                        snapshot_id: snapshot_id.to_string(),
                        source: format!("failed to remove dump {}: {error}", dump_path.display()),
                    });
                }
                _ => {}
            }
        }

        match fs::remove_file(&manifest_path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(SnapshotError::DeleteFailed {
                    plugin: "supabase".to_string(),
                    snapshot_id: snapshot_id.to_string(),
                    source: format!(
                        "failed to remove manifest {}: {error}",
                        manifest_path.display()
                    ),
                });
            }
            _ => {}
        }

        if let Some(bundle_dir) = manifest_path.parent() {
            match fs::remove_dir(bundle_dir) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                Err(error) => {
                    return Err(SnapshotError::DeleteFailed {
                        plugin: "supabase".to_string(),
                        snapshot_id: snapshot_id.to_string(),
                        source: format!(
                            "failed to remove bundle {}: {error}",
                            bundle_dir.display()
                        ),
                    });
                }
            }
        }

        tracing::info!(snapshot_id = snapshot_id, "supabase snapshot deleted");
        Ok(())
    }
}
