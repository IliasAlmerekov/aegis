use serde::Serialize;
use sha2::{Digest, Sha256};

use super::*;
use crate::error::AuditError;

impl AuditLogger {
    /// Verify the integrity of the entire audit log.
    #[must_use = "integrity check result must be inspected"]
    pub fn verify_integrity(&self) -> Result<AuditIntegrityReport> {
        let entries = self.read_all()?;
        Ok(verify_integrity_entries(&entries))
    }

    pub(super) fn apply_integrity(
        &self,
        entry: AuditEntry,
        prev_hash: Option<String>,
    ) -> Result<AuditEntry> {
        match self.integrity_mode {
            AuditIntegrityMode::Off => Ok(entry),
            AuditIntegrityMode::ChainSha256 => {
                let entry_hash = compute_entry_hash(&entry, prev_hash.as_deref())?;
                Ok(entry.with_integrity_chain(prev_hash, entry_hash))
            }
        }
    }

    pub(super) fn latest_chained_hash(&self) -> Result<Option<String>> {
        if let Some(entry) = self.read_last_entry_from_plain_file(&self.path)? {
            return Ok(entry.as_base().entry_hash.clone());
        }

        if let Some(path) = self.existing_archive_path(1) {
            let compressed = path.extension().and_then(|ext| ext.to_str()) == Some("gz");
            let segment = ArchiveSegment {
                path,
                compressed,
                index: 1,
            };
            let entries = self.read_entries_from_segment(&segment, None)?;
            return Ok(entries
                .last()
                .and_then(|entry| entry.as_base().entry_hash.clone()));
        }

        Ok(None)
    }
}

#[derive(Serialize)]
pub(super) struct AuditIntegrityPayload<'a> {
    pub(super) timestamp: AuditTimestamp,
    pub(super) sequence: u64,
    pub(super) command: &'a str,
    pub(super) risk: RiskLevel,
    pub(super) matched_patterns: &'a [MatchedPattern],
    pub(super) pattern_ids: &'a [String],
    pub(super) decision: Decision,
    pub(super) snapshots: &'a [AuditSnapshot],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) explanation: Option<&'a CommandExplanation>,
    pub(super) mode: Option<Mode>,
    pub(super) ci_detected: Option<bool>,
    pub(super) allowlist_matched: Option<bool>,
    pub(super) allowlist_effective: Option<bool>,
    pub(super) chain_alg: &'a str,
    pub(super) prev_hash: Option<&'a str>,
    pub(super) allowlist_pattern: Option<&'a str>,
    pub(super) allowlist_reason: Option<&'a str>,
    pub(super) source: Option<&'a str>,
    pub(super) cwd: Option<&'a str>,
    pub(super) id: Option<&'a str>,
    pub(super) transport: Option<&'a str>,
}

pub(super) fn compute_entry_hash(entry: &AuditEntry, prev_hash: Option<&str>) -> Result<String> {
    let base = entry.as_base();
    let (source, cwd, id, transport): (Option<&str>, Option<&str>, Option<&str>, Option<&str>) =
        match entry {
            AuditEntry::Watch(w) => (
                w.source.as_deref(),
                w.cwd.as_deref(),
                w.id.as_deref(),
                Some("watch"),
            ),
            AuditEntry::Decision(_) => (None, None, None, None),
        };
    let payload = AuditIntegrityPayload {
        timestamp: base.timestamp,
        sequence: base.sequence,
        command: &base.command,
        risk: base.risk,
        matched_patterns: &base.matched_patterns,
        pattern_ids: &base.pattern_ids,
        decision: base.decision,
        snapshots: &base.snapshots,
        explanation: base.explanation.as_ref(),
        mode: base.mode,
        ci_detected: base.ci_detected,
        allowlist_matched: base.allowlist_matched,
        allowlist_effective: base.allowlist_effective,
        chain_alg: CHAIN_ALG_SHA256,
        prev_hash,
        allowlist_pattern: base.allowlist_pattern.as_deref(),
        allowlist_reason: base.allowlist_reason.as_deref(),
        source,
        cwd,
        id,
        transport,
    };

    let canonical = serde_json::to_vec(&payload).map_err(|err| {
        AuditError::Parse(format!(
            "failed to serialize audit integrity payload: {err}"
        ))
    })?;
    let digest = Sha256::digest(canonical);
    Ok(hex_encode(&digest))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

pub(super) fn verify_integrity_entries(entries: &[AuditEntry]) -> AuditIntegrityReport {
    let mut chained_entries = 0usize;
    let mut previous_hash: Option<String> = None;
    let mut seen_chain = false;

    for (index, entry) in entries.iter().enumerate() {
        let base = entry.as_base();
        let is_chained =
            base.entry_hash.is_some() || base.prev_hash.is_some() || base.chain_alg.is_some();

        if !is_chained {
            if seen_chain {
                return AuditIntegrityReport {
                    status: AuditIntegrityStatus::Corrupt,
                    checked_entries: entries.len(),
                    chained_entries,
                    message: format!(
                        "audit integrity failure at entry {}: encountered an unchained entry after the chain started",
                        index + 1
                    ),
                };
            }
            continue;
        }

        seen_chain = true;
        chained_entries += 1;

        if base.chain_alg.as_deref() != Some(CHAIN_ALG_SHA256) {
            return AuditIntegrityReport {
                status: AuditIntegrityStatus::Corrupt,
                checked_entries: entries.len(),
                chained_entries,
                message: format!(
                    "audit integrity failure at entry {}: unsupported or missing chain algorithm",
                    index + 1
                ),
            };
        }

        let Some(entry_hash) = base.entry_hash.as_deref() else {
            return AuditIntegrityReport {
                status: AuditIntegrityStatus::Corrupt,
                checked_entries: entries.len(),
                chained_entries,
                message: format!(
                    "audit integrity failure at entry {}: missing entry hash",
                    index + 1
                ),
            };
        };

        if base.prev_hash.as_deref() != previous_hash.as_deref() {
            return AuditIntegrityReport {
                status: AuditIntegrityStatus::Corrupt,
                checked_entries: entries.len(),
                chained_entries,
                message: format!(
                    "audit integrity failure at entry {}: chain link mismatch",
                    index + 1
                ),
            };
        }

        let expected_hash = match compute_entry_hash(entry, base.prev_hash.as_deref()) {
            Ok(hash) => hash,
            Err(err) => {
                return AuditIntegrityReport {
                    status: AuditIntegrityStatus::Corrupt,
                    checked_entries: entries.len(),
                    chained_entries,
                    message: format!("audit integrity failure at entry {}: {err}", index + 1),
                };
            }
        };

        if entry_hash != expected_hash {
            return AuditIntegrityReport {
                status: AuditIntegrityStatus::Corrupt,
                checked_entries: entries.len(),
                chained_entries,
                message: format!(
                    "audit integrity failure at entry {}: entry hash mismatch",
                    index + 1
                ),
            };
        }

        previous_hash = Some(entry_hash.to_string());
    }

    if chained_entries == 0 {
        AuditIntegrityReport {
            status: AuditIntegrityStatus::NoIntegrityData,
            checked_entries: entries.len(),
            chained_entries: 0,
            message: "no integrity data found in the audit log".to_string(),
        }
    } else {
        AuditIntegrityReport {
            status: AuditIntegrityStatus::Verified,
            checked_entries: entries.len(),
            chained_entries,
            message: format!(
                "audit integrity verified: {} chained entries checked",
                chained_entries
            ),
        }
    }
}
