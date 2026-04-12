use std::path::PathBuf;

use crate::audit::MatchedPattern;
use crate::config::{AllowlistMatch, Mode};
use crate::decision::{BlockReason, ExecutionTransport, PolicyAction, PolicyDecision};
use crate::interceptor::RiskLevel;
use crate::interceptor::scanner::Assessment;

/// Canonical planning result shared by interception surfaces.
pub enum PlanningOutcome {
    /// A normal command plan produced from scanner + policy inputs.
    Planned(InterceptionPlan),
    /// A fail-closed setup outcome produced before normal planning could finish.
    SetupFailure(SetupFailurePlan),
}

/// Canonical typed plan for one intercepted command.
pub struct InterceptionPlan {
    assessment: Assessment,
    decision_context: DecisionContext,
    policy_decision: PolicyDecision,
    approval_requirement: ApprovalRequirement,
    snapshot_plan: SnapshotPlan,
    execution_disposition: ExecutionDisposition,
    audit_facts: AuditFacts,
}

impl InterceptionPlan {
    /// Build a canonical interception plan from a pure policy result.
    pub(crate) fn from_policy(
        assessment: Assessment,
        decision_context: DecisionContext,
        policy_decision: PolicyDecision,
    ) -> Self {
        let approval_requirement = match policy_decision.decision {
            PolicyAction::Prompt => ApprovalRequirement::HumanConfirmationRequired,
            PolicyAction::AutoApprove | PolicyAction::Block => ApprovalRequirement::None,
        };
        let snapshot_plan = if policy_decision.snapshots_required {
            SnapshotPlan::Required {
                applicable_plugins: decision_context.applicable_snapshot_plugins.clone(),
            }
        } else {
            SnapshotPlan::NotRequired
        };
        let execution_disposition = match policy_decision.decision {
            PolicyAction::AutoApprove => ExecutionDisposition::Execute,
            PolicyAction::Prompt => ExecutionDisposition::RequiresApproval,
            PolicyAction::Block => ExecutionDisposition::Block,
        };
        let audit_facts = AuditFacts::from_plan_inputs(
            &assessment,
            &decision_context,
            policy_decision.block_reason(),
            policy_decision.allowlist_effective,
        );

        Self {
            assessment,
            decision_context,
            policy_decision,
            approval_requirement,
            snapshot_plan,
            execution_disposition,
            audit_facts,
        }
    }

    /// Return the scanner assessment used to build the plan.
    pub fn assessment(&self) -> &Assessment {
        &self.assessment
    }

    /// Return the resolved decision context used to evaluate policy.
    pub fn decision_context(&self) -> &DecisionContext {
        &self.decision_context
    }

    /// Return the pure policy decision embedded in the plan.
    pub fn policy_decision(&self) -> PolicyDecision {
        self.policy_decision
    }

    /// Return whether human confirmation is required before execution.
    pub fn approval_requirement(&self) -> ApprovalRequirement {
        self.approval_requirement
    }

    /// Return the pre-execution snapshot requirements for this plan.
    pub fn snapshot_plan(&self) -> SnapshotPlan {
        self.snapshot_plan.clone()
    }

    /// Return what the caller must do next with this command.
    pub fn execution_disposition(&self) -> ExecutionDisposition {
        self.execution_disposition
    }

    /// Return audit-only facts derived during planning.
    pub fn audit_facts(&self) -> &AuditFacts {
        &self.audit_facts
    }
}

/// Typed fail-closed planning result for setup failures.
#[derive(Debug, Clone)]
pub struct SetupFailurePlan {
    kind: SetupFailureKind,
    fail_closed_action: FailClosedAction,
    user_message: String,
    audit_facts: Option<AuditFacts>,
}

impl SetupFailurePlan {
    /// Create a fail-closed setup failure plan.
    pub(crate) fn new(
        kind: SetupFailureKind,
        fail_closed_action: FailClosedAction,
        user_message: String,
        audit_facts: Option<AuditFacts>,
    ) -> Self {
        Self {
            kind,
            fail_closed_action,
            user_message,
            audit_facts,
        }
    }

    /// Return the setup failure classification.
    pub fn kind(&self) -> SetupFailureKind {
        self.kind
    }

    /// Return the fail-closed action surfaces must apply.
    pub fn fail_closed_action(&self) -> FailClosedAction {
        self.fail_closed_action
    }

    /// Return the user-facing setup failure message.
    pub fn user_message(&self) -> &str {
        &self.user_message
    }

    /// Return pre-outcome audit facts when they were available.
    pub fn audit_facts(&self) -> Option<&AuditFacts> {
        self.audit_facts.as_ref()
    }
}

/// Typed planning context resolved before pure policy evaluation.
#[derive(Debug, Clone)]
pub struct DecisionContext {
    /// Effective execution mode.
    pub mode: Mode,
    /// Caller transport requesting the decision.
    pub transport: ExecutionTransport,
    /// Whether CI was detected for this invocation.
    pub ci_detected: bool,
    /// Working-directory resolution state.
    pub cwd_state: CwdState,
    /// Matching allowlist entry for the command in this context, if any.
    pub allowlist_match: Option<AllowlistMatch>,
    /// Snapshot plugins applicable to the resolved cwd.
    pub applicable_snapshot_plugins: Vec<&'static str>,
}

/// Working-directory resolution state visible to planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CwdState {
    /// The command cwd was resolved successfully.
    Resolved(PathBuf),
    /// The command cwd could not be resolved.
    Unavailable,
}

/// Approval requirement derived from policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalRequirement {
    /// No human confirmation is required.
    None,
    /// Human confirmation is required before execution may proceed.
    HumanConfirmationRequired,
}

/// Snapshot requirement derived from policy and cwd context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotPlan {
    /// No snapshots are required before execution.
    NotRequired,
    /// Snapshots are required, with the applicable plugin set already resolved.
    Required {
        applicable_plugins: Vec<&'static str>,
    },
}

/// Next-step execution handling required by the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionDisposition {
    /// Execute immediately.
    Execute,
    /// Require approval before execution.
    RequiresApproval,
    /// Hard-block execution.
    Block,
}

/// Fail-closed action surfaces must apply for setup failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailClosedAction {
    /// Deny execution.
    Deny,
    /// Hard-block execution.
    Block,
    /// Treat as internal error while remaining fail-closed.
    InternalError,
}

/// Setup-failure classification for typed fail-closed planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupFailureKind {
    /// The runtime config could not be prepared safely.
    InvalidConfig,
    /// The scanner could not be prepared safely.
    ScannerUnavailable,
    /// Cwd was required for the policy path but unavailable.
    CwdUnavailableForPolicy,
    /// Allowlist context resolution was ambiguous.
    AllowlistContextAmbiguous,
    /// Any other fail-closed setup error.
    OtherFailClosed,
}

/// Pre-outcome audit facts derived during planning.
#[derive(Debug, Clone)]
pub struct AuditFacts {
    /// Raw command string being planned.
    pub command: String,
    /// Assessed risk for the command.
    pub risk: RiskLevel,
    /// Stable audit representations of matched patterns.
    pub matched_patterns: Vec<MatchedPattern>,
    /// Effective mode used during planning.
    pub mode: Mode,
    /// Whether CI was detected during planning.
    pub ci_detected: bool,
    /// Whether any allowlist rule matched the command.
    pub allowlist_matched: bool,
    /// Whether allowlist changed the policy outcome.
    pub allowlist_effective: bool,
    /// Decision transport associated with the plan.
    pub transport: ExecutionTransport,
    /// Hard-block reason when policy blocked the command.
    pub block_reason: Option<BlockReason>,
}

impl AuditFacts {
    fn from_plan_inputs(
        assessment: &Assessment,
        decision_context: &DecisionContext,
        block_reason: Option<BlockReason>,
        allowlist_effective: bool,
    ) -> Self {
        Self {
            command: assessment.command.raw.clone(),
            risk: assessment.risk,
            matched_patterns: assessment.matched.iter().map(Into::into).collect(),
            mode: decision_context.mode,
            ci_detected: decision_context.ci_detected,
            allowlist_matched: decision_context.allowlist_match.is_some(),
            allowlist_effective,
            transport: decision_context.transport,
            block_reason,
        }
    }
}
