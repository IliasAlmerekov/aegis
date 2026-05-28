use super::*;

fn write_manifest_atomically(manifest_path: &Path, manifest: &SupabaseManifest) -> Result<()> {
    let temp_path = manifest_path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|error| {
        AegisError::Snapshot(format!("failed to serialize supabase manifest: {error}"))
    })?;

    {
        use std::io::Write as _;

        let mut file = fs::File::create(&temp_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }

    #[cfg(test)]
    if INJECT_MANIFEST_WRITE_FAILURE_FOR_TESTS.with(|flag| flag.replace(false)) {
        return Err(AegisError::Snapshot(
            "manifest commit injected failure".to_string(),
        ));
    }

    fs::rename(&temp_path, manifest_path)?;
    let parent = manifest_path
        .parent()
        .ok_or_else(|| AegisError::Snapshot("manifest parent directory is required".to_string()))?;
    sync_parent_directory(parent)?;
    Ok(())
}

fn phase1_complete(
    config: &SupabaseSnapshotConfig,
    artifact_path: &str,
    checksum_sha256: String,
    size_bytes: u64,
) -> Result<SupabaseManifest> {
    let created_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| {
            AegisError::Snapshot(format!("failed to format manifest time: {error}"))
        })?;

    let mut manifest = SupabaseManifest {
        manifest_version: 1,
        provider: "supabase".to_string(),
        created_at,
        capabilities: SupabaseCapabilities::phase1(),
        target: SupabaseTarget {
            project_ref: config.project_ref.clone(),
            db: Some(SupabaseTargetDb {
                database: config.db.database.clone(),
                host: config.db.host.clone(),
                port: config.db.port,
                user: config.db.user.clone(),
            }),
        },
        artifacts: SupabaseArtifacts {
            db: SupabaseDbArtifact {
                present: true,
                complete: true,
                path: Some(artifact_path.to_string()),
                format: Some("postgres.custom".to_string()),
                checksum_sha256: Some(checksum_sha256),
                size_bytes: Some(size_bytes),
            },
        },
        rollback: SupabaseRollback {
            db_supported: false,
            allowed: false,
            config_target_match_required: config.require_config_target_match_on_rollback,
            reasons: Vec::new(),
        },
        partial: false,
        degraded: false,
        warnings: Vec::new(),
        errors: Vec::new(),
        overall_status: SupabaseOverallStatus::Failed,
    };

    let strict = manifest.recompute_strict_eligibility()?;
    manifest.rollback.db_supported = strict.db_supported;
    manifest.rollback.allowed = strict.allowed;
    manifest.overall_status = strict.overall_status;
    Ok(manifest)
}

impl SupabaseCapabilities {
    fn phase1() -> Self {
        Self {
            db: true,
            storage: false,
            functions: false,
            project_config: false,
        }
    }
}

impl SupabaseArtifacts {
    #[cfg(test)]
    fn phase1_empty() -> Self {
        Self {
            db: SupabaseDbArtifact {
                present: false,
                complete: false,
                path: None,
                format: None,
                checksum_sha256: None,
                size_bytes: None,
            },
        }
    }
}

impl SupabaseManifest {
    #[cfg(test)]
    fn phase1_fixture() -> Self {
        Self {
            manifest_version: 1,
            provider: "supabase".to_string(),
            created_at: "2026-04-15T12:34:56Z".to_string(),
            capabilities: SupabaseCapabilities::phase1(),
            target: SupabaseTarget {
                project_ref: "proj_123".to_string(),
                db: Some(SupabaseTargetDb {
                    database: "postgres".to_string(),
                    host: "db.supabase.co".to_string(),
                    port: 5432,
                    user: "postgres".to_string(),
                }),
            },
            artifacts: SupabaseArtifacts {
                db: SupabaseDbArtifact {
                    present: true,
                    complete: true,
                    path: Some("artifacts/db.dump".to_string()),
                    format: Some("postgres.custom".to_string()),
                    checksum_sha256: Some("abc".repeat(21) + "a"),
                    size_bytes: Some(9),
                },
            },
            rollback: SupabaseRollback {
                db_supported: true,
                allowed: true,
                config_target_match_required: true,
                reasons: Vec::new(),
            },
            partial: false,
            degraded: false,
            warnings: Vec::new(),
            errors: Vec::new(),
            overall_status: SupabaseOverallStatus::Complete,
        }
    }

    fn validate_schema_v1(&self) -> Result<()> {
        if self.provider != "supabase" {
            return Err(AegisError::Snapshot(
                "manifest provider must be supabase".to_string(),
            ));
        }
        if self.manifest_version != 1 {
            return Err(AegisError::Snapshot(
                "unsupported supabase manifest version".to_string(),
            ));
        }
        if self.target.db.is_none() {
            return Err(AegisError::Snapshot(
                "manifest target.db is required for v1".to_string(),
            ));
        }
        if self.artifacts.db.present && self.artifacts.db.path.is_none() {
            return Err(AegisError::Snapshot(
                "manifest artifacts.db.path is required when db is present".to_string(),
            ));
        }
        if self.artifacts.db.present && self.artifacts.db.checksum_sha256.is_none() {
            return Err(AegisError::Snapshot(
                "manifest artifacts.db.checksum_sha256 is required when db is present".to_string(),
            ));
        }
        Ok(())
    }

    fn recompute_strict_eligibility(&self) -> Result<SupabaseStrictEligibility> {
        self.validate_schema_v1()?;

        let db_supported = self.capabilities.db
            && self.target.db.is_some()
            && self.artifacts.db.present
            && self.artifacts.db.complete
            && self.artifacts.db.path.is_some()
            && self.artifacts.db.checksum_sha256.is_some()
            && matches!(self.artifacts.db.format.as_deref(), Some("postgres.custom"));

        let allowed = db_supported && !self.partial && !self.degraded && self.errors.is_empty();

        let overall_status = if allowed {
            SupabaseOverallStatus::Complete
        } else if self.partial {
            SupabaseOverallStatus::Partial
        } else if self.degraded {
            SupabaseOverallStatus::Degraded
        } else {
            SupabaseOverallStatus::Failed
        };

        Ok(SupabaseStrictEligibility {
            db_supported,
            allowed,
            overall_status,
        })
    }

    fn ensure_summary_matches_recomputed(&self, strict: &SupabaseStrictEligibility) -> Result<()> {
        let mut mismatches = Vec::new();

        if self.rollback.db_supported != strict.db_supported {
            mismatches.push(format!(
                "rollback.db_supported persisted={} recomputed={}",
                self.rollback.db_supported, strict.db_supported
            ));
        }

        if self.rollback.allowed != strict.allowed {
            mismatches.push(format!(
                "rollback.allowed persisted={} recomputed={}",
                self.rollback.allowed, strict.allowed
            ));
        }

        if self.overall_status != strict.overall_status {
            mismatches.push(format!(
                "overall_status persisted={:?} recomputed={:?}",
                self.overall_status, strict.overall_status
            ));
        }

        if mismatches.is_empty() {
            Ok(())
        } else {
            Err(AegisError::Snapshot(format!(
                "manifest summary does not match recomputed rollback invariants: {}",
                mismatches.join(", ")
            )))
        }
    }

    fn resolve_db_artifact_path(&self, manifest_path: &Path) -> Result<PathBuf> {
        use std::path::Component;

        let bundle_root = manifest_path
            .parent()
            .ok_or_else(|| AegisError::Snapshot("manifest bundle root is required".to_string()))?;
        let relative_path = self.artifacts.db.path.as_deref().ok_or_else(|| {
            AegisError::Snapshot("manifest artifacts.db.path is required".to_string())
        })?;
        let artifact_relative = Path::new(relative_path);

        if artifact_relative.is_absolute()
            || artifact_relative.as_os_str().is_empty()
            || artifact_relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(AegisError::Snapshot(format!(
                "manifest artifacts.db.path must stay within bundle root: {relative_path}"
            )));
        }

        let bundle_root_canonical = bundle_root.canonicalize()?;
        let resolved = bundle_root.join(artifact_relative);

        let parent = resolved.parent().ok_or_else(|| {
            AegisError::Snapshot(format!(
                "manifest artifacts.db.path has no parent under bundle root: {relative_path}"
            ))
        })?;
        let parent_canonical = parent.canonicalize().map_err(|error| {
            AegisError::Snapshot(format!(
                "failed to resolve artifact parent for rollback: {error}"
            ))
        })?;

        if !parent_canonical.starts_with(&bundle_root_canonical) {
            return Err(AegisError::Snapshot(format!(
                "manifest artifacts.db.path escapes bundle root: {relative_path}"
            )));
        }

        if resolved.exists() {
            let canonical_resolved = resolved.canonicalize().map_err(|error| {
                AegisError::Snapshot(format!(
                    "failed to canonicalize rollback artifact path: {error}"
                ))
            })?;

            if !canonical_resolved.starts_with(&bundle_root_canonical) {
                return Err(AegisError::Snapshot(format!(
                    "manifest artifacts.db.path resolves outside bundle root: {relative_path}"
                )));
            }

            return Ok(canonical_resolved);
        }

        Ok(resolved)
    }

    fn ensure_config_target_matches(&self, config: &SupabaseSnapshotConfig) -> Result<()> {
        let target_db = self.target.db.as_ref().ok_or_else(|| {
            AegisError::Snapshot("manifest target.db is required for rollback".to_string())
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
            Err(AegisError::Snapshot(format!(
                "rollback target mismatch: current config differs from manifest target: {}",
                mismatches.join(", ")
            )))
        }
    }
}

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
            AegisError::Snapshot(format!(
                "failed to read supabase manifest {}: {error}",
                manifest_path.display()
            ))
        })?;
        let manifest: SupabaseManifest =
            serde_json::from_slice(&manifest_bytes).map_err(|error| {
                AegisError::Snapshot(format!(
                    "failed to deserialize supabase manifest {}: {error}",
                    manifest_path.display()
                ))
            })?;

        manifest.validate_schema_v1()?;

        let strict = manifest.recompute_strict_eligibility()?;
        if !strict.allowed {
            return Err(AegisError::Snapshot(format!(
                "rollback denied: recomputed manifest eligibility is not allowed (status: {:?})",
                strict.overall_status
            )));
        }
        manifest.ensure_summary_matches_recomputed(&strict)?;

        let dump_path = manifest.resolve_db_artifact_path(&manifest_path)?;
        if !dump_path.exists() {
            return Err(AegisError::RollbackDumpNotFound {
                path: dump_path.to_string_lossy().to_string(),
            });
        }

        let expected_sha256 = manifest
            .artifacts
            .db
            .checksum_sha256
            .as_ref()
            .ok_or_else(|| {
                AegisError::Snapshot(
                    "manifest artifacts.db.checksum_sha256 is required for rollback".to_string(),
                )
            })?
            .clone();
        let actual_sha256 = sha256_hex(&dump_path)?;
        if actual_sha256 != expected_sha256 {
            return Err(AegisError::RollbackIntegrityCheckFailed {
                path: dump_path.to_string_lossy().to_string(),
                expected_sha256,
                actual_sha256,
            });
        }

        if self.config.require_config_target_match_on_rollback {
            manifest.ensure_config_target_matches(&self.config)?;
        }

        let target = manifest.target.db.as_ref().ok_or_else(|| {
            AegisError::Snapshot("manifest target.db is required for rollback".to_string())
        })?;
        self.run_pg_restore(target, &dump_path).await?;

        tracing::info!(snapshot_id = snapshot_id, "supabase snapshot rolled back");
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests;
