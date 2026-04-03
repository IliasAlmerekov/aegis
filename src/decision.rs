use crate::config::{CiPolicy, Mode};
use crate::interceptor::RiskLevel;

/// Inputs required to evaluate the mode-specific policy decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionInput {
    pub mode: Mode,
    pub risk: RiskLevel,
    pub in_ci: bool,
    pub ci_policy: CiPolicy,
    pub allowlist_match: bool,
    pub strict_allowlist_override: bool,
}

/// The action Aegis should take after evaluating policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyAction {
    AutoApprove,
    Prompt,
    Block,
}

/// The reason a command was hard-blocked by policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    /// The command matched a `RiskLevel::Block` pattern — never bypassable.
    IntrinsicRiskBlock,
    /// Strict mode blocked a Warn or Danger command without an explicit override.
    StrictPolicy,
    /// Protect mode is running in CI and `ci_policy = Block` forced a block.
    ProtectCiPolicy,
}

/// The full policy outcome consumed by the runtime layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionPlan {
    pub action: PolicyAction,
    pub prompt_required: bool,
    pub should_snapshot: bool,
    pub allowlist_effective: bool,
    pub block_reason: Option<BlockReason>,
}

/// Evaluate the mode policy without performing side effects.
#[must_use]
pub fn evaluate_policy(input: DecisionInput) -> DecisionPlan {
    match input.mode {
        Mode::Audit => DecisionPlan {
            action: PolicyAction::AutoApprove,
            prompt_required: false,
            should_snapshot: false,
            allowlist_effective: false,
            block_reason: None,
        },
        Mode::Protect => evaluate_protect(input),
        Mode::Strict => evaluate_strict(input),
    }
}

fn evaluate_protect(input: DecisionInput) -> DecisionPlan {
    match input.risk {
        RiskLevel::Safe => auto_approve(false, false),
        RiskLevel::Warn => {
            if input.allowlist_match {
                auto_approve(false, true)
            } else if input.in_ci && input.ci_policy == CiPolicy::Block {
                block(BlockReason::ProtectCiPolicy)
            } else {
                prompt(false)
            }
        }
        RiskLevel::Danger => {
            if input.allowlist_match {
                auto_approve(true, true)
            } else if input.in_ci && input.ci_policy == CiPolicy::Block {
                block(BlockReason::ProtectCiPolicy)
            } else {
                prompt(true)
            }
        }
        RiskLevel::Block => block(BlockReason::IntrinsicRiskBlock),
    }
}

fn evaluate_strict(input: DecisionInput) -> DecisionPlan {
    match input.risk {
        RiskLevel::Safe => auto_approve(false, false),
        RiskLevel::Warn => {
            if input.strict_allowlist_override && input.allowlist_match {
                auto_approve(false, true)
            } else {
                block(BlockReason::StrictPolicy)
            }
        }
        RiskLevel::Danger => {
            if input.strict_allowlist_override && input.allowlist_match {
                auto_approve(true, true)
            } else {
                block(BlockReason::StrictPolicy)
            }
        }
        RiskLevel::Block => block(BlockReason::IntrinsicRiskBlock),
    }
}

fn auto_approve(should_snapshot: bool, allowlist_effective: bool) -> DecisionPlan {
    DecisionPlan {
        action: PolicyAction::AutoApprove,
        prompt_required: false,
        should_snapshot,
        allowlist_effective,
        block_reason: None,
    }
}

fn prompt(should_snapshot: bool) -> DecisionPlan {
    DecisionPlan {
        action: PolicyAction::Prompt,
        prompt_required: true,
        should_snapshot,
        allowlist_effective: false,
        block_reason: None,
    }
}

fn block(reason: BlockReason) -> DecisionPlan {
    DecisionPlan {
        action: PolicyAction::Block,
        prompt_required: false,
        should_snapshot: false,
        allowlist_effective: false,
        block_reason: Some(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::{BlockReason, DecisionInput, DecisionPlan, PolicyAction, evaluate_policy};
    use crate::config::{CiPolicy, Mode};
    use crate::interceptor::RiskLevel;

    fn assert_plan(
        plan: DecisionPlan,
        action: PolicyAction,
        prompt_required: bool,
        should_snapshot: bool,
        allowlist_effective: bool,
        block_reason: Option<BlockReason>,
    ) {
        assert_eq!(plan.action, action);
        assert_eq!(plan.prompt_required, prompt_required);
        assert_eq!(plan.should_snapshot, should_snapshot);
        assert_eq!(plan.allowlist_effective, allowlist_effective);
        assert_eq!(plan.block_reason, block_reason);
    }

    #[test]
    fn audit_mode_never_blocks_or_prompts() {
        let warn = evaluate_policy(DecisionInput {
            mode: Mode::Audit,
            risk: RiskLevel::Warn,
            in_ci: false,
            ci_policy: CiPolicy::Block,
            allowlist_match: true,
            strict_allowlist_override: true,
        });
        let danger = evaluate_policy(DecisionInput {
            mode: Mode::Audit,
            risk: RiskLevel::Danger,
            in_ci: true,
            ci_policy: CiPolicy::Block,
            allowlist_match: false,
            strict_allowlist_override: false,
        });

        assert_plan(warn, PolicyAction::AutoApprove, false, false, false, None);
        assert_plan(danger, PolicyAction::AutoApprove, false, false, false, None);
    }

    #[test]
    fn protect_allowlist_keeps_danger_snapshot() {
        let plan = evaluate_policy(DecisionInput {
            mode: Mode::Protect,
            risk: RiskLevel::Danger,
            in_ci: false,
            ci_policy: CiPolicy::Block,
            allowlist_match: true,
            strict_allowlist_override: false,
        });

        assert_plan(plan, PolicyAction::AutoApprove, false, true, true, None);
    }

    #[test]
    fn strict_mode_blocks_warn_without_override() {
        let plan = evaluate_policy(DecisionInput {
            mode: Mode::Strict,
            risk: RiskLevel::Warn,
            in_ci: false,
            ci_policy: CiPolicy::Allow,
            allowlist_match: true,
            strict_allowlist_override: false,
        });

        assert_plan(
            plan,
            PolicyAction::Block,
            false,
            false,
            false,
            Some(BlockReason::StrictPolicy),
        );
    }

    #[test]
    fn strict_override_auto_approves_danger_but_not_block() {
        let danger = evaluate_policy(DecisionInput {
            mode: Mode::Strict,
            risk: RiskLevel::Danger,
            in_ci: false,
            ci_policy: CiPolicy::Block,
            allowlist_match: true,
            strict_allowlist_override: true,
        });
        let block = evaluate_policy(DecisionInput {
            mode: Mode::Strict,
            risk: RiskLevel::Block,
            in_ci: false,
            ci_policy: CiPolicy::Block,
            allowlist_match: true,
            strict_allowlist_override: true,
        });

        assert_plan(danger, PolicyAction::AutoApprove, false, true, true, None);
        assert_plan(
            block,
            PolicyAction::Block,
            false,
            false,
            false,
            Some(BlockReason::IntrinsicRiskBlock),
        );
    }

    #[test]
    fn block_is_never_bypassable() {
        let protect = evaluate_policy(DecisionInput {
            mode: Mode::Protect,
            risk: RiskLevel::Block,
            in_ci: false,
            ci_policy: CiPolicy::Allow,
            allowlist_match: true,
            strict_allowlist_override: false,
        });
        let strict = evaluate_policy(DecisionInput {
            mode: Mode::Strict,
            risk: RiskLevel::Block,
            in_ci: false,
            ci_policy: CiPolicy::Allow,
            allowlist_match: true,
            strict_allowlist_override: true,
        });
        let audit = evaluate_policy(DecisionInput {
            mode: Mode::Audit,
            risk: RiskLevel::Block,
            in_ci: true,
            ci_policy: CiPolicy::Block,
            allowlist_match: true,
            strict_allowlist_override: true,
        });

        assert_plan(
            protect,
            PolicyAction::Block,
            false,
            false,
            false,
            Some(BlockReason::IntrinsicRiskBlock),
        );
        assert_plan(
            strict,
            PolicyAction::Block,
            false,
            false,
            false,
            Some(BlockReason::IntrinsicRiskBlock),
        );
        assert_plan(audit, PolicyAction::AutoApprove, false, false, false, None);
    }

    #[test]
    fn mode_changes_output_for_same_danger_command() {
        let protect = evaluate_policy(DecisionInput {
            mode: Mode::Protect,
            risk: RiskLevel::Danger,
            in_ci: false,
            ci_policy: CiPolicy::Block,
            allowlist_match: false,
            strict_allowlist_override: false,
        });
        let audit = evaluate_policy(DecisionInput {
            mode: Mode::Audit,
            risk: RiskLevel::Danger,
            in_ci: false,
            ci_policy: CiPolicy::Block,
            allowlist_match: false,
            strict_allowlist_override: false,
        });
        let strict = evaluate_policy(DecisionInput {
            mode: Mode::Strict,
            risk: RiskLevel::Danger,
            in_ci: false,
            ci_policy: CiPolicy::Block,
            allowlist_match: false,
            strict_allowlist_override: false,
        });

        assert_plan(protect, PolicyAction::Prompt, true, true, false, None);
        assert_plan(audit, PolicyAction::AutoApprove, false, false, false, None);
        assert_plan(
            strict,
            PolicyAction::Block,
            false,
            false,
            false,
            Some(BlockReason::StrictPolicy),
        );
    }

    #[test]
    fn protect_ci_block_still_respects_allowlist_for_danger() {
        let plan = evaluate_policy(DecisionInput {
            mode: Mode::Protect,
            risk: RiskLevel::Danger,
            in_ci: true,
            ci_policy: CiPolicy::Block,
            allowlist_match: true,
            strict_allowlist_override: false,
        });

        assert_plan(plan, PolicyAction::AutoApprove, false, true, true, None);
    }

    #[test]
    fn protect_ci_block_sets_ci_block_reason() {
        let plan = evaluate_policy(DecisionInput {
            mode: Mode::Protect,
            risk: RiskLevel::Warn,
            in_ci: true,
            ci_policy: CiPolicy::Block,
            allowlist_match: false,
            strict_allowlist_override: false,
        });

        assert_plan(
            plan,
            PolicyAction::Block,
            false,
            false,
            false,
            Some(BlockReason::ProtectCiPolicy),
        );
    }
}
