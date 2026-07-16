use super::super::types::{
    BlockReason, ExecutionTransport, PolicyAction, PolicyAllowlistResult, PolicyBlocklistResult,
    PolicyCiState, PolicyConfigFlags, PolicyDecision, PolicyExecutionContext, PolicyInput,
    PolicyRationale, PolicyRulesResult,
};
use super::evaluate_policy;
use aegis_parser::Parser as CommandParser;
use aegis_scanner::Assessment;
use aegis_types::RiskLevel;
use aegis_types::{AllowlistOverrideLevel, CiPolicy, Mode, SnapshotPolicy};

fn assessment(risk: RiskLevel) -> Assessment {
    Assessment {
        risk,
        effect_opaque: false,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: CommandParser::parse("terraform destroy -target=module.prod.api"),
    }
}

struct EvalInput<'a> {
    risk: RiskLevel,
    mode: Mode,
    ci_detected: bool,
    ci_policy: CiPolicy,
    allowlist_matched: bool,
    blocklist_matched: bool,
    allowlist_override_level: AllowlistOverrideLevel,
    snapshot_policy: SnapshotPolicy,
    applicable_snapshot_plugins: &'a [&'static str],
}

fn evaluate(input: EvalInput<'_>) -> PolicyDecision {
    use super::super::types::PolicyRulesResult;
    let assessment = assessment(input.risk);
    evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode: input.mode,
        ci_state: PolicyCiState {
            detected: input.ci_detected,
        },
        allowlist: PolicyAllowlistResult {
            matched: input.allowlist_matched,
        },
        blocklist: PolicyBlocklistResult {
            matched: input.blocklist_matched,
        },
        config_flags: PolicyConfigFlags {
            ci_policy: input.ci_policy,
            allowlist_override_level: input.allowlist_override_level,
            snapshot_policy: input.snapshot_policy,
        },
        execution_context: PolicyExecutionContext {
            transport: ExecutionTransport::Shell,
            applicable_snapshot_plugins: input.applicable_snapshot_plugins,
        },
        rules: PolicyRulesResult::default(),
    })
}

fn assert_decision(
    decision: PolicyDecision,
    expected_action: PolicyAction,
    expected_rationale: PolicyRationale,
    requires_confirmation: bool,
    snapshots_required: bool,
    allowlist_effective: bool,
    block_reason: Option<BlockReason>,
) {
    assert_eq!(decision.decision, expected_action);
    assert_eq!(decision.rationale, expected_rationale);
    assert_eq!(decision.requires_confirmation, requires_confirmation);
    assert_eq!(decision.snapshots_required, snapshots_required);
    assert_eq!(decision.allowlist_effective, allowlist_effective);
    assert_eq!(decision.block_reason(), block_reason);
}

#[test]
fn audit_mode_never_requires_confirmation_or_snapshots() {
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Danger,
        mode: Mode::Audit,
        ci_detected: true,
        ci_policy: CiPolicy::Block,
        blocklist_matched: false,
        allowlist_matched: true,
        allowlist_override_level: AllowlistOverrideLevel::Danger,
        snapshot_policy: SnapshotPolicy::Full,
        applicable_snapshot_plugins: &["git"],
    });

    assert_decision(
        decision,
        PolicyAction::AutoApprove,
        PolicyRationale::AuditMode,
        false,
        false,
        false,
        None,
    );
}

#[test]
fn audit_mode_opts_out_of_effect_opaque_recovery() {
    // ADR-016 / Spec #3: `Mode::Audit` is an intentional, observe-only opt-out
    // from *all* enforcement — prompts, blocks, and recovery backstops alike —
    // broader than `SnapshotPolicy::None`. An effect-opaque command that WOULD
    // require a pre-exec snapshot under `Protect`/`Strict` must NOT require one
    // under `Audit`, even with snapshots configured and a plugin available.
    // (Standards #1 still records `effect_opaque=true` on the audit entry; the
    // recovery decision itself is declined here.)
    let assessment = Assessment {
        risk: RiskLevel::Safe,
        effect_opaque: true,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: CommandParser::parse("sh ./cleanup.sh"),
    };
    let decision = evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode: Mode::Audit,
        ci_state: PolicyCiState { detected: false },
        allowlist: PolicyAllowlistResult { matched: false },
        blocklist: PolicyBlocklistResult { matched: false },
        config_flags: PolicyConfigFlags {
            ci_policy: CiPolicy::Block,
            allowlist_override_level: AllowlistOverrideLevel::Never,
            snapshot_policy: SnapshotPolicy::Selective,
        },
        execution_context: PolicyExecutionContext {
            transport: ExecutionTransport::Shell,
            applicable_snapshot_plugins: &["git"],
        },
        rules: PolicyRulesResult::default(),
    });

    assert_decision(
        decision,
        PolicyAction::AutoApprove,
        PolicyRationale::AuditMode,
        false,
        false,
        false,
        None,
    );
}

#[test]
fn protect_warn_without_override_requires_confirmation() {
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Warn,
        mode: Mode::Protect,
        ci_detected: false,
        ci_policy: CiPolicy::Block,
        blocklist_matched: false,
        allowlist_matched: false,
        allowlist_override_level: AllowlistOverrideLevel::Never,
        snapshot_policy: SnapshotPolicy::Selective,
        applicable_snapshot_plugins: &["git"],
    });

    assert_decision(
        decision,
        PolicyAction::Prompt,
        PolicyRationale::RequiresConfirmation,
        true,
        false,
        false,
        None,
    );
}

#[test]
fn protect_allowlisted_warn_autoapproves_without_snapshots() {
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Warn,
        mode: Mode::Protect,
        ci_detected: false,
        ci_policy: CiPolicy::Block,
        blocklist_matched: false,
        allowlist_matched: true,
        allowlist_override_level: AllowlistOverrideLevel::Warn,
        snapshot_policy: SnapshotPolicy::Selective,
        applicable_snapshot_plugins: &["git"],
    });

    assert_decision(
        decision,
        PolicyAction::AutoApprove,
        PolicyRationale::AllowlistOverride,
        false,
        false,
        true,
        None,
    );
}

#[test]
fn protect_danger_prompts_and_requests_snapshots_when_available() {
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Danger,
        mode: Mode::Protect,
        ci_detected: false,
        ci_policy: CiPolicy::Block,
        blocklist_matched: false,
        allowlist_matched: false,
        allowlist_override_level: AllowlistOverrideLevel::Never,
        snapshot_policy: SnapshotPolicy::Selective,
        applicable_snapshot_plugins: &["git"],
    });

    assert_decision(
        decision,
        PolicyAction::Prompt,
        PolicyRationale::RequiresConfirmation,
        true,
        true,
        false,
        None,
    );
}

// ── ADR-016 / H9: effect-opaque recovery backstop ──────────────────────────

/// Evaluate policy for an effect-opaque command (`sh ./cleanup.sh` shape)
/// under the given snapshot posture. Risk stays orthogonal to the recovery
/// requirement — the assessment is built with `effect_opaque = true`.
fn evaluate_effect_opaque(
    risk: RiskLevel,
    snapshot_policy: SnapshotPolicy,
    applicable_snapshot_plugins: &[&'static str],
) -> PolicyDecision {
    use super::super::types::PolicyRulesResult;
    let assessment = Assessment {
        risk,
        effect_opaque: true,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: CommandParser::parse("sh ./cleanup.sh"),
    };
    evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode: Mode::Protect,
        ci_state: PolicyCiState { detected: false },
        allowlist: PolicyAllowlistResult { matched: false },
        blocklist: PolicyBlocklistResult { matched: false },
        config_flags: PolicyConfigFlags {
            ci_policy: CiPolicy::Allow,
            allowlist_override_level: AllowlistOverrideLevel::Never,
            snapshot_policy,
        },
        execution_context: PolicyExecutionContext {
            transport: ExecutionTransport::Shell,
            applicable_snapshot_plugins,
        },
        rules: PolicyRulesResult::default(),
    })
}

#[test]
fn effect_opaque_safe_command_requires_recovery_under_selective_policy() {
    // ADR-016: `sh ./cleanup.sh` is Safe to the quick scan yet effect-opaque.
    // Recovery (`snapshots_required`) is the primary v1 backstop — orthogonal
    // to risk — so a Safe effect-opaque command still requests a pre-exec
    // snapshot when a snapshot policy and applicable plugins exist.
    let decision = evaluate_effect_opaque(RiskLevel::Safe, SnapshotPolicy::Selective, &["git"]);

    assert_eq!(decision.decision, PolicyAction::AutoApprove);
    assert_eq!(decision.rationale, PolicyRationale::SafeCommand);
    assert!(!decision.requires_confirmation);
    assert!(
        decision.snapshots_required,
        "effect-opaque Safe command must request a recovery snapshot"
    );
}

#[test]
fn effect_opaque_recovery_respects_snapshot_policy_none_opt_out() {
    // `SnapshotPolicy::None` is the trusted/global opt-out: effect opacity
    // does not override it. Project config cannot reach `None` (C3 ratchet),
    // but the engine must honour a global `None` without forcing snapshots.
    let decision = evaluate_effect_opaque(RiskLevel::Safe, SnapshotPolicy::None, &["git"]);

    assert!(
        !decision.snapshots_required,
        "SnapshotPolicy::None must suppress even effect-opaque recovery"
    );
}

#[test]
fn effect_opaque_recovery_remains_required_without_applicable_plugins() {
    let decision = evaluate_effect_opaque(RiskLevel::Safe, SnapshotPolicy::Full, &[]);
    assert!(decision.snapshots_required);
}
#[test]
fn strict_effect_opaque_recovery_remains_required_without_applicable_plugins() {
    let assessment = Assessment {
        risk: RiskLevel::Safe,
        effect_opaque: true,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: CommandParser::parse("sh ./cleanup.sh"),
    };
    let decision = evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode: Mode::Strict,
        ci_state: PolicyCiState { detected: false },
        allowlist: PolicyAllowlistResult { matched: false },
        blocklist: PolicyBlocklistResult { matched: false },
        config_flags: PolicyConfigFlags {
            ci_policy: CiPolicy::Block,
            allowlist_override_level: AllowlistOverrideLevel::Never,
            snapshot_policy: SnapshotPolicy::Full,
        },
        execution_context: PolicyExecutionContext {
            transport: ExecutionTransport::Shell,
            applicable_snapshot_plugins: &[],
        },
        rules: PolicyRulesResult::default(),
    });

    assert!(decision.snapshots_required);
}
#[test]
fn audit_policy_rule_allow_does_not_reactivate_effect_opaque_recovery() {
    let assessment = Assessment {
        risk: RiskLevel::Safe,
        effect_opaque: true,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: CommandParser::parse("sh ./cleanup.sh"),
    };
    let decision = evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode: Mode::Audit,
        ci_state: PolicyCiState { detected: false },
        allowlist: PolicyAllowlistResult { matched: false },
        blocklist: PolicyBlocklistResult { matched: false },
        config_flags: PolicyConfigFlags {
            ci_policy: CiPolicy::Allow,
            allowlist_override_level: AllowlistOverrideLevel::Never,
            snapshot_policy: SnapshotPolicy::Full,
        },
        execution_context: PolicyExecutionContext {
            transport: ExecutionTransport::Shell,
            applicable_snapshot_plugins: &["git"],
        },
        rules: PolicyRulesResult {
            matched: true,
            decision: Some(aegis_types::PolicyRuleDecision::Allow),
            justification: None,
        },
    });

    assert!(!decision.snapshots_required);
}
#[test]
fn policy_rule_allow_cannot_waive_effect_opaque_recovery() {
    let assessment = Assessment {
        risk: RiskLevel::Safe,
        effect_opaque: true,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: CommandParser::parse("sh ./cleanup.sh"),
    };
    let decision = evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode: Mode::Protect,
        ci_state: PolicyCiState { detected: false },
        allowlist: PolicyAllowlistResult { matched: false },
        blocklist: PolicyBlocklistResult { matched: false },
        config_flags: PolicyConfigFlags {
            ci_policy: CiPolicy::Allow,
            allowlist_override_level: AllowlistOverrideLevel::Never,
            snapshot_policy: SnapshotPolicy::Selective,
        },
        execution_context: PolicyExecutionContext {
            transport: ExecutionTransport::Shell,
            applicable_snapshot_plugins: &[],
        },
        rules: PolicyRulesResult {
            matched: true,
            decision: Some(aegis_types::PolicyRuleDecision::Allow),
            justification: None,
        },
    });

    assert!(decision.snapshots_required);
}
#[test]
fn protect_warn_allowlist_cannot_waive_effect_opaque_recovery() {
    let assessment = Assessment {
        risk: RiskLevel::Warn,
        effect_opaque: true,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: CommandParser::parse("sh ./cleanup.sh"),
    };
    let decision = evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode: Mode::Protect,
        ci_state: PolicyCiState { detected: false },
        allowlist: PolicyAllowlistResult { matched: true },
        blocklist: PolicyBlocklistResult { matched: false },
        config_flags: PolicyConfigFlags {
            ci_policy: CiPolicy::Allow,
            allowlist_override_level: AllowlistOverrideLevel::Warn,
            snapshot_policy: SnapshotPolicy::Selective,
        },
        execution_context: PolicyExecutionContext {
            transport: ExecutionTransport::Shell,
            applicable_snapshot_plugins: &[],
        },
        rules: PolicyRulesResult::default(),
    });

    assert!(decision.snapshots_required);
}

#[test]
fn policy_decision_carries_confinement_required_axis_defaulting_false() {
    // ADR-016: `confinement_required` is a plumbed axis beside
    // `snapshots_required`. It stays false by default in v1 — sandbox
    // confinement is an optional stricter tier, not the primary backstop
    // for effect opacity — but the field must exist so the audit trail and
    // a future strict tier can populate it.
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Danger,
        mode: Mode::Protect,
        ci_detected: false,
        ci_policy: CiPolicy::Block,
        blocklist_matched: false,
        allowlist_matched: false,
        allowlist_override_level: AllowlistOverrideLevel::Never,
        snapshot_policy: SnapshotPolicy::Selective,
        applicable_snapshot_plugins: &["git"],
    });

    assert!(!decision.confinement_required);
}

#[test]
fn protect_danger_does_not_request_snapshots_when_policy_disables_them() {
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Danger,
        mode: Mode::Protect,
        ci_detected: false,
        ci_policy: CiPolicy::Block,
        blocklist_matched: false,
        allowlist_matched: false,
        allowlist_override_level: AllowlistOverrideLevel::Never,
        snapshot_policy: SnapshotPolicy::None,
        applicable_snapshot_plugins: &["git"],
    });

    assert_decision(
        decision,
        PolicyAction::Prompt,
        PolicyRationale::RequiresConfirmation,
        true,
        false,
        false,
        None,
    );
}

#[test]
fn protect_danger_does_not_request_snapshots_without_applicable_plugins() {
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Danger,
        mode: Mode::Protect,
        ci_detected: false,
        ci_policy: CiPolicy::Block,
        blocklist_matched: false,
        allowlist_matched: false,
        allowlist_override_level: AllowlistOverrideLevel::Never,
        snapshot_policy: SnapshotPolicy::Selective,
        applicable_snapshot_plugins: &[],
    });

    assert_decision(
        decision,
        PolicyAction::Prompt,
        PolicyRationale::RequiresConfirmation,
        true,
        false,
        false,
        None,
    );
}

#[test]
fn protect_ci_policy_blocks_without_confirmation() {
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Warn,
        mode: Mode::Protect,
        ci_detected: true,
        ci_policy: CiPolicy::Block,
        blocklist_matched: false,
        allowlist_matched: false,
        allowlist_override_level: AllowlistOverrideLevel::Never,
        snapshot_policy: SnapshotPolicy::Selective,
        applicable_snapshot_plugins: &["git"],
    });

    assert_decision(
        decision,
        PolicyAction::Block,
        PolicyRationale::ProtectCiPolicy,
        false,
        false,
        false,
        Some(BlockReason::ProtectCiPolicy),
    );
}

#[test]
fn protect_ci_block_still_respects_danger_allowlist_override() {
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Danger,
        mode: Mode::Protect,
        ci_detected: true,
        ci_policy: CiPolicy::Block,
        blocklist_matched: false,
        allowlist_matched: true,
        allowlist_override_level: AllowlistOverrideLevel::Danger,
        snapshot_policy: SnapshotPolicy::Full,
        applicable_snapshot_plugins: &["git"],
    });

    assert_decision(
        decision,
        PolicyAction::AutoApprove,
        PolicyRationale::AllowlistOverride,
        false,
        true,
        true,
        None,
    );
}

#[test]
fn strict_mode_blocks_warn_without_override() {
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Warn,
        mode: Mode::Strict,
        ci_detected: false,
        ci_policy: CiPolicy::Allow,
        blocklist_matched: false,
        allowlist_matched: false,
        allowlist_override_level: AllowlistOverrideLevel::Never,
        snapshot_policy: SnapshotPolicy::Selective,
        applicable_snapshot_plugins: &["git"],
    });

    assert_decision(
        decision,
        PolicyAction::Block,
        PolicyRationale::StrictPolicy,
        false,
        false,
        false,
        Some(BlockReason::StrictPolicy),
    );
}

#[test]
fn strict_allowlist_override_danger_autoapproves_and_keeps_snapshot_requirement() {
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Danger,
        mode: Mode::Strict,
        ci_detected: false,
        ci_policy: CiPolicy::Block,
        blocklist_matched: false,
        allowlist_matched: true,
        allowlist_override_level: AllowlistOverrideLevel::Danger,
        snapshot_policy: SnapshotPolicy::Full,
        applicable_snapshot_plugins: &["git"],
    });

    assert_decision(
        decision,
        PolicyAction::AutoApprove,
        PolicyRationale::AllowlistOverride,
        false,
        true,
        true,
        None,
    );
}

#[test]
fn block_risk_is_never_bypassable() {
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Block,
        mode: Mode::Strict,
        ci_detected: false,
        ci_policy: CiPolicy::Allow,
        blocklist_matched: false,
        allowlist_matched: true,
        allowlist_override_level: AllowlistOverrideLevel::Danger,
        snapshot_policy: SnapshotPolicy::Full,
        applicable_snapshot_plugins: &["git"],
    });

    assert_decision(
        decision,
        PolicyAction::Block,
        PolicyRationale::IntrinsicRiskBlock,
        false,
        false,
        false,
        Some(BlockReason::IntrinsicRiskBlock),
    );
}

#[test]
fn blocklist_override_blocks_in_protect_mode() {
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Warn,
        mode: Mode::Protect,
        ci_detected: false,
        ci_policy: CiPolicy::Block,
        blocklist_matched: true,
        allowlist_matched: true,
        allowlist_override_level: AllowlistOverrideLevel::Warn,
        snapshot_policy: SnapshotPolicy::Selective,
        applicable_snapshot_plugins: &["git"],
    });

    assert_decision(
        decision,
        PolicyAction::Block,
        PolicyRationale::BlocklistOverride,
        false,
        false,
        false,
        Some(BlockReason::BlocklistOverride),
    );
}

#[test]
fn blocklist_override_blocks_in_strict_mode() {
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Danger,
        mode: Mode::Strict,
        ci_detected: false,
        ci_policy: CiPolicy::Allow,
        blocklist_matched: true,
        allowlist_matched: false,
        allowlist_override_level: AllowlistOverrideLevel::Never,
        snapshot_policy: SnapshotPolicy::Full,
        applicable_snapshot_plugins: &["git"],
    });

    assert_decision(
        decision,
        PolicyAction::Block,
        PolicyRationale::BlocklistOverride,
        false,
        false,
        false,
        Some(BlockReason::BlocklistOverride),
    );
}

#[test]
fn blocklist_override_blocks_safe_commands() {
    let decision = evaluate(EvalInput {
        risk: RiskLevel::Safe,
        mode: Mode::Audit,
        ci_detected: false,
        ci_policy: CiPolicy::Allow,
        blocklist_matched: true,
        allowlist_matched: false,
        allowlist_override_level: AllowlistOverrideLevel::Warn,
        snapshot_policy: SnapshotPolicy::Selective,
        applicable_snapshot_plugins: &["git"],
    });

    assert_decision(
        decision,
        PolicyAction::Block,
        PolicyRationale::BlocklistOverride,
        false,
        false,
        false,
        Some(BlockReason::BlocklistOverride),
    );
}

// ── Phase 5.2: [[rules]] policy engine tests ─────────────────────────────

fn evaluate_with_rules(
    risk: RiskLevel,
    mode: Mode,
    rules_matched: bool,
    rules_decision: Option<aegis_types::PolicyRuleDecision>,
) -> PolicyDecision {
    use super::super::types::PolicyRulesResult;
    let assessment = assessment(risk);
    evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode,
        ci_state: PolicyCiState { detected: false },
        allowlist: PolicyAllowlistResult { matched: false },
        blocklist: PolicyBlocklistResult { matched: false },
        config_flags: PolicyConfigFlags {
            ci_policy: CiPolicy::Allow,
            allowlist_override_level: AllowlistOverrideLevel::Never,
            snapshot_policy: SnapshotPolicy::None,
        },
        execution_context: PolicyExecutionContext {
            transport: ExecutionTransport::Shell,
            applicable_snapshot_plugins: &[],
        },
        rules: PolicyRulesResult {
            matched: rules_matched,
            decision: rules_decision,
            justification: None,
        },
    })
}

/// A matched rule with `Allow` decision must auto-approve even a Warn
/// command in Protect mode (bypasses normal mode evaluation).
#[test]
fn test_rules_allow_overrides_protect_mode_warn() {
    let decision = evaluate_with_rules(
        RiskLevel::Warn,
        Mode::Protect,
        true,
        Some(aegis_types::PolicyRuleDecision::Allow),
    );

    assert_eq!(decision.decision, PolicyAction::AutoApprove);
    assert_eq!(decision.rationale, PolicyRationale::PolicyRulesOverride);
}

/// A matched rule with `Block` decision must hard-block even a Safe command.
#[test]
fn test_rules_block_overrides_safe_command() {
    let decision = evaluate_with_rules(
        RiskLevel::Safe,
        Mode::Protect,
        true,
        Some(aegis_types::PolicyRuleDecision::Block),
    );

    assert_eq!(decision.decision, PolicyAction::Block);
    assert_eq!(decision.rationale, PolicyRationale::PolicyRulesOverride);
    assert_eq!(
        decision.block_reason(),
        Some(BlockReason::PolicyRulesOverride)
    );
}

/// A matched rule with `Prompt` decision must prompt even a Safe command.
#[test]
fn test_rules_prompt_overrides_safe_command() {
    let decision = evaluate_with_rules(
        RiskLevel::Safe,
        Mode::Protect,
        true,
        Some(aegis_types::PolicyRuleDecision::Prompt),
    );

    assert_eq!(decision.decision, PolicyAction::Prompt);
    assert_eq!(decision.rationale, PolicyRationale::PolicyRulesOverride);
}

/// When `matched = false`, normal policy must apply (Safe → AutoApprove in Protect).
#[test]
fn test_rules_not_matched_falls_through_to_normal_policy() {
    let decision = evaluate_with_rules(RiskLevel::Safe, Mode::Protect, false, None);

    assert_eq!(decision.decision, PolicyAction::AutoApprove);
    assert_eq!(decision.rationale, PolicyRationale::SafeCommand);
}

/// A `[[rules]]` entry with `decision = "allow"` must NOT bypass a
/// `RiskLevel::Block` command — intrinsic block takes precedence over rules.
#[test]
fn rules_allow_cannot_bypass_block_risk_level() {
    let decision = evaluate_with_rules(
        RiskLevel::Block,
        Mode::Protect,
        true,
        Some(aegis_types::PolicyRuleDecision::Allow),
    );

    assert_eq!(decision.decision, PolicyAction::Block);
    assert_eq!(decision.rationale, PolicyRationale::IntrinsicRiskBlock);
    assert_eq!(
        decision.block_reason(),
        Some(BlockReason::IntrinsicRiskBlock)
    );
}
