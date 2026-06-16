use aegis::audit::{AuditEntry, AuditLogger, AuditSnapshot, Decision};
use aegis::config::AegisConfig;
use aegis::error::AegisError;
use aegis::interceptor::RiskLevel;
use aegis::snapshot::{SnapshotRegistry, SnapshotRegistryConfig};

type Result<T> = std::result::Result<T, AegisError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RollbackTarget {
    pub(crate) plugin: String,
    pub(crate) snapshot_id: String,
}

pub(crate) async fn execute(snapshot_id: String) -> Result<RollbackTarget> {
    let config = AegisConfig::load()?;
    let audit_logger = AuditLogger::from_audit_config(&config.audit);
    let target = find_snapshot_target(&audit_logger, &snapshot_id)?;

    SnapshotRegistry::from_runtime_config(&SnapshotRegistryConfig::for_rollback_from_config(
        &config,
    )?)
    .rollback(&target.plugin, &target.snapshot_id)
    .await?;

    append_rollback_audit_entry(&audit_logger, &target)?;

    Ok(target)
}

fn find_snapshot_target(logger: &AuditLogger, snapshot_id: &str) -> Result<RollbackTarget> {
    let entries = logger.read_all()?;

    // Any id that has been explicitly pruned is no longer recoverable, even if
    // earlier audit entries still reference it. Treat it as removed before doing
    // the normal reverse lookup.
    let mut pruned_ids = std::collections::HashSet::new();
    for entry in &entries {
        let base = entry.as_base();
        if base.decision == Decision::Pruned {
            for snapshot in &base.snapshots {
                pruned_ids.insert(snapshot.snapshot_id.as_str());
            }
            if let Some(id) = snapshot_id_from_prune_command(&base.command) {
                pruned_ids.insert(id);
            }
        }
    }

    if pruned_ids.contains(snapshot_id) {
        return Err(AegisError::Snapshot(format!(
            "snapshot id {snapshot_id:?} has been pruned and is no longer recoverable."
        )));
    }

    entries
        .iter()
        .rev()
        .filter(|entry| entry.as_base().decision != Decision::Pruned)
        .flat_map(|entry| entry.as_base().snapshots.iter().rev())
        .find(|snapshot| snapshot.snapshot_id == snapshot_id)
        .map(|snapshot| RollbackTarget {
            plugin: snapshot.plugin.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
        })
        .ok_or_else(|| {
            AegisError::Snapshot(format!(
                "snapshot id {snapshot_id:?} was not found in the audit log.\n\
                 Hint: run `aegis audit --format json` or `aegis audit --last 20` \
                 to find a recorded snapshot id, then retry `aegis rollback <snapshot-id>`."
            ))
        })
}

/// Extract a snapshot id from a prune audit entry's command field.
///
/// Prune records may store the removed snapshot id as `aegis prune <snapshot_id>`
/// when the `snapshots` array is empty.
fn snapshot_id_from_prune_command(command: &str) -> Option<&str> {
    const PRUNE_PREFIX: &str = "aegis prune ";
    command
        .strip_prefix(PRUNE_PREFIX)
        .filter(|id| !id.is_empty())
}

fn append_rollback_audit_entry(logger: &AuditLogger, target: &RollbackTarget) -> Result<()> {
    Ok(logger.append(AuditEntry::new(
        format!("aegis rollback {}", target.snapshot_id),
        RiskLevel::Safe,
        Vec::new(),
        Decision::Approved,
        vec![AuditSnapshot {
            plugin: target.plugin.clone(),
            snapshot_id: target.snapshot_id.clone(),
        }],
        None,
        None,
    ))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_entry(
        logger: &AuditLogger,
        command: &str,
        plugin: &str,
        snapshot_id: &str,
    ) -> Result<()> {
        Ok(logger.append(AuditEntry::new(
            command,
            RiskLevel::Danger,
            Vec::new(),
            Decision::Denied,
            vec![AuditSnapshot {
                plugin: plugin.to_string(),
                snapshot_id: snapshot_id.to_string(),
            }],
            None,
            None,
        ))?)
    }

    fn write_pruned_entry(
        logger: &AuditLogger,
        command: &str,
        plugin: &str,
        snapshot_id: &str,
    ) -> Result<()> {
        Ok(logger.append(AuditEntry::new(
            command,
            RiskLevel::Safe,
            Vec::new(),
            Decision::Pruned,
            vec![AuditSnapshot {
                plugin: plugin.to_string(),
                snapshot_id: snapshot_id.to_string(),
            }],
            None,
            None,
        ))?)
    }

    #[test]
    fn find_snapshot_target_rejects_pruned_snapshot() {
        let dir = TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

        write_entry(&logger, "rm -rf src", "git", "snap-001").unwrap();
        write_pruned_entry(&logger, "aegis prune snap-001", "git", "snap-001").unwrap();

        let err = find_snapshot_target(&logger, "snap-001")
            .expect_err("a pruned snapshot must not be discoverable for rollback");
        let message = err.to_string();
        assert!(
            message.to_lowercase().contains("pruned"),
            "error must indicate the snapshot was pruned: {message}"
        );
    }

    #[test]
    fn find_snapshot_target_prefers_latest_matching_entry() {
        let dir = TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

        write_entry(&logger, "first", "git", "snap-001").unwrap();
        write_entry(&logger, "second", "docker", "snap-001").unwrap();

        let target = find_snapshot_target(&logger, "snap-001").unwrap();
        assert_eq!(target.plugin, "docker");
    }

    #[test]
    fn find_snapshot_target_returns_recovery_hint_when_missing() {
        let dir = TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

        let err = find_snapshot_target(&logger, "missing").expect_err("lookup must fail");
        let message = err.to_string();

        assert!(message.contains("missing"));
        assert!(message.contains("aegis audit"));
    }
}
