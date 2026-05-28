//! Policy types: inputs, actions, rationales, and execution transport.

use crate::config::{AllowlistOverrideLevel, CiPolicy, Mode, SnapshotPolicy};
use crate::interceptor::scanner::Assessment;
use serde::{Deserialize, Serialize};

/// Identifies the caller path that is asking policy for a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// Blocklist match state visible to policy evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyBlocklistResult {
    /// Whether the command matched a blocklist rule in the current context.
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
    /// Blocklist outcome for the current command and scope.
    pub blocklist: PolicyBlocklistResult,
    /// Effective policy-related config flags.
    pub config_flags: PolicyConfigFlags,
    /// Execution-specific context such as transport and snapshot applicability.
    pub execution_context: PolicyExecutionContext<'a>,
}

/// The action Aegis should take after evaluating policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyAction {
    /// Execute without user confirmation.
    AutoApprove,
    /// Show an interactive confirmation dialog.
    Prompt,
    /// Refuse execution entirely.
    Block,
}

/// The reason a command was hard-blocked by policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockReason {
    /// The command matched a `RiskLevel::Block` pattern — never bypassable.
    IntrinsicRiskBlock,
    /// Strict mode blocked a Warn or Danger command without an explicit override.
    StrictPolicy,
    /// Protect mode is running in CI and `ci_policy = Block` forced a block.
    ProtectCiPolicy,
    /// The command matched an explicit user-defined blocklist rule.
    BlocklistOverride,
}

/// Human-readable policy rationale classified for runtime/UI handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    /// An explicit user-defined blocklist rule matched.
    BlocklistOverride,
}

impl PolicyRationale {
    /// Return the block reason represented by this rationale, if any.
    #[must_use]
    pub fn block_reason(self) -> Option<BlockReason> {
        match self {
            Self::IntrinsicRiskBlock => Some(BlockReason::IntrinsicRiskBlock),
            Self::ProtectCiPolicy => Some(BlockReason::ProtectCiPolicy),
            Self::StrictPolicy => Some(BlockReason::StrictPolicy),
            Self::BlocklistOverride => Some(BlockReason::BlocklistOverride),
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
