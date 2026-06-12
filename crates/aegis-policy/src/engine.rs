//! Pure policy engine: evaluate a prepared policy input and return a decision.

use aegis_types::PolicyRuleDecision;
use aegis_types::RiskLevel;
use aegis_types::{AllowlistOverrideLevel, CiPolicy, Mode, SnapshotPolicy};

use super::types::{PolicyAction, PolicyDecision, PolicyInput, PolicyRationale};

/// Side-effect-free policy evaluator.
pub trait PolicyEngine {
    /// Evaluate the policy input and return a fully explained decision.
    fn evaluate(&self, input: PolicyInput<'_>) -> PolicyDecision;
}

/// Default policy evaluator used by the CLI runtime.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultPolicyEngine;

impl PolicyEngine for DefaultPolicyEngine {
    fn evaluate(&self, input: PolicyInput<'_>) -> PolicyDecision {
        // Intrinsic-block commands are never bypassable — not by rules, not by allowlist.
        if input.assessment.risk == RiskLevel::Block {
            return block(input, PolicyRationale::IntrinsicRiskBlock);
        }
        if input.blocklist.matched {
            return block(input, PolicyRationale::BlocklistOverride);
        }
        if input.rules.matched
            && let Some(decision) = input.rules.decision
        {
            return match decision {
                PolicyRuleDecision::Allow => {
                    // Preserve snapshot requirements for Danger commands even when a rule
                    // auto-approves — consistent with allowlist-override behaviour.
                    let snaps = snapshots_required(&input);
                    auto_approve(input, PolicyRationale::PolicyRulesOverride, false, snaps)
                }
                PolicyRuleDecision::Block => block(input, PolicyRationale::PolicyRulesOverride),
                PolicyRuleDecision::Prompt => {
                    prompt_with_rationale(input, PolicyRationale::PolicyRulesOverride)
                }
            };
        }
        match input.mode {
            Mode::Audit => auto_approve(input, PolicyRationale::AuditMode, false, false),
            Mode::Protect => evaluate_protect(input),
            Mode::Strict => evaluate_strict(input),
        }
    }
}

/// Evaluate policy with the default engine.
#[must_use]
pub fn evaluate_policy(input: PolicyInput<'_>) -> PolicyDecision {
    DefaultPolicyEngine.evaluate(input)
}

fn evaluate_protect(input: PolicyInput<'_>) -> PolicyDecision {
    match input.assessment.risk {
        RiskLevel::Safe => auto_approve(input, PolicyRationale::SafeCommand, false, false),
        RiskLevel::Warn => {
            if allowlist_override_applies(&input) {
                auto_approve(input, PolicyRationale::AllowlistOverride, true, false)
            } else if input.ci_state.detected && input.config_flags.ci_policy == CiPolicy::Block {
                block(input, PolicyRationale::ProtectCiPolicy)
            } else {
                prompt(input)
            }
        }
        RiskLevel::Danger => {
            let snaps = snapshots_required(&input);
            if allowlist_override_applies(&input) {
                auto_approve(input, PolicyRationale::AllowlistOverride, true, snaps)
            } else if input.ci_state.detected && input.config_flags.ci_policy == CiPolicy::Block {
                block(input, PolicyRationale::ProtectCiPolicy)
            } else {
                prompt(input)
            }
        }
        RiskLevel::Block => block(input, PolicyRationale::IntrinsicRiskBlock),
        // Fail safe: an unknown future risk level is treated as Block.
        _ => block(input, PolicyRationale::IntrinsicRiskBlock),
    }
}

fn evaluate_strict(input: PolicyInput<'_>) -> PolicyDecision {
    match input.assessment.risk {
        RiskLevel::Safe => auto_approve(input, PolicyRationale::SafeCommand, false, false),
        RiskLevel::Warn | RiskLevel::Danger => {
            let snaps = snapshots_required(&input);
            if allowlist_override_applies(&input) {
                auto_approve(input, PolicyRationale::AllowlistOverride, true, snaps)
            } else {
                block(input, PolicyRationale::StrictPolicy)
            }
        }
        RiskLevel::Block => block(input, PolicyRationale::IntrinsicRiskBlock),
        // Fail safe: an unknown future risk level is treated as Block.
        _ => block(input, PolicyRationale::IntrinsicRiskBlock),
    }
}

fn allowlist_override_applies(input: &PolicyInput<'_>) -> bool {
    if !input.allowlist.matched {
        return false;
    }

    match input.assessment.risk {
        RiskLevel::Warn => matches!(
            input.config_flags.allowlist_override_level,
            AllowlistOverrideLevel::Warn | AllowlistOverrideLevel::Danger
        ),
        RiskLevel::Danger => matches!(
            input.config_flags.allowlist_override_level,
            AllowlistOverrideLevel::Danger
        ),
        RiskLevel::Safe | RiskLevel::Block => false,
        // Fail safe: an unknown future risk level never qualifies for an override.
        _ => false,
    }
}

fn auto_approve(
    _input: PolicyInput<'_>,
    rationale: PolicyRationale,
    allowlist_effective: bool,
    snapshots_required: bool,
) -> PolicyDecision {
    PolicyDecision {
        decision: PolicyAction::AutoApprove,
        rationale,
        requires_confirmation: false,
        snapshots_required,
        allowlist_effective,
    }
}

fn prompt(input: PolicyInput<'_>) -> PolicyDecision {
    let snaps = snapshots_required(&input);
    PolicyDecision {
        decision: PolicyAction::Prompt,
        rationale: PolicyRationale::RequiresConfirmation,
        requires_confirmation: true,
        snapshots_required: snaps,
        allowlist_effective: false,
    }
}

fn prompt_with_rationale(input: PolicyInput<'_>, rationale: PolicyRationale) -> PolicyDecision {
    let snaps = snapshots_required(&input);
    PolicyDecision {
        decision: PolicyAction::Prompt,
        rationale,
        requires_confirmation: true,
        snapshots_required: snaps,
        allowlist_effective: false,
    }
}

fn block(_input: PolicyInput<'_>, rationale: PolicyRationale) -> PolicyDecision {
    PolicyDecision {
        decision: PolicyAction::Block,
        rationale,
        requires_confirmation: false,
        snapshots_required: false,
        allowlist_effective: false,
    }
}

fn snapshots_required(input: &PolicyInput<'_>) -> bool {
    if input.assessment.risk != RiskLevel::Danger {
        return false;
    }

    if input.config_flags.snapshot_policy == SnapshotPolicy::None {
        return false;
    }

    !input
        .execution_context
        .applicable_snapshot_plugins
        .is_empty()
}

#[cfg(test)]
mod tests {
    use super::super::types::{
        BlockReason, ExecutionTransport, PolicyAction, PolicyAllowlistResult,
        PolicyBlocklistResult, PolicyCiState, PolicyConfigFlags, PolicyDecision,
        PolicyExecutionContext, PolicyInput, PolicyRationale,
    };
    use super::evaluate_policy;
    use aegis_parser::Parser as CommandParser;
    use aegis_scanner::Assessment;
    use aegis_types::RiskLevel;
    use aegis_types::{AllowlistOverrideLevel, CiPolicy, Mode, SnapshotPolicy};

    fn assessment(risk: RiskLevel) -> Assessment {
        Assessment {
            risk,
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
}
