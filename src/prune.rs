//! CLI implementation for `aegis snapshot prune`.

use aegis::audit::{AuditEntry, AuditLogger, AuditSnapshot, Decision};
use aegis::config::{AegisConfig, PruneConfig};
use aegis::error::AegisError;
use aegis::interceptor::RiskLevel;
use aegis::snapshot::{Clock, PrunableRecord, RetentionPolicy, SnapshotRegistry, SystemClock};

/// Execute the prune command and return the records that were actually removed.
///
/// When `--yes` is not given, this prints a dry-run preview and returns an
/// empty vector. When `--yes` is given, it deletes the retention-policy
/// candidates and appends a `Decision::Pruned` audit entry for each one.
pub(crate) async fn execute(args: crate::PruneArgs) -> Result<Vec<PrunableRecord>, AegisError> {
    let config = AegisConfig::load().map_err(AegisError::from)?;
    let registry = SnapshotRegistry::for_rollback().map_err(AegisError::from)?;
    let records: Vec<PrunableRecord> = registry
        .resolve_prunable_records()
        .await
        .map_err(AegisError::from)?;
    let policy = RetentionPolicy::from_config(&config.prune);
    let now = SystemClock.now();
    let candidates = policy.apply(&records, now);

    if !args.yes || args.dry_run {
        preview_candidates(&candidates, &config.prune);
        return Ok(Vec::new());
    }

    let logger = AuditLogger::from_audit_config(&config.audit);
    let mut pruned = Vec::new();
    let mut failures = Vec::new();
    for record in candidates {
        match registry.delete(&record.plugin, &record.snapshot_id).await {
            Ok(()) => {
                let entry = AuditEntry::new(
                    format!("aegis prune {}", record.snapshot_id),
                    RiskLevel::Safe,
                    Vec::new(),
                    Decision::Pruned,
                    vec![AuditSnapshot {
                        plugin: record.plugin.clone(),
                        snapshot_id: record.snapshot_id.clone(),
                    }],
                    None,
                    None,
                );
                logger.append(entry).map_err(AegisError::from)?;
                pruned.push(record);
            }
            Err(error) => {
                tracing::warn!(
                    plugin = %record.plugin,
                    id = %record.snapshot_id,
                    error = %error,
                    "prune delete failed"
                );
                failures.push(format!(
                    "{} snapshot {}: {error}",
                    record.plugin, record.snapshot_id
                ));
            }
        }
    }

    if !failures.is_empty() {
        return Err(AegisError::PrunePartialFailure {
            pruned: pruned.len(),
            failed_count: failures.len(),
            failed: failures.join("\n"),
        });
    }

    println!("Pruned {} snapshot(s).", pruned.len());
    Ok(pruned)
}

fn preview_candidates(candidates: &[PrunableRecord], prune_config: &PruneConfig) {
    if candidates.is_empty() {
        println!("No snapshots are eligible for pruning.");
        return;
    }

    if !prune_config.enabled {
        println!("Note: prune is currently disabled in config; this is a dry-run preview.");
    }

    println!("Would prune {} snapshot(s):", candidates.len());
    for record in candidates {
        println!("  provider: {}  id: {}", record.plugin, record.snapshot_id);
    }
}
