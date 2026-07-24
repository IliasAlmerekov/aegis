use std::borrow::Cow;
use std::sync::Arc;

use super::super::types::{
    BlockReason, ExecutionTransport, PolicyAction, PolicyAllowlistResult, PolicyBlocklistResult,
    PolicyCiState, PolicyConfigFlags, PolicyDecision, PolicyExecutionContext, PolicyInput,
    PolicyRationale, PolicyRulesResult,
};
use super::evaluate_policy;
use aegis_parser::Parser as CommandParser;
use aegis_scanner::Assessment;
use aegis_types::{
    AllowlistOverrideLevel, AnalysisProvenance, Category, CiPolicy, DetectedOperation,
    DetectionSource, MatchEvidence, MatchResult, Mode, OperandCertainty, OperationKind,
    OperationModifiers, Pattern, PatternSource, RiskLevel, SnapshotPolicy, SourceOrigin,
};

fn assessment(risk: RiskLevel) -> Assessment {
    Assessment {
        risk,
        effect_opaque: false,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: CommandParser::parse("terraform destroy -target=module.prod.api"),
        analysis: None,
    }
}

/// A completed language-aware assessment at the given risk.
///
/// This models a semantic `Warn` such as a source-level destructive API that
/// was not auto-approved by the shell scanner. Iteration 9 policy must treat
/// the presence of this summary as distinct from an ordinary scanner warning.
fn language_aware_assessment(risk: RiskLevel) -> Assessment {
    let match_risk = match risk {
        RiskLevel::Safe | RiskLevel::Warn => RiskLevel::Warn,
        RiskLevel::Danger => RiskLevel::Danger,
        RiskLevel::Block => RiskLevel::Warn,
        _ => RiskLevel::Warn,
    };
    let id = "LANG-FS-DEL";
    let language_match = MatchResult {
        pattern: Arc::new(Pattern {
            id: Cow::Borrowed(id),
            category: Category::Filesystem,
            risk: match_risk,
            pattern: Cow::Borrowed(""),
            description: Cow::Borrowed("language-aware filesystem deletion"),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
        }),
        matched_text: String::new(),
        highlight_range: None,
        evidence: MatchEvidence::LanguageRule {
            source: DetectionSource::Builtin,
            operation: DetectedOperation {
                kind: OperationKind::FilesystemDelete,
                modifiers: OperationModifiers::default(),
                certainty: OperandCertainty::Known,
            },
            provenance: AnalysisProvenance {
                language: Some("python".to_string()),
                source_origin: SourceOrigin::Inline,
                rule_id: Some(id.to_string()),
                operation: None,
                file_path: None,
                source_hash: None,
                span: None,
                certainty: OperandCertainty::Known,
                status: aegis_types::AnalysisStatus::Complete,
                degradation_reason: None,
            },
        },
    };
    Assessment {
        matched: vec![language_match],
        analysis: Some(aegis_types::AnalysisSummary {
            status: aegis_types::AnalysisStatus::Complete,
            degradation_reasons: Vec::new(),
        }),
        ..assessment(risk)
    }
}

/// A degraded language-aware assessment at the given risk.
fn degraded_language_aware_assessment(risk: RiskLevel) -> Assessment {
    degraded_language_aware_assessment_with_reason(
        risk,
        aegis_types::DegradationReason::DynamicSource,
    )
}

fn degraded_language_aware_assessment_with_reason(
    risk: RiskLevel,
    reason: aegis_types::DegradationReason,
) -> Assessment {
    Assessment {
        analysis: Some(aegis_types::AnalysisSummary {
            status: aegis_types::AnalysisStatus::Degraded,
            degradation_reasons: vec![reason],
        }),
        ..assessment(risk)
    }
}

/// An effect-opaque `sh ./cleanup.sh`-shaped assessment at the given `risk`
/// (ADR-016). Shared by the effect-opaque recovery tests below, which only
/// ever vary `risk`.
fn effect_opaque_assessment(risk: RiskLevel) -> Assessment {
    Assessment {
        risk,
        effect_opaque: true,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: CommandParser::parse("sh ./cleanup.sh"),
        analysis: None,
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
    let assessment = effect_opaque_assessment(RiskLevel::Safe);
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
fn audit_mode_autoapproves_degraded_language_aware_safe_assessment() {
    let assessment = degraded_language_aware_assessment(RiskLevel::Safe);
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
            applicable_snapshot_plugins: &[],
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
fn audit_mode_precedes_policy_rule_allow_for_degraded_language_aware_input() {
    let assessment = degraded_language_aware_assessment(RiskLevel::Safe);
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
            applicable_snapshot_plugins: &[],
        },
        rules: PolicyRulesResult {
            matched: true,
            decision: Some(aegis_types::PolicyRuleDecision::Allow),
            justification: None,
        },
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
fn audit_mode_precedes_blocklist_for_degraded_language_aware_input() {
    let assessment = degraded_language_aware_assessment(RiskLevel::Safe);
    let decision = evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode: Mode::Audit,
        ci_state: PolicyCiState { detected: false },
        allowlist: PolicyAllowlistResult { matched: false },
        blocklist: PolicyBlocklistResult { matched: true },
        config_flags: PolicyConfigFlags {
            ci_policy: CiPolicy::Block,
            allowlist_override_level: AllowlistOverrideLevel::Never,
            snapshot_policy: SnapshotPolicy::Selective,
        },
        execution_context: PolicyExecutionContext {
            transport: ExecutionTransport::Shell,
            applicable_snapshot_plugins: &[],
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
fn protect_degraded_language_aware_safe_requires_confirmation() {
    let assessment = degraded_language_aware_assessment(RiskLevel::Safe);
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
        rules: PolicyRulesResult::default(),
    });

    assert_decision(
        decision,
        PolicyAction::Prompt,
        PolicyRationale::AnalysisConfirmationRequired,
        true,
        false,
        false,
        None,
    );
}

#[test]
fn protect_allowlisted_language_aware_warn_requires_confirmation() {
    let assessment = language_aware_assessment(RiskLevel::Warn);
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

    assert_decision(
        decision,
        PolicyAction::Prompt,
        PolicyRationale::AnalysisConfirmationRequired,
        true,
        false,
        false,
        None,
    );
}

#[test]
fn protect_policy_rule_allow_for_language_aware_warn_requires_confirmation() {
    let assessment = language_aware_assessment(RiskLevel::Warn);
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

    assert_decision(
        decision,
        PolicyAction::Prompt,
        PolicyRationale::AnalysisConfirmationRequired,
        true,
        false,
        false,
        None,
    );
}

#[test]
fn protect_every_analysis_degradation_class_requires_one_time_confirmation() {
    use aegis_types::DegradationReason::{
        DynamicSource, GrammarUnavailable, IncompleteSyntax, LimitExceeded, UnsafeSource,
        UnsupportedEncoding, WorkerFailure,
    };

    for reason in [
        GrammarUnavailable,
        IncompleteSyntax,
        UnsafeSource,
        UnsupportedEncoding,
        LimitExceeded,
        DynamicSource,
        WorkerFailure,
    ] {
        let assessment = degraded_language_aware_assessment_with_reason(RiskLevel::Safe, reason);
        let decision = evaluate_policy(PolicyInput {
            assessment: &assessment,
            mode: Mode::Protect,
            ci_state: PolicyCiState { detected: false },
            allowlist: PolicyAllowlistResult { matched: true },
            blocklist: PolicyBlocklistResult { matched: false },
            config_flags: PolicyConfigFlags {
                ci_policy: CiPolicy::Allow,
                allowlist_override_level: AllowlistOverrideLevel::Danger,
                snapshot_policy: SnapshotPolicy::Selective,
            },
            execution_context: PolicyExecutionContext {
                transport: ExecutionTransport::Shell,
                applicable_snapshot_plugins: &[],
            },
            rules: PolicyRulesResult::default(),
        });

        assert_eq!(decision.decision, PolicyAction::Prompt, "{reason:?}");
        assert_eq!(
            decision.rationale,
            PolicyRationale::AnalysisConfirmationRequired,
            "{reason:?}"
        );
        assert!(!decision.allowlist_effective, "{reason:?}");
    }
}

#[test]
fn protect_ci_block_applies_to_completed_language_aware_warn() {
    let assessment = language_aware_assessment(RiskLevel::Warn);
    let decision = evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode: Mode::Protect,
        ci_state: PolicyCiState { detected: true },
        allowlist: PolicyAllowlistResult { matched: false },
        blocklist: PolicyBlocklistResult { matched: false },
        config_flags: PolicyConfigFlags {
            ci_policy: CiPolicy::Block,
            allowlist_override_level: AllowlistOverrideLevel::Never,
            snapshot_policy: SnapshotPolicy::Selective,
        },
        execution_context: PolicyExecutionContext {
            transport: ExecutionTransport::Evaluation,
            applicable_snapshot_plugins: &[],
        },
        rules: PolicyRulesResult::default(),
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
fn protect_policy_rule_prompt_for_language_aware_warn_uses_one_time_confirmation() {
    let assessment = language_aware_assessment(RiskLevel::Warn);
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
            decision: Some(aegis_types::PolicyRuleDecision::Prompt),
            justification: None,
        },
    });

    assert_decision(
        decision,
        PolicyAction::Prompt,
        PolicyRationale::AnalysisConfirmationRequired,
        true,
        false,
        false,
        None,
    );
}

#[test]
fn protect_language_aware_danger_requires_confirmation_and_snapshots() {
    let assessment = language_aware_assessment(RiskLevel::Danger);
    let decision = evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode: Mode::Protect,
        ci_state: PolicyCiState { detected: false },
        allowlist: PolicyAllowlistResult { matched: true },
        blocklist: PolicyBlocklistResult { matched: false },
        config_flags: PolicyConfigFlags {
            ci_policy: CiPolicy::Allow,
            allowlist_override_level: AllowlistOverrideLevel::Danger,
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
        PolicyAction::Prompt,
        PolicyRationale::AnalysisConfirmationRequired,
        true,
        true,
        false,
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

#[test]
fn strict_scanner_warn_with_only_analysis_degradation_remains_blocked() {
    let assessment = degraded_language_aware_assessment(RiskLevel::Warn);
    let decision = evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode: Mode::Strict,
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
        rules: PolicyRulesResult::default(),
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
fn strict_policy_rule_allow_cannot_autoapprove_scanner_warn_with_analysis_degradation() {
    let assessment = degraded_language_aware_assessment(RiskLevel::Warn);
    let decision = evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode: Mode::Strict,
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

mod recovery_and_rules;
