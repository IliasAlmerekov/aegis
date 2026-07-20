use super::*;

#[test]
fn audit_entry_serializes_nested_explanation_sections() {
    let explanation = CommandExplanation {
        scan: ScanExplanation {
            highest_risk: RiskLevel::Danger,
            decision_source: aegis_scanner::DecisionSource::BuiltinPattern,
            basis: aegis_scanner::AssessmentBasis::Decisive {
                match_ids: vec!["FS-001".to_string()],
            },
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
            evidence: None,
            detection_id: None,
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
            evidence: None,
            detection_id: None,
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
            basis: aegis_scanner::AssessmentBasis::Decisive {
                match_ids: vec!["FS-001".to_string()],
            },
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
fn sandbox_bypass_serializes_status_and_legacy_boolean() {
    let entry = AuditEntry::new(
        "rm -rf target",
        RiskLevel::Danger,
        vec![],
        Decision::Approved,
        vec![],
        None,
        None,
    )
    .with_sandbox_status(SandboxStatus::Unavailable);

    let json = serde_json::to_value(&entry).unwrap();

    assert_eq!(json["sandbox_status"], "unavailable");
    // Legacy boolean is mirrored so older readers still see the bypass.
    assert_eq!(json["sandbox_active"], serde_json::json!(false));
}

#[test]
fn sandbox_active_serializes_status_and_legacy_boolean() {
    let entry = AuditEntry::new(
        "ls",
        RiskLevel::Safe,
        vec![],
        Decision::Approved,
        vec![],
        None,
        None,
    )
    .with_sandbox_status(SandboxStatus::Active);

    let json = serde_json::to_value(&entry).unwrap();

    assert_eq!(json["sandbox_status"], "active");
    assert_eq!(json["sandbox_active"], serde_json::json!(true));
}

#[test]
fn not_configured_omits_legacy_boolean() {
    let entry = AuditEntry::new(
        "ls",
        RiskLevel::Safe,
        vec![],
        Decision::Approved,
        vec![],
        None,
        None,
    );

    let json = serde_json::to_value(&entry).unwrap();

    assert_eq!(json["sandbox_status"], "not_configured");
    assert!(json.get("sandbox_active").is_none());
}

#[test]
fn not_attempted_serializes_without_legacy_boolean() {
    let entry = AuditEntry::new(
        "ls",
        RiskLevel::Safe,
        vec![],
        Decision::Denied,
        vec![],
        None,
        None,
    )
    .with_sandbox_status(SandboxStatus::NotAttempted);

    let json = serde_json::to_value(&entry).unwrap();

    assert_eq!(json["sandbox_status"], "not_attempted");
    assert!(json.get("sandbox_active").is_none());
}

#[test]
fn legacy_sandbox_active_false_deserializes_as_bypass() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    let legacy_entry = r#"{"timestamp":"2023-11-14T22:13:20Z","command":"rm -rf /","risk":"Danger","matched_patterns":[],"decision":"Approved","snapshots":[],"sandbox_active":false}"#;

    fs::write(logger.path(), format!("{legacy_entry}\n")).unwrap();

    let entries = logger.read_all().unwrap();
    assert_eq!(
        entries[0].as_base().sandbox_status,
        SandboxStatus::Unavailable
    );
}

#[test]
fn legacy_entry_without_sandbox_fields_is_not_configured() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    let legacy_entry = r#"{"timestamp":"2023-11-14T22:13:20Z","command":"ls","risk":"Safe","matched_patterns":[],"decision":"AutoApproved","snapshots":[]}"#;

    fs::write(logger.path(), format!("{legacy_entry}\n")).unwrap();

    let entries = logger.read_all().unwrap();
    assert_eq!(
        entries[0].as_base().sandbox_status,
        SandboxStatus::NotConfigured
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

// ── ADR-016: recovery backstop audit fields ───────────────────────────────

#[test]
fn legacy_entry_without_recovery_backstop_fields_still_deserializes() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    let legacy_entry = r#"{"timestamp":"2023-11-14T22:13:20Z","command":"sh ./x","risk":"Safe","matched_patterns":[],"decision":"AutoApproved","snapshots":[]}"#;

    fs::write(logger.path(), format!("{legacy_entry}\n")).unwrap();

    let entries = logger.read_all().unwrap();
    assert_eq!(entries.len(), 1);
    let base = entries[0].as_base();
    // Pre-ADR-016 logs never recorded these axes; they must read back as
    // `None` (not `Some(false)`) so "not recorded" stays distinguishable from
    // "explicitly false" — the same convention used for allowlist flags.
    assert_eq!(base.effect_opaque, None);
    assert_eq!(base.snapshots_required, None);
    assert_eq!(base.confinement_required, None);
    assert_eq!(base.recovery_degradation, None);
}

#[test]
fn new_entry_records_recovery_backstop_for_effect_opaque_command() {
    let entry = AuditEntry::new(
        "sh ./cleanup.sh",
        RiskLevel::Safe,
        vec![],
        Decision::AutoApproved,
        vec![],
        None,
        None,
    )
    .with_effect_opaque(true)
    .with_required_backstops(true, false);

    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(json["effect_opaque"], serde_json::json!(true));
    assert_eq!(json["snapshots_required"], serde_json::json!(true));
    assert_eq!(json["confinement_required"], serde_json::json!(false));
    assert!(json.get("recovery_degradation").is_none());

    // Round-trips through serde.
    let roundtripped: AuditEntry = serde_json::from_value(json).unwrap();
    let base = roundtripped.as_base();
    assert_eq!(base.effect_opaque, Some(true));
    assert_eq!(base.snapshots_required, Some(true));
    assert_eq!(base.confinement_required, Some(false));
    assert_eq!(base.recovery_degradation, None);
}

#[test]
fn new_entry_records_recovery_degradation_reason() {
    let entry = AuditEntry::new(
        "sh ./cleanup.sh",
        RiskLevel::Safe,
        vec![],
        Decision::Denied,
        vec![],
        None,
        None,
    )
    .with_effect_opaque(true)
    .with_required_backstops(true, false)
    .with_recovery_degradation(RecoveryDegradation::NoSnapshotAvailable);

    let json = serde_json::to_value(&entry).unwrap();
    assert_eq!(
        json["recovery_degradation"],
        serde_json::json!("no_snapshot_available")
    );

    let roundtripped: AuditEntry = serde_json::from_value(json).unwrap();
    assert_eq!(
        roundtripped.as_base().recovery_degradation,
        Some(RecoveryDegradation::NoSnapshotAvailable)
    );
}
