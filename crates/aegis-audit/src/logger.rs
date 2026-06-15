//! Audit logger: append-only JSONL with optional hash-chain integrity.

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use aegis_config::{AuditIntegrityMode, Mode};
use aegis_explanation::CommandExplanation;
use aegis_scanner::MatchResult;
use aegis_types::{Category, PatternSource, RiskLevel, SnapshotRecord};

use crate::error::AuditError;

mod integrity;
mod query;
mod rotation;
mod writer;

type Result<T> = std::result::Result<T, AuditError>;
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

    /// Create a timestamp from Unix seconds.
    pub fn from_unix_seconds(seconds: i64) -> std::result::Result<Self, String> {
        OffsetDateTime::from_unix_timestamp(seconds)
            .map(Self)
            .map_err(|err| format!("invalid unix timestamp {seconds}: {err}"))
    }

    /// Parse a timestamp from an RFC 3339 string.
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

/// Core fields present in every audit entry.
///
/// Shell-wrapper and direct-invocation entries are stored as
/// `AuditEntry::Decision(DecisionEntry)`. Watch-mode entries are
/// `AuditEntry::Watch(WatchEntry)`, which embeds this struct via `base`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionEntry {
    /// When the decision was recorded.
    pub timestamp: AuditTimestamp,
    /// Monotonic sequence number within the current Aegis process.
    ///
    /// Disambiguates entries with very similar timestamps. Zero in older logs
    /// that predate this field.
    pub sequence: u64,
    /// The command string that was intercepted.
    pub command: String,
    /// Risk level assessed by the scanner.
    pub risk: RiskLevel,
    /// Patterns that matched the command.
    pub matched_patterns: Vec<MatchedPattern>,
    /// Top-level projection of `matched_patterns[].id` for log aggregation.
    pub pattern_ids: Vec<String>,
    /// Final user or system decision.
    pub decision: Decision,
    /// Snapshots taken before command execution.
    pub snapshots: Vec<AuditSnapshot>,
    /// Human-readable explanation of the decision.
    pub explanation: Option<CommandExplanation>,
    /// Aegis operating mode at the time.
    pub mode: Option<Mode>,
    /// Whether a CI environment was detected.
    pub ci_detected: Option<bool>,
    /// Whether an allowlist pattern matched.
    pub allowlist_matched: Option<bool>,
    /// Whether the allowlist overrode the risk level.
    pub allowlist_effective: Option<bool>,
    /// Hash algorithm used for the integrity chain.
    pub chain_alg: Option<String>,
    /// Hash of the previous audit entry.
    pub prev_hash: Option<String>,
    /// Hash of this audit entry.
    pub entry_hash: Option<String>,
    /// ID of the allowlist pattern that matched.
    pub allowlist_pattern: Option<String>,
    /// Reason the allowlist matched.
    pub allowlist_reason: Option<String>,
    /// Whether a sandbox profile was active for this execution.
    pub sandbox_active: Option<bool>,
}

/// Watch-mode audit entry.
///
/// Contains all `DecisionEntry` fields via `base`, plus the watch-frame
/// correlation fields.  `source`, `cwd` and `id` are `Option<String>` so that
/// legacy audit log lines which omit them still deserialize correctly.
/// `transport` is implicit — it is always `"watch"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEntry {
    /// Core decision fields shared with shell-wrapper entries.
    pub base: DecisionEntry,
    /// Agent/caller identity from the watch-mode input frame.
    pub source: Option<String>,
    /// Working directory from the watch-mode input frame.
    pub cwd: Option<String>,
    /// Correlation ID from the watch-mode input frame.
    pub id: Option<String>,
}

/// One append-only audit record stored as a single JSON line.
///
/// Shell-wrapper entries are `Decision`; watch-mode entries are `Watch`.
/// Both serialise to the same flat JSON format for backwards compatibility
/// with existing `~/.aegis/audit.jsonl` files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditEntry {
    /// Shell-wrapper or direct-invocation entry.
    Decision(DecisionEntry),
    /// Watch-mode entry with extra correlation context.
    Watch(WatchEntry),
}

impl AuditEntry {
    /// Shared reference to the common decision fields, regardless of variant.
    pub fn as_base(&self) -> &DecisionEntry {
        match self {
            AuditEntry::Decision(e) => e,
            AuditEntry::Watch(w) => &w.base,
        }
    }

    pub(super) fn as_base_mut(&mut self) -> &mut DecisionEntry {
        match self {
            AuditEntry::Decision(e) => e,
            AuditEntry::Watch(w) => &mut w.base,
        }
    }

    /// Returns `Some` only for watch-mode entries.
    pub fn watch_context(&self) -> Option<&WatchEntry> {
        match self {
            AuditEntry::Watch(w) => Some(w),
            _ => None,
        }
    }
}

// Private flat struct used exclusively for JSON serde.
// Preserves the on-disk format so existing audit logs remain readable.
#[derive(Serialize, Deserialize)]
struct AuditEntryFlat {
    timestamp: AuditTimestamp,
    #[serde(default)]
    sequence: u64,
    command: String,
    risk: RiskLevel,
    matched_patterns: Vec<MatchedPattern>,
    #[serde(default)]
    pattern_ids: Vec<String>,
    decision: Decision,
    snapshots: Vec<AuditSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    explanation: Option<CommandExplanation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<Mode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ci_detected: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allowlist_matched: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allowlist_effective: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chain_alg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prev_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entry_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allowlist_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allowlist_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sandbox_active: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transport: Option<String>,
}

impl From<AuditEntryFlat> for AuditEntry {
    fn from(flat: AuditEntryFlat) -> Self {
        let is_watch = flat.transport.as_deref() == Some("watch")
            || flat.source.is_some()
            || flat.cwd.is_some()
            || flat.id.is_some();
        let base = DecisionEntry {
            timestamp: flat.timestamp,
            sequence: flat.sequence,
            command: flat.command,
            risk: flat.risk,
            matched_patterns: flat.matched_patterns,
            pattern_ids: flat.pattern_ids,
            decision: flat.decision,
            snapshots: flat.snapshots,
            explanation: flat.explanation,
            mode: flat.mode,
            ci_detected: flat.ci_detected,
            allowlist_matched: flat.allowlist_matched,
            allowlist_effective: flat.allowlist_effective,
            chain_alg: flat.chain_alg,
            prev_hash: flat.prev_hash,
            entry_hash: flat.entry_hash,
            allowlist_pattern: flat.allowlist_pattern,
            allowlist_reason: flat.allowlist_reason,
            sandbox_active: flat.sandbox_active,
        };
        if is_watch {
            AuditEntry::Watch(WatchEntry {
                base,
                source: flat.source,
                cwd: flat.cwd,
                id: flat.id,
            })
        } else {
            AuditEntry::Decision(base)
        }
    }
}

impl From<&AuditEntry> for AuditEntryFlat {
    fn from(entry: &AuditEntry) -> Self {
        let base = entry.as_base();
        let (source, cwd, id, transport) = match entry {
            AuditEntry::Watch(w) => (
                w.source.clone(),
                w.cwd.clone(),
                w.id.clone(),
                Some("watch".to_string()),
            ),
            AuditEntry::Decision(_) => (None, None, None, None),
        };
        Self {
            timestamp: base.timestamp,
            sequence: base.sequence,
            command: base.command.clone(),
            risk: base.risk,
            matched_patterns: base.matched_patterns.clone(),
            pattern_ids: base.pattern_ids.clone(),
            decision: base.decision,
            snapshots: base.snapshots.clone(),
            explanation: base.explanation.clone(),
            mode: base.mode,
            ci_detected: base.ci_detected,
            allowlist_matched: base.allowlist_matched,
            allowlist_effective: base.allowlist_effective,
            chain_alg: base.chain_alg.clone(),
            prev_hash: base.prev_hash.clone(),
            entry_hash: base.entry_hash.clone(),
            allowlist_pattern: base.allowlist_pattern.clone(),
            allowlist_reason: base.allowlist_reason.clone(),
            sandbox_active: base.sandbox_active,
            source,
            cwd,
            id,
            transport,
        }
    }
}

impl Serialize for AuditEntry {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        AuditEntryFlat::from(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AuditEntry {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        AuditEntryFlat::deserialize(deserializer).map(AuditEntry::from)
    }
}

pub use aegis_types::Decision;

/// Parameters for filtering the audit log.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditQuery {
    /// Maximum number of recent entries to return.
    pub last: Option<usize>,
    /// Filter by assessed risk level.
    pub risk: Option<RiskLevel>,
    /// Filter by final decision.
    pub decision: Option<Decision>,
    /// Only entries at or after this timestamp.
    pub since: Option<AuditTimestamp>,
    /// Only entries at or before this timestamp.
    pub until: Option<AuditTimestamp>,
    /// Only entries whose command contains this substring.
    pub command_contains: Option<String>,
}

/// Aggregated statistics over a slice of the audit log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditSummary {
    /// Total number of entries matched by the query.
    pub total_entries: usize,
    /// Breakdown of decisions.
    pub decision_counts: DecisionCounts,
    /// Breakdown of risk levels.
    pub risk_counts: RiskCounts,
    /// Most frequently matched patterns.
    pub top_patterns: Vec<PatternCount>,
}

/// Count of each decision type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct DecisionCounts {
    /// Number of `Approved` decisions.
    pub approved: usize,
    /// Number of `Denied` decisions.
    pub denied: usize,
    /// Number of `AutoApproved` decisions.
    pub auto_approved: usize,
    /// Number of `Blocked` decisions.
    pub blocked: usize,
}

/// Count of each risk level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct RiskCounts {
    /// Number of `Safe` assessments.
    pub safe: usize,
    /// Number of `Warn` assessments.
    pub warn: usize,
    /// Number of `Danger` assessments.
    pub danger: usize,
    /// Number of `Block` assessments.
    pub block: usize,
}

/// How many times a specific pattern matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PatternCount {
    /// Pattern identifier.
    pub id: String,
    /// Number of occurrences.
    pub count: usize,
}

/// Stable audit representation of a matched scanner pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchedPattern {
    /// Pattern identifier (e.g. "FS-001").
    pub id: String,
    /// Risk level assigned by the pattern.
    pub risk: RiskLevel,
    /// Human-readable description of what the pattern detects.
    pub description: String,
    /// Suggested safe alternative, if any.
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
    /// Name of the snapshot plugin that created it.
    pub plugin: String,
    /// Opaque identifier used to restore the snapshot.
    pub snapshot_id: String,
}

/// Policy for rotating the append-only audit log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRotationPolicy {
    max_file_size_bytes: u64,
    retention_files: usize,
    compress_rotated: bool,
}

#[derive(Debug, Clone)]
struct ArchiveSegment {
    path: std::path::PathBuf,
    compressed: bool,
    index: usize,
}

struct AuditLock {
    file: std::fs::File,
}

/// Result of verifying the cryptographic integrity chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditIntegrityReport {
    /// Overall verification status.
    pub status: AuditIntegrityStatus,
    /// Total entries examined.
    pub checked_entries: usize,
    /// Entries linked by a valid hash chain.
    pub chained_entries: usize,
    /// Human-readable summary message.
    pub message: String,
}

/// Outcome of an integrity chain verification pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditIntegrityStatus {
    /// Every chained entry verified successfully.
    Verified,
    /// No integrity metadata present in the log.
    NoIntegrityData,
    /// At least one chain link is broken or tampered.
    Corrupt,
}

/// Append-only JSONL audit log stored under `~/.aegis/audit.jsonl`.
pub struct AuditLogger {
    pub(super) path: PathBuf,
    rotation: Option<AuditRotationPolicy>,
    integrity_mode: AuditIntegrityMode,
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
