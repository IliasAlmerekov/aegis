use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::AegisError;
use crate::interceptor::RiskLevel;
use crate::interceptor::patterns::Pattern;
use crate::snapshot::SnapshotRecord;

type Result<T> = std::result::Result<T, AegisError>;

/// One append-only audit record stored as a single JSON line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: u64,
    pub command: String,
    pub risk: RiskLevel,
    pub matched_patterns: Vec<MatchedPattern>,
    pub decision: Decision,
    pub snapshots: Vec<AuditSnapshot>,
    /// The allowlist glob pattern that caused this command to be auto-approved,
    /// if any.  `None` when the command was not allowlisted.
    ///
    /// Skipped in JSON output when absent to keep old log entries valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist_pattern: Option<String>,
}

/// User-visible outcome of the interception flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Approved,
    Denied,
    AutoApproved,
    Blocked,
}

/// Stable audit representation of a matched scanner pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchedPattern {
    pub id: String,
    pub risk: RiskLevel,
    pub description: String,
    pub safe_alt: Option<String>,
}

/// Stable audit representation of one snapshot created before execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditSnapshot {
    pub plugin: String,
    pub snapshot_id: String,
}

/// Append-only JSONL audit log stored under `~/.aegis/audit.jsonl`.
pub struct AuditLogger {
    path: PathBuf,
}

impl AuditEntry {
    pub fn new(
        command: impl Into<String>,
        risk: RiskLevel,
        matched_patterns: Vec<MatchedPattern>,
        decision: Decision,
        snapshots: Vec<AuditSnapshot>,
        allowlist_pattern: Option<String>,
    ) -> Self {
        Self {
            timestamp: current_timestamp(),
            command: command.into(),
            risk,
            matched_patterns,
            decision,
            snapshots,
            allowlist_pattern,
        }
    }
}

impl std::fmt::Display for Decision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Decision::Approved => "approved",
            Decision::Denied => "denied",
            Decision::AutoApproved => "auto-approved",
            Decision::Blocked => "blocked",
        };

        f.write_str(value)
    }
}

impl From<&Pattern> for MatchedPattern {
    fn from(pattern: &Pattern) -> Self {
        Self {
            id: pattern.id.to_string(),
            risk: pattern.risk,
            description: pattern.description.to_string(),
            safe_alt: pattern.safe_alt.as_ref().map(ToString::to_string),
        }
    }
}

impl From<&SnapshotRecord> for AuditSnapshot {
    fn from(snapshot: &SnapshotRecord) -> Self {
        Self {
            plugin: snapshot.plugin.to_string(),
            snapshot_id: snapshot.snapshot_id.clone(),
        }
    }
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new(default_audit_path())
    }
}

impl AuditLogger {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, entry: AuditEntry) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        serde_json::to_writer(&mut file, &entry)
            .map_err(|e| AegisError::Io(std::io::Error::other(e)))?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<AuditEntry>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();

        for (index, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let entry = serde_json::from_str::<AuditEntry>(&line).map_err(|e| {
                AegisError::Config(format!(
                    "failed to parse audit log line {} in {}: {e}",
                    index + 1,
                    self.path.display()
                ))
            })?;

            entries.push(entry);
        }

        Ok(entries)
    }

    pub fn query(&self, last: Option<usize>, risk: Option<RiskLevel>) -> Result<Vec<AuditEntry>> {
        let mut entries = self.read_all()?;

        if let Some(risk) = risk {
            entries.retain(|entry| entry.risk == risk);
        }

        if let Some(last) = last {
            let keep_from = entries.len().saturating_sub(last);
            entries = entries.split_off(keep_from);
        }

        Ok(entries)
    }

    pub fn format_entries(entries: &[AuditEntry]) -> String {
        if entries.is_empty() {
            return "No audit entries matched.\n".to_string();
        }

        let mut out = String::new();

        for entry in entries {
            out.push_str(&format!(
                "[{}] risk={} decision={}\n",
                entry.timestamp, entry.risk, entry.decision
            ));
            out.push_str(&format!("  command: {}\n", entry.command));

            if entry.matched_patterns.is_empty() {
                out.push_str("  matched: none\n");
            } else {
                let matched = entry
                    .matched_patterns
                    .iter()
                    .map(|pattern| format!("{} ({})", pattern.id, pattern.risk))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("  matched: {matched}\n"));
            }

            if entry.snapshots.is_empty() {
                out.push_str("  snapshots: none\n");
            } else {
                let snapshots = entry
                    .snapshots
                    .iter()
                    .map(|snapshot| format!("{}={}", snapshot.plugin, snapshot.snapshot_id))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("  snapshots: {snapshots}\n"));
            }

            if let Some(pattern) = &entry.allowlist_pattern {
                out.push_str(&format!("  allowlisted by: {pattern}\n"));
            }
        }

        out
    }
}

fn default_audit_path() -> PathBuf {
    let home = env::var_os("HOME").unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".aegis").join("audit.jsonl")
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn entry(index: usize, risk: RiskLevel) -> AuditEntry {
        AuditEntry {
            timestamp: 1_700_000_000 + index as u64,
            command: format!("command-{index}"),
            risk,
            matched_patterns: vec![MatchedPattern {
                id: format!("PAT-{index:03}"),
                risk,
                description: format!("pattern-{index}"),
                safe_alt: Some(format!("safe-{index}")),
            }],
            decision: match index % 4 {
                0 => Decision::Approved,
                1 => Decision::Denied,
                2 => Decision::AutoApproved,
                _ => Decision::Blocked,
            },
            snapshots: vec![AuditSnapshot {
                plugin: "git".to_string(),
                snapshot_id: format!("snap-{index}"),
            }],
            allowlist_pattern: None,
        }
    }

    #[test]
    fn append_and_read_back_five_entries_field_by_field() {
        let dir = TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

        let written = vec![
            entry(0, RiskLevel::Safe),
            entry(1, RiskLevel::Warn),
            entry(2, RiskLevel::Danger),
            entry(3, RiskLevel::Block),
            entry(4, RiskLevel::Warn),
        ];

        for entry in &written {
            logger.append(entry.clone()).unwrap();
        }

        let read_back = logger.read_all().unwrap();
        assert_eq!(read_back.len(), 5);

        for (expected, actual) in written.iter().zip(read_back.iter()) {
            assert_eq!(actual.timestamp, expected.timestamp);
            assert_eq!(actual.command, expected.command);
            assert_eq!(actual.risk, expected.risk);
            assert_eq!(actual.decision, expected.decision);
            assert_eq!(
                actual.matched_patterns.len(),
                expected.matched_patterns.len()
            );
            assert_eq!(actual.snapshots.len(), expected.snapshots.len());

            for (expected_pattern, actual_pattern) in expected
                .matched_patterns
                .iter()
                .zip(actual.matched_patterns.iter())
            {
                assert_eq!(actual_pattern.id, expected_pattern.id);
                assert_eq!(actual_pattern.risk, expected_pattern.risk);
                assert_eq!(actual_pattern.description, expected_pattern.description);
                assert_eq!(actual_pattern.safe_alt, expected_pattern.safe_alt);
            }

            for (expected_snapshot, actual_snapshot) in
                expected.snapshots.iter().zip(actual.snapshots.iter())
            {
                assert_eq!(actual_snapshot.plugin, expected_snapshot.plugin);
                assert_eq!(actual_snapshot.snapshot_id, expected_snapshot.snapshot_id);
            }
        }
    }

    #[test]
    fn query_filters_by_risk() {
        let dir = TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

        for (index, risk) in [
            RiskLevel::Safe,
            RiskLevel::Warn,
            RiskLevel::Danger,
            RiskLevel::Warn,
        ]
        .into_iter()
        .enumerate()
        {
            logger.append(entry(index, risk)).unwrap();
        }

        let entries = logger.query(None, Some(RiskLevel::Warn)).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| entry.risk == RiskLevel::Warn));
    }

    #[test]
    fn query_returns_last_n_entries() {
        let dir = TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

        for index in 0..5 {
            logger.append(entry(index, RiskLevel::Warn)).unwrap();
        }

        let entries = logger.query(Some(2), None).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].command, "command-3");
        assert_eq!(entries[1].command, "command-4");
    }
}
