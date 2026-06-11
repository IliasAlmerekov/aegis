use std::fs::{self, File};
use std::io::Read;

use flate2::read::GzDecoder;

use super::integrity::{AuditIntegrityPayload, compute_entry_hash, verify_integrity_entries};
use super::*;
use aegis_explanation::{
    CommandExplanation, ExecutionContextExplanation, ExecutionDecisionExplanation,
    ExecutionOutcomeExplanation, ExplainedPatternMatch, PolicyExplanation, ScanExplanation,
    SnapshotOutcomeExplanation,
};
use aegis_policy::{ExecutionTransport, PolicyAction, PolicyRationale};
use tempfile::TempDir;

pub fn entry(index: usize, risk: RiskLevel) -> AuditEntry {
    AuditEntry::Decision(DecisionEntry {
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
        pattern_ids: vec![format!("PAT-{index:03}")],
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
        explanation: None,
        mode: None,
        ci_detected: None,
        allowlist_matched: Some(false),
        allowlist_effective: Some(false),
        chain_alg: None,
        prev_hash: None,
        entry_hash: None,
        allowlist_pattern: None,
        allowlist_reason: None,
    })
}

pub fn explanation_with_match_text(matched_text: &str) -> CommandExplanation {
    CommandExplanation {
        scan: ScanExplanation {
            highest_risk: RiskLevel::Danger,
            decision_source: aegis_scanner::DecisionSource::BuiltinPattern,
            matched_patterns: vec![ExplainedPatternMatch {
                id: "FS-001".to_string(),
                risk: RiskLevel::Danger,
                description: "recursive delete".to_string(),
                matched_text: matched_text.to_string(),
                justification: None,
            }],
        },
        policy: PolicyExplanation {
            action: PolicyAction::Prompt,
            rationale: PolicyRationale::RequiresConfirmation,
            requires_confirmation: true,
            snapshots_required: true,
            allowlist_effective: false,
            block_reason: None,
        },
        context: ExecutionContextExplanation {
            mode: Mode::Protect,
            transport: ExecutionTransport::Shell,
            ci_detected: false,
            allowlist_match: None,
            applicable_snapshot_plugins: vec!["git".to_string()],
        },
        outcome: Some(ExecutionOutcomeExplanation {
            decision: ExecutionDecisionExplanation::Approved,
            snapshots: vec![SnapshotOutcomeExplanation {
                plugin: "git".to_string(),
                snapshot_id: "snap-1".to_string(),
            }],
        }),
    }
}

pub fn entry_bytes(index: usize, risk: RiskLevel) -> usize {
    let mut bytes = serde_json::to_vec(&entry(index, risk)).unwrap();
    bytes.push(b'\n');
    bytes.len()
}

pub fn rotation_policy(
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

mod append_tests;
mod integrity_tests;
mod query_tests;
mod rotation_tests;
mod serialization_tests;
mod watch_tests;
