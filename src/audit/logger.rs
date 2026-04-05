use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::config::AuditConfig;
use crate::error::AegisError;
use crate::interceptor::RiskLevel;
use crate::interceptor::patterns::Category;
use crate::interceptor::patterns::PatternSource;
use crate::interceptor::scanner::MatchResult;
use crate::snapshot::SnapshotRecord;

type Result<T> = std::result::Result<T, AegisError>;
static AUDIT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

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

    fn from_unix_seconds(seconds: i64) -> std::result::Result<Self, String> {
        OffsetDateTime::from_unix_timestamp(seconds)
            .map(Self)
            .map_err(|err| format!("invalid unix timestamp {seconds}: {err}"))
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
    pub decision: Decision,
    pub snapshots: Vec<AuditSnapshot>,
    /// The allowlist glob pattern that caused this command to be auto-approved,
    /// if any.  `None` when the command was not allowlisted.
    ///
    /// Skipped in JSON output when absent to keep old log entries valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist_pattern: Option<String>,

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

impl AuditRotationPolicy {
    pub fn from_config(config: &AuditConfig) -> Option<Self> {
        config.rotation_enabled.then_some(Self {
            max_file_size_bytes: config.max_file_size_bytes,
            retention_files: config.retention_files,
            compress_rotated: config.compress_rotated,
        })
    }
}

#[derive(Debug, Clone)]
struct ArchiveSegment {
    path: PathBuf,
    compressed: bool,
    index: usize,
}

/// Append-only JSONL audit log stored under `~/.aegis/audit.jsonl`.
pub struct AuditLogger {
    path: PathBuf,
    rotation: Option<AuditRotationPolicy>,
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
            sequence: next_sequence(),
            command: command.into(),
            risk,
            matched_patterns,
            decision,
            snapshots,
            allowlist_pattern,
            source: None,
            cwd: None,
            id: None,
            transport: None,
        }
    }

    /// Attach watch-mode context fields and set `transport = "watch"`.
    pub fn with_watch_context(
        mut self,
        source: Option<String>,
        cwd: Option<String>,
        id: Option<String>,
    ) -> Self {
        self.source = source;
        self.cwd = cwd;
        self.id = id;
        self.transport = Some("watch".to_string());
        self
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

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new(default_audit_path())
    }
}

impl AuditLogger {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            rotation: None,
        }
    }

    pub fn with_rotation(mut self, policy: AuditRotationPolicy) -> Self {
        self.rotation = Some(policy);
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, entry: AuditEntry) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut serialized =
            serde_json::to_vec(&entry).map_err(|e| AegisError::Io(std::io::Error::other(e)))?;
        serialized.push(b'\n');

        if let Some(policy) = &self.rotation {
            self.rotate_if_needed(policy, serialized.len() as u64)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        file.write_all(&serialized)?;
        file.flush()?;
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<AuditEntry>> {
        let mut entries = Vec::new();
        for segment in self.segments_oldest_to_newest()? {
            self.extend_entries_from_segment(&segment, None, &mut entries)?;
        }
        Ok(entries)
    }

    pub fn query(&self, last: Option<usize>, risk: Option<RiskLevel>) -> Result<Vec<AuditEntry>> {
        match (last, risk) {
            (Some(last), risk) => self.read_last_matching(last, risk),
            (None, Some(risk)) => self.read_matching(risk),
            (None, None) => self.read_all(),
        }
    }

    pub fn format_entries(entries: &[AuditEntry]) -> String {
        if entries.is_empty() {
            return "No audit entries matched.\n".to_string();
        }

        let mut out = String::new();

        for entry in entries {
            out.push_str(&format!("[{}]", entry.timestamp));
            if entry.sequence > 0 {
                out.push_str(&format!(" seq={}", entry.sequence));
            }
            out.push_str(&format!(
                " risk={} decision={}\n",
                entry.risk, entry.decision
            ));
            out.push_str(&format!("  command: {}\n", entry.command));

            if entry.matched_patterns.is_empty() {
                out.push_str("  matched: none\n");
            } else {
                let matched = entry
                    .matched_patterns
                    .iter()
                    .map(|pattern| {
                        let source = pattern
                            .source
                            .map(|source| match source {
                                PatternSource::Builtin => ", source=builtin".to_string(),
                                PatternSource::Custom => ", source=custom".to_string(),
                            })
                            .unwrap_or_default();
                        format!("{} ({}{})", pattern.id, pattern.risk, source)
                    })
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

    fn read_matching(&self, risk: RiskLevel) -> Result<Vec<AuditEntry>> {
        let mut entries = Vec::new();
        for segment in self.segments_oldest_to_newest()? {
            self.extend_entries_from_segment(&segment, Some(risk), &mut entries)?;
        }
        Ok(entries)
    }

    fn read_last_matching(&self, last: usize, risk: Option<RiskLevel>) -> Result<Vec<AuditEntry>> {
        if last == 0 {
            return Ok(Vec::new());
        }

        let mut remaining = last;
        let mut newest_segments = Vec::new();

        if self.path.exists() {
            let current = ArchiveSegment {
                path: self.path.clone(),
                compressed: false,
                index: 0,
            };
            let tail = self.read_last_matching_from_plain_segment(&current, remaining, risk)?;
            remaining = remaining.saturating_sub(tail.len());
            if !tail.is_empty() {
                newest_segments.push(tail);
            }
        }

        for segment in self.segments_newest_archive_first()? {
            if remaining == 0 {
                break;
            }

            let mut entries = self.read_entries_from_segment(&segment, risk)?;
            let keep_from = entries.len().saturating_sub(remaining);
            entries = entries.split_off(keep_from);
            remaining = remaining.saturating_sub(entries.len());

            if !entries.is_empty() {
                newest_segments.push(entries);
            }
        }

        newest_segments.reverse();
        Ok(newest_segments.into_iter().flatten().collect())
    }

    fn parse_entry_line(
        &self,
        line: &[u8],
        source: &Path,
        line_number: Option<usize>,
    ) -> Result<Option<AuditEntry>> {
        if line.iter().all(|byte| byte.is_ascii_whitespace()) {
            return Ok(None);
        }

        serde_json::from_slice::<AuditEntry>(line)
            .map(Some)
            .map_err(|err| match line_number {
                Some(index) => AegisError::Config(format!(
                    "failed to parse audit log line {} in {}: {err}",
                    index,
                    source.display()
                )),
                None => AegisError::Config(format!(
                    "failed to parse audit log while scanning tail of {}: {err}",
                    source.display()
                )),
            })
    }

    fn extend_entries_from_segment(
        &self,
        segment: &ArchiveSegment,
        risk: Option<RiskLevel>,
        out: &mut Vec<AuditEntry>,
    ) -> Result<()> {
        for entry in self.read_entries_from_segment(segment, risk)? {
            out.push(entry);
        }
        Ok(())
    }

    fn read_entries_from_segment(
        &self,
        segment: &ArchiveSegment,
        risk: Option<RiskLevel>,
    ) -> Result<Vec<AuditEntry>> {
        let reader = self.open_segment_reader(segment)?;
        let mut entries = Vec::new();

        for (index, line) in reader.lines().enumerate() {
            let Some(entry) =
                self.parse_entry_line(line?.as_bytes(), &segment.path, Some(index + 1))?
            else {
                continue;
            };

            if risk.is_none_or(|expected| entry.risk == expected) {
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    fn read_last_matching_from_plain_segment(
        &self,
        segment: &ArchiveSegment,
        last: usize,
        risk: Option<RiskLevel>,
    ) -> Result<Vec<AuditEntry>> {
        if last == 0 {
            return Ok(Vec::new());
        }

        let mut file = File::open(&segment.path)?;
        let mut position = file.seek(SeekFrom::End(0))?;
        let mut entries = Vec::with_capacity(last);
        let mut remainder = Vec::new();
        let mut chunk = vec![0_u8; 8192];

        while position > 0 && entries.len() < last {
            let read_size = usize::try_from(position.min(chunk.len() as u64))
                .expect("chunk size is always bounded by usize");
            position -= read_size as u64;
            file.seek(SeekFrom::Start(position))?;
            file.read_exact(&mut chunk[..read_size])?;

            let mut buffer = Vec::with_capacity(read_size + remainder.len());
            buffer.extend_from_slice(&chunk[..read_size]);
            buffer.extend_from_slice(&remainder);

            let split_at = if position == 0 {
                0
            } else {
                match buffer.iter().position(|byte| *byte == b'\n') {
                    Some(index) => index + 1,
                    None => {
                        remainder = buffer;
                        continue;
                    }
                }
            };

            remainder = buffer[..split_at].to_vec();

            for line in buffer[split_at..].split(|byte| *byte == b'\n').rev() {
                if entries.len() >= last {
                    break;
                }

                let Some(entry) = self.parse_entry_line(line, &segment.path, None)? else {
                    continue;
                };

                if risk.is_none_or(|expected| entry.risk == expected) {
                    entries.push(entry);
                }
            }
        }

        if entries.len() < last {
            let Some(entry) = self.parse_entry_line(&remainder, &segment.path, None)? else {
                entries.reverse();
                return Ok(entries);
            };

            if risk.is_none_or(|expected| entry.risk == expected) {
                entries.push(entry);
            }
        }

        entries.reverse();
        Ok(entries)
    }

    fn open_segment_reader(&self, segment: &ArchiveSegment) -> Result<Box<dyn BufRead>> {
        let file = File::open(&segment.path)?;
        if segment.compressed {
            Ok(Box::new(BufReader::new(GzDecoder::new(file))))
        } else {
            Ok(Box::new(BufReader::new(file)))
        }
    }

    fn segments_oldest_to_newest(&self) -> Result<Vec<ArchiveSegment>> {
        let mut segments = self.discover_archives()?;
        segments.sort_by_key(|segment| segment.index);
        segments.reverse();
        if self.path.exists() {
            segments.push(ArchiveSegment {
                path: self.path.clone(),
                compressed: false,
                index: 0,
            });
        }
        Ok(segments)
    }

    fn segments_newest_archive_first(&self) -> Result<Vec<ArchiveSegment>> {
        let mut segments = self.discover_archives()?;
        segments.sort_by_key(|segment| segment.index);
        Ok(segments)
    }

    fn discover_archives(&self) -> Result<Vec<ArchiveSegment>> {
        let Some(parent) = self.path.parent() else {
            return Ok(Vec::new());
        };
        let Some(base_name) = self.path.file_name().and_then(|name| name.to_str()) else {
            return Ok(Vec::new());
        };

        let mut segments = Vec::new();
        let prefix = format!("{base_name}.");

        if !parent.exists() {
            return Ok(segments);
        }

        for entry in fs::read_dir(parent)? {
            let entry = entry?;
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };

            let Some(rest) = file_name.strip_prefix(&prefix) else {
                continue;
            };

            let (index_part, compressed) = match rest.strip_suffix(".gz") {
                Some(index) => (index, true),
                None => (rest, false),
            };

            let Ok(index) = index_part.parse::<usize>() else {
                continue;
            };

            if index == 0 {
                continue;
            }

            segments.push(ArchiveSegment {
                path: entry.path(),
                compressed,
                index,
            });
        }

        segments.sort_by(|left, right| {
            left.index
                .cmp(&right.index)
                .then(right.compressed.cmp(&left.compressed))
        });
        segments.dedup_by(|left, right| left.index == right.index);
        Ok(segments)
    }

    fn rotate_if_needed(&self, policy: &AuditRotationPolicy, incoming_bytes: u64) -> Result<()> {
        if !self.path.exists() {
            return Ok(());
        }

        let current_size = fs::metadata(&self.path)?.len();
        if current_size.saturating_add(incoming_bytes) <= policy.max_file_size_bytes {
            return Ok(());
        }

        self.rotate(policy)
    }

    fn rotate(&self, policy: &AuditRotationPolicy) -> Result<()> {
        self.remove_existing_archive(policy.retention_files)?;

        for index in (1..policy.retention_files).rev() {
            if let Some(source) = self.existing_archive_path(index) {
                let destination = if source
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext == "gz")
                {
                    self.archive_path(index + 1, true)
                } else {
                    self.archive_path(index + 1, false)
                };
                if destination.exists() {
                    fs::remove_file(&destination)?;
                }
                fs::rename(source, destination)?;
            }
        }

        if policy.compress_rotated {
            self.compress_current_to_archive(&self.archive_path(1, true))?;
        } else {
            let destination = self.archive_path(1, false);
            if destination.exists() {
                fs::remove_file(&destination)?;
            }
            fs::rename(&self.path, destination)?;
        }

        Ok(())
    }

    fn compress_current_to_archive(&self, destination: &Path) -> Result<()> {
        if destination.exists() {
            fs::remove_file(destination)?;
        }

        let mut source = File::open(&self.path)?;
        let archive = File::create(destination)?;
        let mut encoder = GzEncoder::new(archive, Compression::default());
        std::io::copy(&mut source, &mut encoder)?;
        encoder.finish()?;
        fs::remove_file(&self.path)?;
        Ok(())
    }

    fn remove_existing_archive(&self, index: usize) -> Result<()> {
        for path in [
            self.archive_path(index, false),
            self.archive_path(index, true),
        ] {
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    fn existing_archive_path(&self, index: usize) -> Option<PathBuf> {
        [
            self.archive_path(index, true),
            self.archive_path(index, false),
        ]
        .into_iter()
        .find(|path| path.exists())
    }

    fn archive_path(&self, index: usize, compressed: bool) -> PathBuf {
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| {
                if compressed {
                    format!("{name}.{index}.gz")
                } else {
                    format!("{name}.{index}")
                }
            })
            .unwrap_or_else(|| {
                if compressed {
                    format!("audit.jsonl.{index}.gz")
                } else {
                    format!("audit.jsonl.{index}")
                }
            });

        self.path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(file_name)
    }
}

fn default_audit_path() -> PathBuf {
    let home = env::var_os("HOME").unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".aegis").join("audit.jsonl")
}

fn current_timestamp() -> AuditTimestamp {
    AuditTimestamp::now()
}

fn next_sequence() -> u64 {
    AUDIT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn entry(index: usize, risk: RiskLevel) -> AuditEntry {
        AuditEntry {
            timestamp: AuditTimestamp::from_unix_seconds(1_700_000_000 + index as i64).unwrap(),
            sequence: index as u64 + 1,
            command: format!("command-{index}"),
            risk,
            matched_patterns: vec![MatchedPattern {
                id: format!("PAT-{index:03}"),
                risk,
                description: format!("pattern-{index}"),
                safe_alt: Some(format!("safe-{index}")),
                category: None,
                matched_text: None,
                source: None,
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
            source: None,
            cwd: None,
            id: None,
            transport: None,
        }
    }

    fn entry_bytes(index: usize, risk: RiskLevel) -> usize {
        let mut bytes = serde_json::to_vec(&entry(index, risk)).unwrap();
        bytes.push(b'\n');
        bytes.len()
    }

    fn rotation_policy(
        max_file_size_bytes: u64,
        retention_files: usize,
        compress_rotated: bool,
    ) -> AuditRotationPolicy {
        AuditRotationPolicy {
            max_file_size_bytes,
            retention_files,
            compress_rotated,
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

    #[test]
    fn query_returns_last_n_entries_for_matching_risk_only() {
        let dir = TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

        for (index, risk) in [
            RiskLevel::Safe,
            RiskLevel::Warn,
            RiskLevel::Danger,
            RiskLevel::Warn,
            RiskLevel::Danger,
            RiskLevel::Warn,
        ]
        .into_iter()
        .enumerate()
        {
            logger.append(entry(index, risk)).unwrap();
        }

        let entries = logger.query(Some(2), Some(RiskLevel::Warn)).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].command, "command-3");
        assert_eq!(entries[1].command, "command-5");
    }

    #[test]
    fn query_last_handles_missing_trailing_newline() {
        let dir = TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
        let lines = [
            entry(0, RiskLevel::Safe),
            entry(1, RiskLevel::Warn),
            entry(2, RiskLevel::Danger),
        ]
        .into_iter()
        .map(|entry| serde_json::to_string(&entry).unwrap())
        .collect::<Vec<_>>()
        .join("\n");

        fs::write(logger.path(), lines).unwrap();

        let entries = logger.query(Some(2), None).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].command, "command-1");
        assert_eq!(entries[1].command, "command-2");
    }

    #[test]
    fn append_serializes_rfc3339_timestamp_and_sequence() {
        let dir = TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

        logger.append(entry(0, RiskLevel::Safe)).unwrap();

        let written = fs::read_to_string(logger.path()).unwrap();
        let json: serde_json::Value = serde_json::from_str(written.trim()).unwrap();

        assert_eq!(json["timestamp"], "2023-11-14T22:13:20Z");
        assert_eq!(json["sequence"], 1);
    }

    #[test]
    fn read_all_accepts_legacy_unix_seconds_timestamp() {
        let dir = TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
        let legacy_entry = r#"{"timestamp":1700000000,"command":"legacy","risk":"Safe","matched_patterns":[],"decision":"AutoApproved","snapshots":[]}"#;

        fs::write(logger.path(), format!("{legacy_entry}\n")).unwrap();

        let entries = logger.read_all().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].timestamp.to_string(), "2023-11-14T22:13:20Z");
        assert_eq!(entries[0].sequence, 0);
    }

    #[test]
    fn rotation_keeps_archives_and_queries_span_them() {
        let dir = TempDir::new().unwrap();
        let max_bytes = entry_bytes(0, RiskLevel::Warn) as u64 + 1;
        let logger = AuditLogger::new(dir.path().join("audit.jsonl"))
            .with_rotation(rotation_policy(max_bytes, 3, false));

        for index in 0..3 {
            logger.append(entry(index, RiskLevel::Warn)).unwrap();
        }

        assert!(dir.path().join("audit.jsonl.1").exists());
        assert!(dir.path().join("audit.jsonl.2").exists());

        let all = logger.read_all().unwrap();
        assert_eq!(
            all.iter()
                .map(|entry| entry.command.as_str())
                .collect::<Vec<_>>(),
            vec!["command-0", "command-1", "command-2",]
        );

        let last = logger.query(Some(2), None).unwrap();
        assert_eq!(
            last.iter()
                .map(|entry| entry.command.as_str())
                .collect::<Vec<_>>(),
            vec!["command-1", "command-2"]
        );
    }

    #[test]
    fn rotation_can_compress_archives_and_still_read_them() {
        let dir = TempDir::new().unwrap();
        let max_bytes = entry_bytes(0, RiskLevel::Warn) as u64 + 1;
        let logger = AuditLogger::new(dir.path().join("audit.jsonl"))
            .with_rotation(rotation_policy(max_bytes, 2, true));

        logger.append(entry(0, RiskLevel::Warn)).unwrap();
        logger.append(entry(1, RiskLevel::Warn)).unwrap();

        let archive_path = dir.path().join("audit.jsonl.1.gz");
        assert!(archive_path.exists());

        let mut decompressed = String::new();
        GzDecoder::new(File::open(&archive_path).unwrap())
            .read_to_string(&mut decompressed)
            .unwrap();
        assert!(decompressed.contains("command-0"));

        let all = logger.read_all().unwrap();
        assert_eq!(
            all.iter()
                .map(|entry| entry.command.as_str())
                .collect::<Vec<_>>(),
            vec!["command-0", "command-1"]
        );
    }

    #[test]
    fn rotation_enforces_retention_limit() {
        let dir = TempDir::new().unwrap();
        let max_bytes = entry_bytes(0, RiskLevel::Warn) as u64 + 1;
        let logger = AuditLogger::new(dir.path().join("audit.jsonl"))
            .with_rotation(rotation_policy(max_bytes, 2, false));

        for index in 0..4 {
            logger.append(entry(index, RiskLevel::Warn)).unwrap();
        }

        assert!(dir.path().join("audit.jsonl.1").exists());
        assert!(dir.path().join("audit.jsonl.2").exists());
        assert!(!dir.path().join("audit.jsonl.3").exists());

        let all = logger.read_all().unwrap();
        assert_eq!(
            all.iter()
                .map(|entry| entry.command.as_str())
                .collect::<Vec<_>>(),
            vec!["command-1", "command-2", "command-3"]
        );
    }

    #[test]
    fn watch_context_fields_round_trip_through_json() {
        let entry = AuditEntry::new(
            "git status",
            RiskLevel::Safe,
            vec![],
            Decision::AutoApproved,
            vec![],
            None,
        )
        .with_watch_context(
            Some("claude".to_string()),
            Some("/home/user/project".to_string()),
            Some("frame-42".to_string()),
        );

        let json = serde_json::to_string(&entry).unwrap();
        let back: AuditEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(back.source.as_deref(), Some("claude"));
        assert_eq!(back.cwd.as_deref(), Some("/home/user/project"));
        assert_eq!(back.id.as_deref(), Some("frame-42"));
        assert_eq!(back.transport.as_deref(), Some("watch"));
    }

    #[test]
    fn watch_context_fields_absent_when_not_set() {
        let entry = AuditEntry::new(
            "ls",
            RiskLevel::Safe,
            vec![],
            Decision::AutoApproved,
            vec![],
            None,
        );

        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("source"), "source must be absent when None");
        assert!(!json.contains("transport"), "transport must be absent when None");
    }
}
