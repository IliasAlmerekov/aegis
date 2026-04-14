use std::path::PathBuf;

use crate::audit::MatchedPattern;
use crate::config::{AllowlistMatch, Mode};
use crate::decision::{BlockReason, ExecutionTransport, PolicyAction, PolicyDecision};
use crate::explanation::CommandExplanation;
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
    assessment: Box<Assessment>,
    decision_context: DecisionContext,
    policy_decision: PolicyDecision,
    approval_requirement: ApprovalRequirement,
    snapshot_plan: SnapshotPlan,
    execution_disposition: ExecutionDisposition,
    explanation: Box<CommandExplanation>,
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
        let explanation =
            CommandExplanation::from_plan_inputs(&assessment, &decision_context, policy_decision);

        Self {
            assessment: Box::new(assessment),
            decision_context,
            policy_decision,
            approval_requirement,
            snapshot_plan,
            execution_disposition,
            explanation: Box::new(explanation),
        }
    }

    /// Return the scanner assessment used to build the plan.
    pub fn assessment(&self) -> &Assessment {
        self.assessment.as_ref()
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

    /// Return the descriptive explanation assembled during planning.
    pub fn explanation(&self) -> &CommandExplanation {
        self.explanation.as_ref()
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
    mode: Mode,
    transport: ExecutionTransport,
    ci_detected: bool,
    cwd_state: CwdState,
    allowlist_match: Option<AllowlistMatch>,
    applicable_snapshot_plugins: Vec<&'static str>,
}

impl DecisionContext {
    /// Construct a decision context with all policy-relevant inputs resolved.
    pub(crate) fn new(
        mode: Mode,
        transport: ExecutionTransport,
        ci_detected: bool,
        cwd_state: CwdState,
        allowlist_match: Option<AllowlistMatch>,
        applicable_snapshot_plugins: Vec<&'static str>,
    ) -> Self {
        Self {
            mode,
            transport,
            ci_detected,
            cwd_state,
            allowlist_match,
            applicable_snapshot_plugins,
        }
    }

    /// Return the effective execution mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Return the caller transport requesting the decision.
    pub fn transport(&self) -> ExecutionTransport {
        self.transport
    }

    /// Return whether CI was detected for this invocation.
    pub fn ci_detected(&self) -> bool {
        self.ci_detected
    }

    /// Return the working-directory resolution state.
    pub fn cwd_state(&self) -> &CwdState {
        &self.cwd_state
    }

    /// Return the matching allowlist entry for the command in this context, if any.
    pub fn allowlist_match(&self) -> Option<&AllowlistMatch> {
        self.allowlist_match.as_ref()
    }

    /// Return the snapshot plugins applicable to the resolved cwd.
    pub fn applicable_snapshot_plugins(&self) -> &[&'static str] {
        self.applicable_snapshot_plugins.as_slice()
    }
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
    command: String,
    risk: RiskLevel,
    matched_patterns: Vec<MatchedPattern>,
    mode: Mode,
    ci_detected: bool,
    allowlist_matched: bool,
    allowlist_effective: bool,
    transport: ExecutionTransport,
    block_reason: Option<BlockReason>,
}

impl AuditFacts {
    pub(crate) fn from_plan_inputs(
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

    /// Return the raw command string being planned.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Return the assessed risk for the command.
    pub fn risk(&self) -> RiskLevel {
        self.risk
    }

    /// Return the stable audit representations of matched patterns.
    pub fn matched_patterns(&self) -> &[MatchedPattern] {
        self.matched_patterns.as_slice()
    }

    /// Return the effective mode used during planning.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Return whether CI was detected during planning.
    pub fn ci_detected(&self) -> bool {
        self.ci_detected
    }

    /// Return whether any allowlist rule matched the command.
    pub fn allowlist_matched(&self) -> bool {
        self.allowlist_matched
    }

    /// Return whether allowlist changed the policy outcome.
    pub fn allowlist_effective(&self) -> bool {
        self.allowlist_effective
    }

    /// Return the decision transport associated with the plan.
    pub fn transport(&self) -> ExecutionTransport {
        self.transport
    }

    /// Return the hard-block reason when policy blocked the command.
    pub fn block_reason(&self) -> Option<BlockReason> {
        self.block_reason
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::allowlist::AllowlistSourceLayer;
    use crate::decision::BlockReason;
    use crate::decision::{PolicyAction, PolicyDecision, PolicyRationale};
    use crate::explanation::CommandExplanation;
    use crate::interceptor;

    #[test]
    fn decision_context_constructor_preserves_read_access_via_getters() {
        let cwd_state = CwdState::Resolved(PathBuf::from("."));
        let allowlist_match = AllowlistMatch {
            pattern: "echo *".to_string(),
            reason: "trusted local echo".to_string(),
            source_layer: AllowlistSourceLayer::Project,
        };
        let applicable_snapshot_plugins = vec!["git"];
        let context = DecisionContext::new(
            Mode::Protect,
            ExecutionTransport::Shell,
            true,
            cwd_state.clone(),
            Some(allowlist_match.clone()),
            applicable_snapshot_plugins.clone(),
        );

        assert_eq!(context.mode(), Mode::Protect);
        assert_eq!(context.transport(), ExecutionTransport::Shell);
        assert!(context.ci_detected());
        assert_eq!(context.cwd_state(), &cwd_state);
        assert_eq!(context.allowlist_match(), Some(&allowlist_match));
        assert_eq!(
            context.applicable_snapshot_plugins(),
            applicable_snapshot_plugins.as_slice()
        );
    }

    #[test]
    fn audit_facts_exposes_pre_outcome_fields_via_getters() {
        let assessment = interceptor::assess("rm -rf /").unwrap();
        let decision_context = DecisionContext::new(
            Mode::Strict,
            ExecutionTransport::Shell,
            false,
            CwdState::Resolved(PathBuf::from(".")),
            None,
            Vec::new(),
        );

        let audit_facts = AuditFacts::from_plan_inputs(
            &assessment,
            &decision_context,
            Some(BlockReason::IntrinsicRiskBlock),
            false,
        );

        assert_eq!(audit_facts.command(), "rm -rf /");
        assert_eq!(audit_facts.risk(), RiskLevel::Block);
        assert!(!audit_facts.matched_patterns().is_empty());
        assert_eq!(audit_facts.mode(), Mode::Strict);
        assert!(!audit_facts.ci_detected());
        assert!(!audit_facts.allowlist_matched());
        assert!(!audit_facts.allowlist_effective());
        assert_eq!(audit_facts.transport(), ExecutionTransport::Shell);
        assert_eq!(
            audit_facts.block_reason(),
            Some(BlockReason::IntrinsicRiskBlock)
        );
    }

    #[test]
    fn from_policy_builds_command_explanation_once() {
        let assessment = interceptor::assess("rm -rf ./tmp").unwrap();
        let decision_context = DecisionContext::new(
            Mode::Protect,
            ExecutionTransport::Shell,
            false,
            CwdState::Resolved(PathBuf::from(".")),
            None,
            vec!["git"],
        );
        let policy_decision = PolicyDecision {
            decision: PolicyAction::Prompt,
            rationale: PolicyRationale::RequiresConfirmation,
            requires_confirmation: true,
            snapshots_required: true,
            allowlist_effective: false,
        };
        let expected_explanation =
            CommandExplanation::from_plan_inputs(&assessment, &decision_context, policy_decision);

        let plan = InterceptionPlan::from_policy(assessment, decision_context, policy_decision);

        assert_eq!(plan.explanation(), &expected_explanation);
        assert!(std::ptr::eq(plan.explanation(), plan.explanation()));
    }

    #[test]
    fn planning_keeps_allowlist_provenance_in_context_section() {
        let assessment = interceptor::assess("cargo test --lib").unwrap();
        let allowlist_match = AllowlistMatch {
            pattern: "cargo test *".to_string(),
            reason: "safe local verification".to_string(),
            source_layer: AllowlistSourceLayer::Global,
        };
        let decision_context = DecisionContext::new(
            Mode::Strict,
            ExecutionTransport::Shell,
            true,
            CwdState::Resolved(PathBuf::from(".")),
            Some(allowlist_match.clone()),
            vec!["git", "docker"],
        );
        let policy_decision = PolicyDecision {
            decision: PolicyAction::AutoApprove,
            rationale: PolicyRationale::AllowlistOverride,
            requires_confirmation: false,
            snapshots_required: false,
            allowlist_effective: true,
        };

        let plan = InterceptionPlan::from_policy(assessment, decision_context, policy_decision);

        let explanation = plan.explanation();
        let allowlist_explanation = explanation
            .context
            .allowlist_match
            .as_ref()
            .expect("planning should preserve allowlist provenance");

        assert_eq!(allowlist_explanation.pattern, allowlist_match.pattern);
        assert_eq!(allowlist_explanation.reason, allowlist_match.reason);
        assert_eq!(
            allowlist_explanation.source_layer,
            allowlist_match.source_layer
        );
    }
}
