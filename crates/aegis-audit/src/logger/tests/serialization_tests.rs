use super::*;

#[test]
fn audit_entry_serializes_nested_explanation_sections() {
    let explanation = CommandExplanation {
        scan: ScanExplanation {
            highest_risk: RiskLevel::Danger,
            decision_source: aegis_scanner::DecisionSource::BuiltinPattern,
            matched_patterns: vec![ExplainedPatternMatch {
                id: "FS-001".to_string(),
                risk: RiskLevel::Danger,
                description: "recursive delete".to_string(),
                matched_text: "rm -rf".to_string(),
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
            decision_source: aegis_scanner::DecisionSource::BuiltinPattern,
            matched_patterns: vec![ExplainedPatternMatch {
                id: "FS-001".to_string(),
                risk: RiskLevel::Danger,
                description: "recursive delete".to_string(),
                matched_text: "rm -rf".to_string(),
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
fn read_all_accepts_legacy_unix_seconds_timestamp() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    let legacy_entry = r#"{"timestamp":1700000000,"command":"legacy","risk":"Safe","matched_patterns":[],"decision":"AutoApproved","snapshots":[]}"#;

    fs::write(logger.path(), format!("{legacy_entry}\n")).unwrap();

    let entries = logger.read_all().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].as_base().timestamp.to_string(),
        "2023-11-14T22:13:20Z"
    );
    assert_eq!(entries[0].as_base().sequence, 0);
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
