//! Snapshot retention policy and prunable-record resolution.

use std::collections::{HashMap, HashSet};

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use aegis_config::PruneConfig;

use crate::error::SnapshotError;
use crate::paths::home_dir;

type Result<T> = std::result::Result<T, SnapshotError>;

/// One snapshot record that may be eligible for pruning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrunableRecord {
    /// Name of the snapshot plugin that created the record.
    pub plugin: String,
    /// Opaque snapshot identifier.
    pub snapshot_id: String,
    /// Timestamp when the snapshot was recorded in the audit log.
    pub recorded_at: OffsetDateTime,
}

/// Retention policy used to decide which snapshots become prune candidates.
///
/// A snapshot is kept if it satisfies either the per-provider count rule or
/// the global age rule. Only snapshots that fail both rules are returned by
/// [`RetentionPolicy::apply`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RetentionPolicy {
    max_count_per_provider: Option<usize>,
    max_age_days: Option<u32>,
}

impl RetentionPolicy {
    /// Retention policy that only uses a maximum age rule.
    pub fn from_max_age_days(days: u32) -> Self {
        Self {
            max_age_days: Some(days),
            ..Self::default()
        }
    }

    /// Retention policy that only uses a per-provider count rule.
    pub fn from_max_count_per_provider(count: usize) -> Self {
        Self {
            max_count_per_provider: Some(count),
            ..Self::default()
        }
    }

    /// Build a policy from the effective runtime config.
    pub fn from_config(config: &PruneConfig) -> Self {
        Self {
            max_count_per_provider: config.max_count_per_provider,
            max_age_days: config.max_age_days,
        }
    }

    /// Apply this policy to a set of records and return the prune candidates.
    ///
    /// Candidates preserve the original input order.
    pub fn apply(&self, records: &[PrunableRecord], now: OffsetDateTime) -> Vec<PrunableRecord> {
        if self.max_count_per_provider.is_none() && self.max_age_days.is_none() {
            return Vec::new();
        }

        let mut kept = HashSet::new();

        if let Some(days) = self.max_age_days {
            let max_age = time::Duration::days(i64::from(days));
            for record in records {
                if now - record.recorded_at <= max_age {
                    kept.insert((record.plugin.clone(), record.snapshot_id.clone()));
                }
            }
        }

        if let Some(count) = self.max_count_per_provider {
            let mut by_provider: HashMap<&str, Vec<&PrunableRecord>> = HashMap::new();
            for record in records {
                by_provider
                    .entry(record.plugin.as_str())
                    .or_default()
                    .push(record);
            }
            for group in by_provider.values_mut() {
                group.sort_by(|a, b| b.recorded_at.cmp(&a.recorded_at));
                for record in group.iter().take(count) {
                    kept.insert((record.plugin.clone(), record.snapshot_id.clone()));
                }
            }
        }

        records
            .iter()
            .filter(|record| !kept.contains(&(record.plugin.clone(), record.snapshot_id.clone())))
            .cloned()
            .collect()
    }
}

#[derive(Debug, serde::Deserialize)]
struct MinimalAuditSnapshot {
    plugin: String,
    snapshot_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct MinimalAuditEntry {
    timestamp: String,
    decision: aegis_types::Decision,
    command: String,
    #[serde(default)]
    snapshots: Vec<MinimalAuditSnapshot>,
}

/// Resolve the snapshot records that are still on record and have not been
/// pruned, by reading the default audit log (`~/.aegis/audit.jsonl`).
///
/// Collects the latest recorded timestamp for each `(plugin, snapshot_id)` pair
/// and removes any id that has a later `Decision::Pruned` entry. If the audit
/// log is missing, the result is empty.
pub(crate) fn resolve_prunable_records_from_default_audit_log() -> Result<Vec<PrunableRecord>> {
    let Some(home) = home_dir() else {
        return Ok(Vec::new());
    };
    let path = home.join(".aegis").join("audit.jsonl");

    if !path.exists() {
        return Ok(Vec::new());
    }

    let contents = std::fs::read_to_string(&path).map_err(SnapshotError::Io)?;
    let mut latest: HashMap<(String, String), OffsetDateTime> = HashMap::new();
    let mut pruned_pairs: HashSet<(String, String)> = HashSet::new();
    let mut pruned_ids: HashSet<String> = HashSet::new();

    for line in contents.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: MinimalAuditEntry = match serde_json::from_str(line) {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(%error, "skipping unparseable audit log line during prune");
                continue;
            }
        };

        if entry.decision == aegis_types::Decision::Pruned {
            for snapshot in &entry.snapshots {
                pruned_pairs.insert((snapshot.plugin.clone(), snapshot.snapshot_id.clone()));
            }
            if let Some(id) = snapshot_id_from_prune_command(&entry.command) {
                pruned_ids.insert(id.to_string());
            }
            continue;
        }

        let recorded_at = match OffsetDateTime::parse(&entry.timestamp, &Rfc3339) {
            Ok(ts) => ts,
            Err(error) => {
                tracing::warn!(%error, timestamp = %entry.timestamp, "skipping audit log line with invalid timestamp");
                continue;
            }
        };

        for snapshot in &entry.snapshots {
            let key = (snapshot.plugin.clone(), snapshot.snapshot_id.clone());
            match latest.get(&key) {
                Some(previous) if *previous >= recorded_at => {}
                _ => {
                    latest.insert(key, recorded_at);
                }
            }
        }
    }

    let mut records: Vec<PrunableRecord> = latest
        .into_iter()
        .filter(|((plugin, snapshot_id), _)| {
            !pruned_pairs.contains(&(plugin.clone(), snapshot_id.clone()))
                && !pruned_ids.contains(snapshot_id)
        })
        .map(|((plugin, snapshot_id), recorded_at)| PrunableRecord {
            plugin,
            snapshot_id,
            recorded_at,
        })
        .collect();
    records.sort_by(|a, b| b.recorded_at.cmp(&a.recorded_at));
    Ok(records)
}

/// Extract a snapshot id from a prune audit entry's command field.
///
/// Prune records store the removed snapshot id as `aegis prune <snapshot_id>` in
/// the `command` field when the `snapshots` array is empty.
fn snapshot_id_from_prune_command(command: &str) -> Option<&str> {
    const PRUNE_PREFIX: &str = "aegis prune ";
    command
        .strip_prefix(PRUNE_PREFIX)
        .filter(|id| !id.is_empty())
}
