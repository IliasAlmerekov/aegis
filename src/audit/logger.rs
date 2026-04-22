use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::config::{AuditIntegrityMode, Mode};
use crate::error::AegisError;
use crate::explanation::CommandExplanation;
use crate::interceptor::RiskLevel;
use crate::interceptor::patterns::{Category, PatternSource};
use crate::interceptor::scanner::MatchResult;
use crate::snapshot::SnapshotRecord;

mod integrity;
mod query;
mod rotation;
mod writer;

type Result<T> = std::result::Result<T, AegisError>;
static AUDIT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const CHAIN_ALG_SHA256: &str = "sha256";

/// RFC 3339 timestamp stored in the audit log.
///
/// New entries serialize as RFC 3339 / ISO 8601 strings with an explicit
/// timezone. Older logs that stored Unix seconds remain readable via the
/// custom deserializer below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AuditTimestamp(OffsetDateTime);

impl AuditTimestamp {
    fn now() -> Self {
        Self(OffsetDateTime::now_utc())
    }

    pub fn from_unix_seconds(seconds: i64) -> std::result::Result<Self, String> {
        OffsetDateTime::from_unix_timestamp(seconds)
            .map(Self)
            .map_err(|err| format!("invalid unix timestamp {seconds}: {err}"))
    }

    pub fn parse_rfc3339(value: &str) -> std::result::Result<Self, String> {
        OffsetDateTime::parse(value, &Rfc3339)
            .map(Self)
            .map_err(|err| format!("invalid RFC 3339 timestamp {value:?}: {err}"))
    }

    fn format_rfc3339(&self) -> String {
        self.0
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
    }
}

impl std::fmt::Display for AuditTimestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.format_rfc3339())
    }
}

impl Serialize for AuditTimestamp {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.format_rfc3339())
    }
}

impl<'de> Deserialize<'de> for AuditTimestamp {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum TimestampRepr {
            Rfc3339(String),
            UnixSecondsI64(i64),
            UnixSecondsU64(u64),
        }

        match TimestampRepr::deserialize(deserializer)? {
            TimestampRepr::Rfc3339(value) => OffsetDateTime::parse(&value, &Rfc3339)
                .map(Self)
                .map_err(|err| {
                    D::Error::custom(format!("invalid RFC 3339 timestamp {value:?}: {err}"))
                }),
            TimestampRepr::UnixSecondsI64(value) => {
                Self::from_unix_seconds(value).map_err(D::Error::custom)
            }
            TimestampRepr::UnixSecondsU64(value) => {
                let seconds = i64::try_from(value).map_err(|_| {
                    D::Error::custom(format!("timestamp {value} exceeds i64 range"))
                })?;
                Self::from_unix_seconds(seconds).map_err(D::Error::custom)
            }
        }
    }
}

/// One append-only audit record stored as a single JSON line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: AuditTimestamp,
    /// Monotonic sequence number within the current Aegis process.
    ///
    /// This disambiguates entries with very similar timestamps and preserves a
    /// stable in-process ordering signal without relying only on wall-clock
    /// time. Missing in older logs, so default to zero on deserialization.
    #[serde(default)]
    pub sequence: u64,
    pub command: String,
    pub risk: RiskLevel,
    pub matched_patterns: Vec<MatchedPattern>,
    /// Top-level projection of `matched_patterns[].id` for easier indexing in
    /// log aggregation systems.
    #[serde(default)]
    pub pattern_ids: Vec<String>,
    pub decision: Decision,
    pub snapshots: Vec<AuditSnapshot>,
    /// Nested consumer-facing explanation view built from planning/runtime facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<CommandExplanation>,
    /// Effective Aegis operating mode for this policy decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    /// Whether Aegis detected a CI environment while evaluating the command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci_detected: Option<bool>,
    /// Whether any allowlist rule matched the command/context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist_matched: Option<bool>,
    /// Whether the matched allowlist rule affected the final decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist_effective: Option<bool>,
    /// Hash chain algorithm used for this entry, when integrity mode is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_alg: Option<String>,
    /// Previous chained entry hash, or `None` for the first chained entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_hash: Option<String>,
    /// Hash of the canonical payload for this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_hash: Option<String>,
    /// The allowlist glob pattern that caused this command to be auto-approved,
    /// if any.  `None` when the command was not allowlisted.
    ///
    /// Skipped in JSON output when absent to keep old log entries valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist_pattern: Option<String>,

    /// The operator rationale attached to the allowlist rule, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist_reason: Option<String>,

    /// The agent/caller identity passed in the watch-mode input frame.
    /// Absent for shell-wrapper entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// The working directory from the watch-mode input frame.
    /// Absent for shell-wrapper entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,

    /// The correlation ID from the watch-mode input frame, echoed back.
    /// Absent for shell-wrapper entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Set to `"watch"` for entries created in watch mode.
    /// Absent for shell-wrapper entries, making them distinguishable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
}

/// User-visible outcome of the interception flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Approved,
    Denied,
    AutoApproved,
    Blocked,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditQuery {
    pub last: Option<usize>,
    pub risk: Option<RiskLevel>,
    pub decision: Option<Decision>,
    pub since: Option<AuditTimestamp>,
    pub until: Option<AuditTimestamp>,
    pub command_contains: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditSummary {
    pub total_entries: usize,
    pub decision_counts: DecisionCounts,
    pub risk_counts: RiskCounts,
    pub top_patterns: Vec<PatternCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct DecisionCounts {
    pub approved: usize,
    pub denied: usize,
    pub auto_approved: usize,
    pub blocked: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct RiskCounts {
    pub safe: usize,
    pub warn: usize,
    pub danger: usize,
    pub block: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PatternCount {
    pub id: String,
    pub count: usize,
}

/// Stable audit representation of a matched scanner pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchedPattern {
    pub id: String,
    pub risk: RiskLevel,
    pub description: String,
    pub safe_alt: Option<String>,
    /// Category of the pattern (e.g. Filesystem, Git). Optional for backwards compat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
    /// The actual substring of the command that triggered this pattern.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_text: Option<String>,
    /// Origin of this pattern in the runtime set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PatternSource>,
}

/// Stable audit representation of one snapshot created before execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditSnapshot {
    pub plugin: String,
    pub snapshot_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRotationPolicy {
    max_file_size_bytes: u64,
    retention_files: usize,
    compress_rotated: bool,
}

#[derive(Debug, Clone)]
struct ArchiveSegment {
    path: PathBuf,
    compressed: bool,
    index: usize,
}

struct AuditLock {
    file: std::fs::File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditIntegrityReport {
    pub status: AuditIntegrityStatus,
    pub checked_entries: usize,
    pub chained_entries: usize,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditIntegrityStatus {
    Verified,
    NoIntegrityData,
    Corrupt,
}

/// Append-only JSONL audit log stored under `~/.aegis/audit.jsonl`.
pub struct AuditLogger {
    path: PathBuf,
    rotation: Option<AuditRotationPolicy>,
    integrity_mode: AuditIntegrityMode,
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

impl std::str::FromStr for Decision {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "approved" => Ok(Self::Approved),
            "denied" => Ok(Self::Denied),
            "auto-approved" => Ok(Self::AutoApproved),
            "blocked" => Ok(Self::Blocked),
            other => Err(format!(
                "invalid decision '{other}', expected one of: approved, denied, auto-approved, blocked"
            )),
        }
    }
}

impl From<&MatchResult> for MatchedPattern {
    fn from(m: &MatchResult) -> Self {
        Self {
            id: m.pattern.id.to_string(),
            risk: m.pattern.risk,
            description: m.pattern.description.to_string(),
            safe_alt: m.pattern.safe_alt.as_ref().map(ToString::to_string),
            category: Some(m.pattern.category),
            matched_text: Some(m.matched_text.clone()),
            source: Some(m.pattern.source),
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

#[cfg(test)]
mod tests;
