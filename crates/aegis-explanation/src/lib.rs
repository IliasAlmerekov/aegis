//! Structured explanation types for Aegis interception decisions.

pub use aegis_policy::{BlockReason, ExecutionTransport, PolicyAction, PolicyRationale};

use aegis_config::ConfigSourceLayer;
use aegis_config::Mode;
use aegis_types::{AssessmentBasis, DecisionSource, RiskLevel};
use serde::{Deserialize, Serialize};

/// Descriptive explanation assembled from existing planning and runtime facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandExplanation {
    /// Scanner-derived explanation facts.
    pub scan: ScanExplanation,
    /// Policy-derived explanation facts.
    pub policy: PolicyExplanation,
    /// Execution-context facts that influenced planning.
    pub context: ExecutionContextExplanation,
    /// Runtime execution outcome facts, when execution reached that stage.
    pub outcome: Option<ExecutionOutcomeExplanation>,
}

/// Scanner facts that describe why a command was classified as it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanExplanation {
    /// Highest risk observed in the scanner assessment.
    pub highest_risk: RiskLevel,
    /// High-level source of the assessment — the v1 compatibility projection.
    /// Retained so existing explanation/audit consumers keep their field; the
    /// richer successor is `basis` (ADR-022 §4, §10).
    pub decision_source: DecisionSource,
    /// What produced the decision, expressed as the decisive `Match`es — every
    /// `Match` at the assessment's maximum `RiskLevel`, or `Fallback` when
    /// nothing matched (`AssessmentBasis`, ADR-022 §4). Carried in-memory
    /// alongside `decision_source`; `#[serde(skip)]` keeps it out of the v1
    /// audit JSONL so existing logs deserialize byte-for-byte. Iteration 2
    /// (Audit v2) promotes this to a persisted, v2-compat field.
    #[serde(skip)]
    pub basis: AssessmentBasis,
    /// Pattern matches preserved for explanation output.
    pub matched_patterns: Vec<ExplainedPatternMatch>,
}

/// Descriptive view of one matched pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplainedPatternMatch {
    /// Stable pattern identifier.
    pub id: String,
    /// Risk associated with the matched pattern.
    pub risk: RiskLevel,
    /// Pattern description copied from the scanner pipeline.
    pub description: String,
    /// Concrete matched text captured by the scanner.
    pub matched_text: String,
    /// Optional human-readable explanation of why the rule is risky.
    pub justification: Option<String>,
}

/// Policy facts resolved during planning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyExplanation {
    /// Planned policy action selected for this command.
    pub action: PolicyAction,
    /// Descriptive rationale carried through from policy evaluation.
    pub rationale: PolicyRationale,
    /// Whether human confirmation is required before execution.
    pub requires_confirmation: bool,
    /// Whether snapshots should be attempted before execution.
    pub snapshots_required: bool,
    /// Whether the allowlist materially changed the policy outcome.
    pub allowlist_effective: bool,
    /// Hard-block reason when policy selected a block action.
    pub block_reason: Option<BlockReason>,
}

impl PolicyExplanation {
    /// Return the canonical concise reason label for consumer projections.
    #[must_use]
    pub fn concise_reason_label(&self) -> &'static str {
        match self.rationale {
            PolicyRationale::AuditMode => "audit mode auto-approved this command",
            PolicyRationale::SafeCommand => "safe command",
            PolicyRationale::AllowlistOverride => "allowlist override applied",
            PolicyRationale::RequiresConfirmation => "requires confirmation",
            PolicyRationale::AnalysisConfirmationRequired => {
                "language analysis requires one-time confirmation"
            }
            PolicyRationale::AnalysisOverrideRequired => "analysis override required",
            PolicyRationale::IntrinsicRiskBlock => "blocked by intrinsic risk",
            PolicyRationale::ProtectCiPolicy => "blocked by protect-mode CI policy",
            PolicyRationale::StrictPolicy => "blocked by strict mode",
            PolicyRationale::BlocklistOverride => "blocked by user-defined blocklist rule",
            PolicyRationale::PolicyRulesOverride => "overridden by typed policy rule",
        }
    }
}

/// Execution-context facts already known during planning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionContextExplanation {
    /// Effective operating mode resolved before policy evaluation.
    pub mode: Mode,
    /// Product surface that requested the decision.
    pub transport: ExecutionTransport,
    /// Whether CI was detected for this invocation.
    pub ci_detected: bool,
    /// Matching allowlist entry resolved for this context, when present.
    pub allowlist_match: Option<AllowlistExplanation>,
    /// Snapshot plugins applicable to the resolved execution context.
    pub applicable_snapshot_plugins: Vec<String>,
}

/// Descriptive allowlist provenance for explanation output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowlistExplanation {
    /// Original configured allowlist pattern that matched.
    pub pattern: String,
    /// Operator-facing reason stored on the allowlist rule.
    pub reason: String,
    /// Config layer that supplied the effective rule.
    pub source_layer: ConfigSourceLayer,
}

/// Runtime-only execution outcome facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionOutcomeExplanation {
    /// Runtime decision observed after planning completed.
    pub decision: ExecutionDecisionExplanation,
    /// Snapshot records created before the command reached execution.
    pub snapshots: Vec<SnapshotOutcomeExplanation>,
}

/// User-visible execution decision for runtime explanations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionDecisionExplanation {
    /// The command ran after explicit approval.
    Approved,
    /// The command was denied by the human confirmation step.
    Denied,
    /// The command ran without a confirmation prompt.
    AutoApproved,
    /// The command was blocked before execution.
    Blocked,
}

/// Runtime snapshot record surfaced in the explanation model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotOutcomeExplanation {
    /// Plugin name that produced the snapshot.
    pub plugin: String,
    /// Opaque snapshot identifier returned by the plugin.
    pub snapshot_id: String,
}
