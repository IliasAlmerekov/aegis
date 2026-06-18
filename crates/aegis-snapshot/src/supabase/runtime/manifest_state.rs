//! Manifest construction, schema validation, and strict rollback-eligibility
//! recomputation for the Supabase snapshot runtime.
//!
//! The eligibility recomputation is the security-critical invariant that
//! prevents rollback of partial/degraded manifests. The logic here MUST be
//! preserved exactly: no relaxing of `db_supported`/`allowed` checks, no
//! changes to the `overall_status` mapping, and no drift between persisted
//! summary fields and the recomputed values.

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use aegis_config::SupabaseSnapshotConfig;

use super::super::{
    Result, SnapshotError, SupabaseArtifacts, SupabaseCapabilities, SupabaseDbArtifact,
    SupabaseManifest, SupabaseOverallStatus, SupabaseRollback, SupabaseStrictEligibility,
    SupabaseTarget, SupabaseTargetDb,
};

pub(super) fn phase1_complete(
    config: &SupabaseSnapshotConfig,
    artifact_path: &str,
    checksum_sha256: String,
    size_bytes: u64,
) -> Result<SupabaseManifest> {
    let created_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| {
            SnapshotError::Snapshot(format!("failed to format manifest time: {error}"))
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
    pub(super) fn phase1() -> Self {
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
    pub(super) fn phase1_empty() -> Self {
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
    pub(super) fn phase1_fixture() -> Self {
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

    pub(super) fn validate_schema_v1(&self) -> Result<()> {
        if self.provider != "supabase" {
            return Err(SnapshotError::Snapshot(
                "manifest provider must be supabase".to_string(),
            ));
        }
        if self.manifest_version != 1 {
            return Err(SnapshotError::Snapshot(
                "unsupported supabase manifest version".to_string(),
            ));
        }
        if self.target.db.is_none() {
            return Err(SnapshotError::Snapshot(
                "manifest target.db is required for v1".to_string(),
            ));
        }
        if self.artifacts.db.present && self.artifacts.db.path.is_none() {
            return Err(SnapshotError::Snapshot(
                "manifest artifacts.db.path is required when db is present".to_string(),
            ));
        }
        if self.artifacts.db.present && self.artifacts.db.checksum_sha256.is_none() {
            return Err(SnapshotError::Snapshot(
                "manifest artifacts.db.checksum_sha256 is required when db is present".to_string(),
            ));
        }
        Ok(())
    }

    pub(super) fn recompute_strict_eligibility(&self) -> Result<SupabaseStrictEligibility> {
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

    pub(super) fn ensure_summary_matches_recomputed(
        &self,
        strict: &SupabaseStrictEligibility,
    ) -> Result<()> {
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
            Err(SnapshotError::Snapshot(format!(
                "manifest summary does not match recomputed rollback invariants: {}",
                mismatches.join(", ")
            )))
        }
    }
}
