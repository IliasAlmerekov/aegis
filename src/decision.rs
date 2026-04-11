use crate::config::{AllowlistOverrideLevel, CiPolicy, Mode, SnapshotPolicy};
use crate::interceptor::RiskLevel;
use crate::interceptor::scanner::Assessment;

/// Identifies the caller path that is asking policy for a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionTransport {
    /// Normal shell-wrapper execution from `aegis -c ...`.
    Shell,
    /// NDJSON watch-mode execution from `aegis watch`.
    Watch,
    /// Evaluation-only output such as `aegis --output json`.
    Evaluation,
}

/// CI detection state visible to policy evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyCiState {
    /// Whether the current invocation is running under CI detection.
    pub detected: bool,
}

/// Allowlist match state visible to policy evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyAllowlistResult {
    /// Whether the command matched an allowlist rule in the current context.
    pub matched: bool,
}

/// Policy-relevant config flags already resolved by config loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyConfigFlags {
    /// Effective CI policy.
    pub ci_policy: CiPolicy,
    /// Effective allowlist ceiling for non-safe commands.
    pub allowlist_override_level: AllowlistOverrideLevel,
    /// Effective snapshot mode.
    pub snapshot_policy: SnapshotPolicy,
}

/// Runtime context that can influence policy without introducing side effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyExecutionContext<'a> {
    /// Which product surface is asking for a decision.
    pub transport: ExecutionTransport,
    /// Snapshot plugins applicable to the current execution context.
    pub applicable_snapshot_plugins: &'a [&'static str],
}

/// Full input required to evaluate policy.
#[derive(Clone, Copy)]
pub struct PolicyInput<'a> {
    /// Scanner assessment for the command under evaluation.
    pub assessment: &'a Assessment,
    /// Effective operating mode.
    pub mode: Mode,
    /// Current CI detection state.
    pub ci_state: PolicyCiState,
    /// Allowlist outcome for the current command and scope.
    pub allowlist: PolicyAllowlistResult,
    /// Effective policy-related config flags.
    pub config_flags: PolicyConfigFlags,
    /// Execution-specific context such as transport and snapshot applicability.
    pub execution_context: PolicyExecutionContext<'a>,
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

/// Human-readable policy rationale classified for runtime/UI handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyRationale {
    /// Audit mode bypasses normal approval flow.
    AuditMode,
    /// Safe commands are auto-approved.
    SafeCommand,
    /// An allowlist override made the command auto-approvable.
    AllowlistOverride,
    /// The command requires an explicit confirmation step.
    RequiresConfirmation,
    /// The command is intrinsically block-level.
    IntrinsicRiskBlock,
    /// CI policy forced a Protect-mode block.
    ProtectCiPolicy,
    /// Strict mode forced a block.
    StrictPolicy,
}

impl PolicyRationale {
    /// Return the block reason represented by this rationale, if any.
    #[must_use]
    pub fn block_reason(self) -> Option<BlockReason> {
        match self {
            Self::IntrinsicRiskBlock => Some(BlockReason::IntrinsicRiskBlock),
            Self::ProtectCiPolicy => Some(BlockReason::ProtectCiPolicy),
            Self::StrictPolicy => Some(BlockReason::StrictPolicy),
            Self::AuditMode
            | Self::SafeCommand
            | Self::AllowlistOverride
            | Self::RequiresConfirmation => None,
        }
    }
}

/// Full policy outcome consumed by UI and execution layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyDecision {
    /// The final side-effect-free policy decision.
    pub decision: PolicyAction,
    /// Why policy reached this decision.
    pub rationale: PolicyRationale,
    /// Whether the caller must ask the human for confirmation.
    pub requires_confirmation: bool,
    /// Whether snapshots should be attempted before execution.
    pub snapshots_required: bool,
    /// Whether the allowlist materially changed the outcome.
    pub allowlist_effective: bool,
}

impl PolicyDecision {
    /// Return the block reason, when the decision is a hard block.
    #[must_use]
    pub fn block_reason(self) -> Option<BlockReason> {
        self.rationale.block_reason()
    }
}

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
            if allowlist_override_applies(input) {
                auto_approve(input, PolicyRationale::AllowlistOverride, true, false)
            } else if input.ci_state.detected && input.config_flags.ci_policy == CiPolicy::Block {
                block(input, PolicyRationale::ProtectCiPolicy)
            } else {
                prompt(input)
            }
        }
        RiskLevel::Danger => {
            if allowlist_override_applies(input) {
                auto_approve(
                    input,
                    PolicyRationale::AllowlistOverride,
                    true,
                    snapshots_required(input),
                )
            } else if input.ci_state.detected && input.config_flags.ci_policy == CiPolicy::Block {
                block(input, PolicyRationale::ProtectCiPolicy)
            } else {
                prompt(input)
            }
        }
        RiskLevel::Block => block(input, PolicyRationale::IntrinsicRiskBlock),
    }
}

fn evaluate_strict(input: PolicyInput<'_>) -> PolicyDecision {
    match input.assessment.risk {
        RiskLevel::Safe => auto_approve(input, PolicyRationale::SafeCommand, false, false),
        RiskLevel::Warn | RiskLevel::Danger => {
            if allowlist_override_applies(input) {
                auto_approve(
                    input,
                    PolicyRationale::AllowlistOverride,
                    true,
                    snapshots_required(input),
                )
            } else {
                block(input, PolicyRationale::StrictPolicy)
            }
        }
        RiskLevel::Block => block(input, PolicyRationale::IntrinsicRiskBlock),
    }
}

fn allowlist_override_applies(input: PolicyInput<'_>) -> bool {
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
    PolicyDecision {
        decision: PolicyAction::Prompt,
        rationale: PolicyRationale::RequiresConfirmation,
        requires_confirmation: true,
        snapshots_required: snapshots_required(input),
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

fn snapshots_required(input: PolicyInput<'_>) -> bool {
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
    use super::{
        BlockReason, ExecutionTransport, PolicyAction, PolicyAllowlistResult, PolicyCiState,
        PolicyConfigFlags, PolicyDecision, PolicyExecutionContext, PolicyInput, PolicyRationale,
        evaluate_policy,
    };
    use crate::config::{AllowlistOverrideLevel, CiPolicy, Mode, SnapshotPolicy};
    use crate::interceptor::RiskLevel;
    use crate::interceptor::parser::Parser as CommandParser;
    use crate::interceptor::scanner::Assessment;

    fn assessment(risk: RiskLevel) -> Assessment {
        Assessment {
            risk,
            matched: Vec::new(),
            highlight_ranges: Vec::new(),
            command: CommandParser::parse("terraform destroy -target=module.prod.api"),
        }
    }

    fn evaluate(
        risk: RiskLevel,
        mode: Mode,
        ci_detected: bool,
        ci_policy: CiPolicy,
        allowlist_matched: bool,
        allowlist_override_level: AllowlistOverrideLevel,
        snapshot_policy: SnapshotPolicy,
        applicable_snapshot_plugins: &[&'static str],
    ) -> PolicyDecision {
        let assessment = assessment(risk);
        evaluate_policy(PolicyInput {
            assessment: &assessment,
            mode,
            ci_state: PolicyCiState {
                detected: ci_detected,
            },
            allowlist: PolicyAllowlistResult {
                matched: allowlist_matched,
            },
            config_flags: PolicyConfigFlags {
                ci_policy,
                allowlist_override_level,
                snapshot_policy,
            },
            execution_context: PolicyExecutionContext {
                transport: ExecutionTransport::Shell,
                applicable_snapshot_plugins,
            },
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
        let decision = evaluate(
            RiskLevel::Danger,
            Mode::Audit,
            true,
            CiPolicy::Block,
            true,
            AllowlistOverrideLevel::Danger,
            SnapshotPolicy::Full,
            &["git"],
        );

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
        let decision = evaluate(
            RiskLevel::Warn,
            Mode::Protect,
            false,
            CiPolicy::Block,
            false,
            AllowlistOverrideLevel::Never,
            SnapshotPolicy::Selective,
            &["git"],
        );

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
        let decision = evaluate(
            RiskLevel::Warn,
            Mode::Protect,
            false,
            CiPolicy::Block,
            true,
            AllowlistOverrideLevel::Warn,
            SnapshotPolicy::Selective,
            &["git"],
        );

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
        let decision = evaluate(
            RiskLevel::Danger,
            Mode::Protect,
            false,
            CiPolicy::Block,
            false,
            AllowlistOverrideLevel::Never,
            SnapshotPolicy::Selective,
            &["git"],
        );

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
        let decision = evaluate(
            RiskLevel::Danger,
            Mode::Protect,
            false,
            CiPolicy::Block,
            false,
            AllowlistOverrideLevel::Never,
            SnapshotPolicy::None,
            &["git"],
        );

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
        let decision = evaluate(
            RiskLevel::Danger,
            Mode::Protect,
            false,
            CiPolicy::Block,
            false,
            AllowlistOverrideLevel::Never,
            SnapshotPolicy::Selective,
            &[],
        );

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
        let decision = evaluate(
            RiskLevel::Warn,
            Mode::Protect,
            true,
            CiPolicy::Block,
            false,
            AllowlistOverrideLevel::Never,
            SnapshotPolicy::Selective,
            &["git"],
        );

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
        let decision = evaluate(
            RiskLevel::Danger,
            Mode::Protect,
            true,
            CiPolicy::Block,
            true,
            AllowlistOverrideLevel::Danger,
            SnapshotPolicy::Full,
            &["git"],
        );

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
        let decision = evaluate(
            RiskLevel::Warn,
            Mode::Strict,
            false,
            CiPolicy::Allow,
            false,
            AllowlistOverrideLevel::Never,
            SnapshotPolicy::Selective,
            &["git"],
        );

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
        let decision = evaluate(
            RiskLevel::Danger,
            Mode::Strict,
            false,
            CiPolicy::Block,
            true,
            AllowlistOverrideLevel::Danger,
            SnapshotPolicy::Full,
            &["git"],
        );

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
        let decision = evaluate(
            RiskLevel::Block,
            Mode::Strict,
            false,
            CiPolicy::Allow,
            true,
            AllowlistOverrideLevel::Danger,
            SnapshotPolicy::Full,
            &["git"],
        );

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
}
