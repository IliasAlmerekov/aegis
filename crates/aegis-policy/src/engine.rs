//! Pure policy engine: evaluate a prepared policy input and return a decision.

use aegis_types::PolicyRuleDecision;
use aegis_types::{
    AllowlistOverrideLevel, AnalysisStatus, CiPolicy, DetectionMechanism, Mode, RiskLevel,
    SnapshotPolicy,
};

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
        if input.mode == Mode::Audit {
            return auto_approve(input, PolicyRationale::AuditMode, false, false);
        }
        if input.blocklist.matched {
            return block(input, PolicyRationale::BlocklistOverride);
        }

        if input.rules.matched && input.rules.decision == Some(PolicyRuleDecision::Block) {
            return block(input, PolicyRationale::PolicyRulesOverride);
        }

        if input.mode == Mode::Protect && analysis_requires_explicit_approval(&input) {
            if input.ci_state.detected && input.config_flags.ci_policy == CiPolicy::Block {
                return block(input, PolicyRationale::ProtectCiPolicy);
            }
            return analysis_confirmation_required(input);
        }

        if input.mode == Mode::Strict && strict_analysis_override_applies(&input) {
            return analysis_override_required(input);
        }

        if !analysis_requires_explicit_approval(&input)
            && input.rules.matched
            && let Some(decision) = input.rules.decision
        {
            return match decision {
                PolicyRuleDecision::Allow => {
                    // Preserve snapshot requirements for Danger commands even when a rule
                    // auto-approves — consistent with allowlist-override behaviour.
                    let snaps = snapshots_required(&input);
                    auto_approve(input, PolicyRationale::PolicyRulesOverride, false, snaps)
                }
                // Handled before the language-aware gate so an explicit rule
                // block remains terminal.
                PolicyRuleDecision::Block => block(input, PolicyRationale::PolicyRulesOverride),
                PolicyRuleDecision::Prompt => {
                    prompt_with_rationale(input, PolicyRationale::PolicyRulesOverride)
                }
            };
        }
        match input.mode {
            // `Mode::Audit` is an intentional, observe-only opt-out from *all*
            // enforcement — prompts, blocks, and recovery backstops alike — so
            // it declines ADR-016 recovery (`snapshots_required = false`) even
            // for effect-opaque commands with snapshots configured and a plugin
            // available. This is broader than `SnapshotPolicy::None` (the
            // trusted/global recovery opt-out): Audit mode auto-approves
            // everything and takes no snapshots, by design. The audit entry
            // still records the assessment's `effect_opaque` flag.
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
        RiskLevel::Safe => {
            let snaps = snapshots_required(&input);
            auto_approve(input, PolicyRationale::SafeCommand, false, snaps)
        }
        RiskLevel::Warn => {
            if allowlist_override_applies(&input) {
                let snaps = snapshots_required(&input);
                auto_approve(input, PolicyRationale::AllowlistOverride, true, snaps)
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
        RiskLevel::Safe => {
            let snaps = snapshots_required(&input);
            auto_approve(input, PolicyRationale::SafeCommand, false, snaps)
        }
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

    // ADR-022 §5: allowlist entries cannot auto-approve a completed semantic
    // Match or an Analysis degradation. The resulting Protect-mode prompt is a
    // one-time decision for this assessment, not a reusable allowlist grant.
    if analysis_requires_explicit_approval(input) {
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

fn analysis_requires_explicit_approval(input: &PolicyInput<'_>) -> bool {
    let Some(analysis) = input.assessment.analysis.as_ref() else {
        return false;
    };

    match analysis.status {
        AnalysisStatus::NotApplicable => false,
        AnalysisStatus::Degraded => true,
        AnalysisStatus::Complete => input
            .assessment
            .matched
            .iter()
            .any(|matched| matched.evidence.mechanism() == DetectionMechanism::LanguageRule),
        // Fail safe if a future analysis status is introduced.
        _ => true,
    }
}

fn strict_analysis_override_applies(input: &PolicyInput<'_>) -> bool {
    if has_language_match_at_assessment_risk(input) {
        return true;
    }

    input.assessment.risk == RiskLevel::Safe
        && input.assessment.analysis.as_ref().is_some_and(|analysis| {
            !matches!(
                analysis.status,
                AnalysisStatus::NotApplicable | AnalysisStatus::Complete
            )
        })
}

fn has_language_match_at_assessment_risk(input: &PolicyInput<'_>) -> bool {
    input.assessment.matched.iter().any(|matched| {
        matched.pattern.risk == input.assessment.risk
            && matched.evidence.mechanism() == DetectionMechanism::LanguageRule
    })
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
        confinement_required: false,
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
        confinement_required: false,
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
        confinement_required: false,
        allowlist_effective: false,
    }
}

fn analysis_override_required(input: PolicyInput<'_>) -> PolicyDecision {
    prompt_with_rationale(input, PolicyRationale::AnalysisOverrideRequired)
}

fn analysis_confirmation_required(input: PolicyInput<'_>) -> PolicyDecision {
    prompt_with_rationale(input, PolicyRationale::AnalysisConfirmationRequired)
}

fn block(_input: PolicyInput<'_>, rationale: PolicyRationale) -> PolicyDecision {
    PolicyDecision {
        decision: PolicyAction::Block,
        rationale,
        requires_confirmation: false,
        snapshots_required: false,
        confinement_required: false,
        allowlist_effective: false,
    }
}

fn snapshots_required(input: &PolicyInput<'_>) -> bool {
    if input.mode == Mode::Audit || input.config_flags.snapshot_policy == SnapshotPolicy::None {
        return false;
    }

    // ADR-016: plugin applicability cannot erase Required recovery for
    // effect-opaque execution. An empty plugin set is observed later as a
    // Recovery degradation.
    if input.assessment.effect_opaque {
        return true;
    }

    if input
        .execution_context
        .applicable_snapshot_plugins
        .is_empty()
    {
        return false;
    }

    // Ordinary non-effect-opaque Danger snapshots remain best-effort and are
    // requested only when a plugin is applicable.
    input.assessment.risk == RiskLevel::Danger
}

#[cfg(test)]
mod tests;
