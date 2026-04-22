use std::fs::{self, File};
use std::io::Read;

use flate2::read::GzDecoder;

use super::integrity::{AuditIntegrityPayload, compute_entry_hash, verify_integrity_entries};
use super::*;
use crate::decision::{ExecutionTransport, PolicyAction, PolicyRationale};
use crate::explanation::{
    CommandExplanation, ExecutionContextExplanation, ExecutionDecisionExplanation,
    ExecutionOutcomeExplanation, ExplainedPatternMatch, PolicyExplanation, ScanExplanation,
    SnapshotOutcomeExplanation,
};
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
        source: None,
        cwd: None,
        id: None,
        transport: None,
    }
}

fn explanation_with_match_text(matched_text: &str) -> CommandExplanation {
    CommandExplanation {
        scan: ScanExplanation {
            highest_risk: RiskLevel::Danger,
            decision_source: crate::interceptor::scanner::DecisionSource::BuiltinPattern,
            matched_patterns: vec![ExplainedPatternMatch {
                id: "FS-001".to_string(),
                risk: RiskLevel::Danger,
                description: "recursive delete".to_string(),
                matched_text: matched_text.to_string(),
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

    let entries = logger
        .query(AuditQuery {
            risk: Some(RiskLevel::Warn),
            ..AuditQuery::default()
        })
        .unwrap();
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

    let entries = logger
        .query(AuditQuery {
            last: Some(2),
            ..AuditQuery::default()
        })
        .unwrap();
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

    let entries = logger
        .query(AuditQuery {
            last: Some(2),
            risk: Some(RiskLevel::Warn),
            ..AuditQuery::default()
        })
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].command, "command-3");
    assert_eq!(entries[1].command, "command-5");
}

#[test]
fn query_filters_by_decision() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    for index in 0..6 {
        logger.append(entry(index, RiskLevel::Warn)).unwrap();
    }

    let entries = logger
        .query(AuditQuery {
            decision: Some(Decision::Blocked),
            ..AuditQuery::default()
        })
        .unwrap();

    assert!(!entries.is_empty());
    assert!(
        entries
            .iter()
            .all(|entry| entry.decision == Decision::Blocked)
    );
}

#[test]
fn query_filters_by_command_substring_case_sensitively() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    logger.append(entry(0, RiskLevel::Safe)).unwrap();
    logger.append(entry(1, RiskLevel::Warn)).unwrap();
    logger
        .append(AuditEntry {
            command: "git stash clear".to_string(),
            ..entry(2, RiskLevel::Warn)
        })
        .unwrap();

    let entries = logger
        .query(AuditQuery {
            command_contains: Some("stash".to_string()),
            ..AuditQuery::default()
        })
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].command, "git stash clear");

    let no_match = logger
        .query(AuditQuery {
            command_contains: Some("Stash".to_string()),
            ..AuditQuery::default()
        })
        .unwrap();
    assert!(
        no_match.is_empty(),
        "substring filter must be case-sensitive"
    );
}

#[test]
fn query_filters_by_inclusive_time_range() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    for index in 0..4 {
        logger.append(entry(index, RiskLevel::Warn)).unwrap();
    }

    let entries = logger
        .query(AuditQuery {
            since: Some(AuditTimestamp::from_unix_seconds(1_700_000_001).unwrap()),
            until: Some(AuditTimestamp::from_unix_seconds(1_700_000_002).unwrap()),
            ..AuditQuery::default()
        })
        .unwrap();

    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.command.as_str())
            .collect::<Vec<_>>(),
        vec!["command-1", "command-2"]
    );
}

#[test]
fn query_applies_last_after_other_filters() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    for index in 0..6 {
        logger.append(entry(index, RiskLevel::Warn)).unwrap();
    }

    let entries = logger
        .query(AuditQuery {
            decision: Some(Decision::Denied),
            last: Some(1),
            ..AuditQuery::default()
        })
        .unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].command, "command-5");
    assert_eq!(entries[0].decision, Decision::Denied);
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

    let entries = logger
        .query(AuditQuery {
            last: Some(2),
            ..AuditQuery::default()
        })
        .unwrap();
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
fn append_serializes_pattern_ids_and_allowlist_flags() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    logger.append(entry(0, RiskLevel::Warn)).unwrap();

    let written = fs::read_to_string(logger.path()).unwrap();
    let json: serde_json::Value = serde_json::from_str(written.trim()).unwrap();

    assert_eq!(json["pattern_ids"], serde_json::json!(["PAT-000"]));
    assert_eq!(json["allowlist_matched"], serde_json::json!(false));
    assert_eq!(json["allowlist_effective"], serde_json::json!(false));
}

#[test]
fn audit_entry_serializes_nested_explanation_sections() {
    let explanation = CommandExplanation {
        scan: ScanExplanation {
            highest_risk: RiskLevel::Danger,
            decision_source: crate::interceptor::scanner::DecisionSource::BuiltinPattern,
            matched_patterns: vec![ExplainedPatternMatch {
                id: "FS-001".to_string(),
                risk: RiskLevel::Danger,
                description: "recursive delete".to_string(),
                matched_text: "rm -rf".to_string(),
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
    };

    let entry = AuditEntry::new(
        "rm -rf target",
        RiskLevel::Danger,
        vec![MatchedPattern {
            id: "FS-001".to_string(),
            risk: RiskLevel::Danger,
            description: "recursive delete".to_string(),
            safe_alt: Some("rm -ri target".to_string()),
            category: Some(Category::Filesystem),
            matched_text: Some("rm -rf".to_string()),
            source: Some(PatternSource::Builtin),
        }],
        Decision::Approved,
        vec![AuditSnapshot {
            plugin: "git".to_string(),
            snapshot_id: "snap-1".to_string(),
        }],
        None,
        None,
    )
    .with_explanation(explanation);

    let json = serde_json::to_value(&entry).unwrap();

    assert_eq!(json["explanation"]["scan"]["highest_risk"], "Danger");
    assert_eq!(
        json["explanation"]["scan"]["matched_patterns"][0]["id"],
        "FS-001"
    );
    assert_eq!(json["explanation"]["policy"]["action"], "Prompt");
    assert_eq!(json["explanation"]["context"]["transport"], "Shell");
    assert_eq!(json["explanation"]["outcome"]["decision"], "Approved");
    assert_eq!(
        json["explanation"]["outcome"]["snapshots"][0]["snapshot_id"],
        "snap-1"
    );
}

#[test]
fn audit_entry_keeps_existing_top_level_fields_for_backward_compatibility() {
    let entry = AuditEntry::new(
        "rm -rf target",
        RiskLevel::Danger,
        vec![MatchedPattern {
            id: "FS-001".to_string(),
            risk: RiskLevel::Danger,
            description: "recursive delete".to_string(),
            safe_alt: Some("rm -ri target".to_string()),
            category: Some(Category::Filesystem),
            matched_text: Some("rm -rf".to_string()),
            source: Some(PatternSource::Builtin),
        }],
        Decision::Approved,
        vec![AuditSnapshot {
            plugin: "git".to_string(),
            snapshot_id: "snap-1".to_string(),
        }],
        None,
        None,
    )
    .with_explanation(CommandExplanation {
        scan: ScanExplanation {
            highest_risk: RiskLevel::Danger,
            decision_source: crate::interceptor::scanner::DecisionSource::BuiltinPattern,
            matched_patterns: vec![ExplainedPatternMatch {
                id: "FS-001".to_string(),
                risk: RiskLevel::Danger,
                description: "recursive delete".to_string(),
                matched_text: "rm -rf".to_string(),
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
    });

    let json = serde_json::to_value(&entry).unwrap();

    assert_eq!(json["command"], "rm -rf target");
    assert_eq!(json["risk"], "Danger");
    assert_eq!(json["matched_patterns"][0]["id"], "FS-001");
    assert_eq!(json["pattern_ids"], serde_json::json!(["FS-001"]));
    assert_eq!(json["decision"], "Approved");
    assert_eq!(json["snapshots"][0]["plugin"], "git");
    assert!(json.get("explanation").is_some());
}

#[test]
fn new_does_not_create_files_or_directories() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("nested/audit.jsonl"));

    assert!(!logger.path().exists());
    assert!(!logger.lock_path().exists());
}

#[test]
fn append_creates_parent_and_writes_entry_without_prebuilt_helpers() {
    let temp = tempfile::TempDir::new().unwrap();
    let log_path = temp.path().join("nested/audit.jsonl");
    let logger = AuditLogger::new(&log_path);

    let entry = AuditEntry::new(
        "echo hello".to_string(),
        RiskLevel::Safe,
        Vec::new(),
        Decision::Approved,
        Vec::new(),
        None,
        None,
    );

    logger.append(entry).unwrap();

    assert!(log_path.exists());
    let contents = std::fs::read_to_string(log_path).unwrap();
    assert!(contents.contains("\"command\":\"echo hello\""));
}

#[test]
fn append_with_chain_sha256_populates_hash_fields() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"))
        .with_integrity_mode(AuditIntegrityMode::ChainSha256);

    logger.append(entry(0, RiskLevel::Safe)).unwrap();
    logger.append(entry(1, RiskLevel::Warn)).unwrap();

    let entries = logger.read_all().unwrap();
    assert_eq!(entries[0].chain_alg.as_deref(), Some("sha256"));
    assert!(entries[0].entry_hash.is_some());
    assert!(entries[0].prev_hash.is_none());
    assert_eq!(entries[1].chain_alg.as_deref(), Some("sha256"));
    assert_eq!(entries[1].prev_hash, entries[0].entry_hash);
}

#[test]
fn append_normalizes_legacy_fields_only_once() {
    let source = include_str!("writer.rs");
    let append_start = source
        .find("pub fn append(&self, entry: AuditEntry) -> Result<()> {")
        .expect("append function must exist");
    let append_source = &source[append_start..];
    let next_fn = append_source
        .find("\n    pub(super) fn lock_path(&self) -> PathBuf {")
        .expect("append must be followed by read_all");
    let append_body = &append_source[..next_fn];

    let normalize_calls = append_body.matches("normalize_legacy_fields()").count();
    assert_eq!(
        normalize_calls, 1,
        "append must normalize legacy fields exactly once to avoid hidden repeat transforms"
    );
}

#[test]
fn append_documents_directory_creation_race_window() {
    let source = include_str!("writer.rs");
    let append_start = source
        .find("pub fn append(&self, entry: AuditEntry) -> Result<()> {")
        .expect("append function must exist");
    let append_source = &source[append_start..];
    let next_fn = append_source
        .find("\n    pub(super) fn lock_path(&self) -> PathBuf {")
        .expect("append must be followed by read_all");
    let append_body = &append_source[..next_fn];

    assert!(
        append_body.contains("narrow race window")
            && append_body.contains("create_dir_all")
            && append_body.contains("lock file lives inside that directory"),
        "append must document the acceptable create_dir_all-before-lock race window"
    );
}

#[test]
fn compute_entry_hash_changes_when_explanation_changes() {
    let base_entry =
        entry(0, RiskLevel::Danger).with_explanation(explanation_with_match_text("rm -rf"));
    let changed_entry =
        entry(0, RiskLevel::Danger).with_explanation(explanation_with_match_text("rm -fr"));

    let entry_hash = compute_entry_hash(&base_entry, None).unwrap();
    let changed_hash = compute_entry_hash(&changed_entry, None).unwrap();

    assert_ne!(entry_hash, changed_hash);
}

#[test]
fn integrity_payload_omits_explanation_key_when_absent() {
    let entry = entry(0, RiskLevel::Warn);
    let payload = AuditIntegrityPayload {
        timestamp: entry.timestamp,
        sequence: entry.sequence,
        command: &entry.command,
        risk: entry.risk,
        matched_patterns: &entry.matched_patterns,
        pattern_ids: &entry.pattern_ids,
        decision: entry.decision,
        snapshots: &entry.snapshots,
        explanation: None,
        mode: entry.mode,
        ci_detected: entry.ci_detected,
        allowlist_matched: entry.allowlist_matched,
        allowlist_effective: entry.allowlist_effective,
        chain_alg: CHAIN_ALG_SHA256,
        prev_hash: None,
        allowlist_pattern: entry.allowlist_pattern.as_ref(),
        allowlist_reason: entry.allowlist_reason.as_ref(),
        source: entry.source.as_ref(),
        cwd: entry.cwd.as_ref(),
        id: entry.id.as_ref(),
        transport: entry.transport.as_ref(),
    };

    let json = serde_json::to_value(&payload).unwrap();

    assert!(json.get("explanation").is_none());
}

#[test]
fn verify_integrity_reports_no_data_for_legacy_entries() {
    let report = verify_integrity_entries(&[entry(0, RiskLevel::Safe), entry(1, RiskLevel::Warn)]);

    assert_eq!(report.status, AuditIntegrityStatus::NoIntegrityData);
}

#[test]
fn verify_integrity_detects_reordered_entries() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"))
        .with_integrity_mode(AuditIntegrityMode::ChainSha256);

    logger.append(entry(0, RiskLevel::Safe)).unwrap();
    logger.append(entry(1, RiskLevel::Warn)).unwrap();
    logger.append(entry(2, RiskLevel::Danger)).unwrap();

    let mut entries = logger.read_all().unwrap();
    entries.swap(1, 2);

    let report = verify_integrity_entries(&entries);
    assert_eq!(report.status, AuditIntegrityStatus::Corrupt);
    assert!(report.message.contains("chain link mismatch"));
}

#[test]
fn append_creates_companion_lock_file() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    logger.append(entry(0, RiskLevel::Safe)).unwrap();

    assert!(
        dir.path().join("audit.jsonl.lock").exists(),
        "append path must create a companion lock file"
    );
}

#[test]
fn read_all_creates_companion_lock_file_when_log_exists() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    logger.append(entry(0, RiskLevel::Safe)).unwrap();
    fs::remove_file(dir.path().join("audit.jsonl.lock")).unwrap();

    let entries = logger.read_all().unwrap();

    assert_eq!(entries.len(), 1);
    assert!(
        dir.path().join("audit.jsonl.lock").exists(),
        "read path must use the companion lock file too"
    );
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
fn read_all_backfills_pattern_ids_for_legacy_entries() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    let legacy_entry = r#"{"timestamp":"2023-11-14T22:13:20Z","command":"legacy","risk":"Warn","matched_patterns":[{"id":"FS-001","risk":"Warn","description":"recursive delete","safe_alt":"rm -ri"}],"decision":"Denied","snapshots":[]}"#;

    fs::write(logger.path(), format!("{legacy_entry}\n")).unwrap();

    let entries = logger.read_all().unwrap();
    let json = serde_json::to_value(&entries[0]).unwrap();
    assert_eq!(json["pattern_ids"], serde_json::json!(["FS-001"]));
    assert_eq!(json["allowlist_matched"], serde_json::json!(false));
    assert_eq!(json["allowlist_effective"], serde_json::json!(false));
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

    let last = logger
        .query(AuditQuery {
            last: Some(2),
            ..AuditQuery::default()
        })
        .unwrap();
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
        None,
    );

    let json = serde_json::to_string(&entry).unwrap();
    assert!(!json.contains("source"), "source must be absent when None");
    assert!(
        !json.contains("transport"),
        "transport must be absent when None"
    );
}
