use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use flate2::read::GzDecoder;

use super::*;
use crate::error::AuditError;

impl AuditLogger {
    /// Read every audit entry from all segments, oldest first.
    #[must_use = "audit log entries must be used or explicitly discarded"]
    pub fn read_all(&self) -> Result<Vec<AuditEntry>> {
        let _lock = self.acquire_shared_lock()?;
        let mut entries = Vec::new();
        for segment in self.segments_oldest_to_newest()? {
            self.extend_entries_from_segment(&segment, None, &mut entries)?;
        }
        Ok(entries)
    }

    /// Filter audit entries according to the given query.
    #[must_use = "audit query result must be used or explicitly discarded"]
    pub fn query(&self, query: AuditQuery) -> Result<Vec<AuditEntry>> {
        let mut entries = self.read_all()?;
        entries.retain(|entry| entry_matches_query(entry, &query));

        if let Some(last) = query.last {
            if last == 0 {
                entries.clear();
            } else if entries.len() > last {
                let keep_from = entries.len() - last;
                entries = entries.split_off(keep_from);
            }
        }

        Ok(entries)
    }

    /// Render a human-readable table of audit entries.
    pub fn format_entries(entries: &[AuditEntry]) -> String {
        if entries.is_empty() {
            return "No audit entries matched.\n".to_string();
        }

        let mut out = String::new();
        out.push_str("timestamp                 decision       risk    command\n");

        for entry in entries {
            let base = entry.as_base();
            out.push_str(&format!(
                "{:<25} {:<14} {:<7} {}\n",
                base.timestamp, base.decision, base.risk, base.command
            ));

            if base.matched_patterns.is_empty() {
                out.push_str("  matched: none\n");
            } else {
                let matched = base
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

            if base.snapshots.is_empty() {
                out.push_str("  snapshots: none\n");
            } else {
                let snapshots = base
                    .snapshots
                    .iter()
                    .map(|snapshot| format!("{}={}", snapshot.plugin, snapshot.snapshot_id))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("  snapshots: {snapshots}\n"));
            }

            if let Some(pattern) = &base.allowlist_pattern {
                match &base.allowlist_reason {
                    Some(reason) => {
                        out.push_str(&format!("  allowlisted by: {pattern} ({reason})\n"));
                    }
                    None => {
                        out.push_str(&format!("  allowlisted by: {pattern}\n"));
                    }
                }
            }
        }

        out
    }

    /// Summarise a slice of audit entries into counts and top patterns.
    pub fn summarize_entries(entries: &[AuditEntry]) -> AuditSummary {
        let mut summary = AuditSummary {
            total_entries: entries.len(),
            decision_counts: DecisionCounts::default(),
            risk_counts: RiskCounts::default(),
            top_patterns: Vec::new(),
        };
        let mut pattern_counts = std::collections::BTreeMap::<String, usize>::new();

        for entry in entries {
            let base = entry.as_base();
            match base.decision {
                Decision::Approved => summary.decision_counts.approved += 1,
                Decision::Denied => summary.decision_counts.denied += 1,
                Decision::AutoApproved => summary.decision_counts.auto_approved += 1,
                Decision::Blocked => summary.decision_counts.blocked += 1,
                Decision::Pruned => summary.decision_counts.pruned += 1,
                // Unknown future decision variants are not tallied in the v1 summary buckets.
                _ => {}
            }

            match base.risk {
                RiskLevel::Safe => summary.risk_counts.safe += 1,
                RiskLevel::Warn => summary.risk_counts.warn += 1,
                RiskLevel::Danger => summary.risk_counts.danger += 1,
                RiskLevel::Block => summary.risk_counts.block += 1,
                // Unknown future risk levels are not tallied in the v1 summary buckets.
                _ => {}
            }

            for pattern in &base.matched_patterns {
                *pattern_counts.entry(pattern.id.clone()).or_default() += 1;
            }
        }

        let mut top_patterns = pattern_counts
            .into_iter()
            .map(|(id, count)| PatternCount { id, count })
            .collect::<Vec<_>>();
        top_patterns.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.id.cmp(&right.id))
        });
        summary.top_patterns = top_patterns;
        summary
    }

    /// Render a human-readable summary of audit statistics.
    pub fn format_summary(summary: &AuditSummary) -> String {
        if summary.total_entries == 0 {
            return "No audit entries matched.\n".to_string();
        }

        let mut out = String::new();
        out.push_str("Audit summary\n");
        out.push_str(&format!("  total entries: {}\n", summary.total_entries));
        out.push_str("  decisions:\n");
        out.push_str(&format!(
            "    approved={} denied={} auto-approved={} blocked={} pruned={}\n",
            summary.decision_counts.approved,
            summary.decision_counts.denied,
            summary.decision_counts.auto_approved,
            summary.decision_counts.blocked,
            summary.decision_counts.pruned
        ));
        out.push_str("  risks:\n");
        out.push_str(&format!(
            "    safe={} warn={} danger={} block={}\n",
            summary.risk_counts.safe,
            summary.risk_counts.warn,
            summary.risk_counts.danger,
            summary.risk_counts.block
        ));
        out.push_str("  Top matched patterns:\n");

        if summary.top_patterns.is_empty() {
            out.push_str("    none\n");
        } else {
            for pattern in &summary.top_patterns {
                out.push_str(&format!("    {} ({})\n", pattern.id, pattern.count));
            }
        }

        out
    }

    pub(super) fn read_last_entry_from_plain_file(
        &self,
        path: &Path,
    ) -> Result<Option<AuditEntry>> {
        if !path.exists() {
            return Ok(None);
        }

        let mut file = File::open(path)?;
        let file_len = file.metadata()?.len();
        if file_len == 0 {
            return Ok(None);
        }

        let mut pos = file_len;
        let mut tail = Vec::new();

        loop {
            let read_start = pos.saturating_sub(8192);
            let read_len = (pos - read_start) as usize;
            let mut chunk = vec![0; read_len];
            file.seek(SeekFrom::Start(read_start))?;
            file.read_exact(&mut chunk)?;
            chunk.extend_from_slice(&tail);
            tail = chunk;

            if let Some(line) = tail.split(|byte| *byte == b'\n').rev().find(|line| {
                !line.is_empty() && !line.iter().all(|byte| byte.is_ascii_whitespace())
            }) {
                return self.parse_entry_line(line, path, None);
            }

            if read_start == 0 {
                return Ok(None);
            }

            pos = read_start;
        }
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
            .map(AuditEntry::normalize_legacy_fields)
            .map(Some)
            .map_err(|err| match line_number {
                Some(index) => AuditError::Parse(format!(
                    "failed to parse audit log line {} in {}: {err}",
                    index,
                    source.display()
                )),
                None => AuditError::Parse(format!(
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

    pub(super) fn read_entries_from_segment(
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

            if risk.is_none_or(|expected| entry.as_base().risk == expected) {
                entries.push(entry);
            }
        }

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
}

fn entry_matches_query(entry: &AuditEntry, query: &AuditQuery) -> bool {
    let base = entry.as_base();

    if query.risk.is_some_and(|risk| base.risk != risk) {
        return false;
    }

    if query
        .decision
        .is_some_and(|decision| base.decision != decision)
    {
        return false;
    }

    if query.since.is_some_and(|since| base.timestamp < since) {
        return false;
    }

    if query.until.is_some_and(|until| base.timestamp > until) {
        return false;
    }

    if query
        .command_contains
        .as_ref()
        .is_some_and(|needle| !base.command.contains(needle))
    {
        return false;
    }

    true
}
