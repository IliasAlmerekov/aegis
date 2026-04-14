use serde::{Deserialize, Serialize};

use crate::audit::Decision;
use crate::config::{AllowlistMatch, AllowlistSourceLayer, Mode};
use crate::decision::{
    BlockReason, ExecutionTransport, PolicyAction, PolicyDecision, PolicyRationale,
};
use crate::interceptor::RiskLevel;
use crate::interceptor::scanner::{Assessment, DecisionSource};
use crate::planning::DecisionContext;
use crate::snapshot::SnapshotRecord;

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
    /// High-level source of the assessment.
    pub decision_source: DecisionSource,
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
    pub source_layer: AllowlistSourceLayer,
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

impl CommandExplanation {
    /// Build an explanation from planning-time inputs.
    #[must_use]
    pub fn from_plan_inputs(
        assessment: &Assessment,
        context: &DecisionContext,
        decision: PolicyDecision,
    ) -> Self {
        Self {
            scan: ScanExplanation {
                highest_risk: assessment.risk,
                decision_source: assessment.decision_source(),
                matched_patterns: assessment
                    .matched
                    .iter()
                    .map(ExplainedPatternMatch::from)
                    .collect(),
            },
            policy: PolicyExplanation {
                action: decision.decision,
                rationale: decision.rationale,
                requires_confirmation: decision.requires_confirmation,
                snapshots_required: decision.snapshots_required,
                allowlist_effective: decision.allowlist_effective,
                block_reason: decision.block_reason(),
            },
            context: ExecutionContextExplanation {
                mode: context.mode(),
                transport: context.transport(),
                ci_detected: context.ci_detected(),
                allowlist_match: context.allowlist_match().map(AllowlistExplanation::from),
                applicable_snapshot_plugins: context
                    .applicable_snapshot_plugins()
                    .iter()
                    .map(|plugin| (*plugin).to_string())
                    .collect(),
            },
            outcome: None,
        }
    }

    /// Return this explanation with runtime outcome facts appended.
    #[must_use]
    pub fn with_runtime_outcome(mut self, outcome: ExecutionOutcomeExplanation) -> Self {
        self.outcome = Some(outcome);
        self
    }
}

impl ExecutionOutcomeExplanation {
    /// Build the runtime outcome explanation from execution results.
    #[must_use]
    pub fn from_runtime(decision: Decision, snapshots: &[SnapshotRecord]) -> Self {
        Self {
            decision: match decision {
                Decision::Approved => ExecutionDecisionExplanation::Approved,
                Decision::Denied => ExecutionDecisionExplanation::Denied,
                Decision::AutoApproved => ExecutionDecisionExplanation::AutoApproved,
                Decision::Blocked => ExecutionDecisionExplanation::Blocked,
            },
            snapshots: snapshots
                .iter()
                .map(|snapshot| SnapshotOutcomeExplanation {
                    plugin: snapshot.plugin.to_string(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                })
                .collect(),
        }
    }
}

impl From<&crate::interceptor::scanner::MatchResult> for ExplainedPatternMatch {
    fn from(value: &crate::interceptor::scanner::MatchResult) -> Self {
        Self {
            id: value.pattern.id.to_string(),
            risk: value.pattern.risk,
            description: value.pattern.description.to_string(),
            matched_text: value.matched_text.clone(),
        }
    }
}

impl From<&AllowlistMatch> for AllowlistExplanation {
    fn from(value: &AllowlistMatch) -> Self {
        Self {
            pattern: value.pattern.clone(),
            reason: value.reason.clone(),
            source_layer: value.source_layer,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::audit::Decision;
    use crate::config::{AllowlistMatch, AllowlistSourceLayer, Mode};
    use crate::decision::{BlockReason, ExecutionTransport, PolicyAction, PolicyRationale};
    use crate::interceptor::parser::Parser;
    use crate::interceptor::patterns::{Category, Pattern, PatternSource};
    use crate::interceptor::scanner::MatchResult;
    use crate::planning::CwdState;

    use super::*;

    #[test]
    fn builds_base_explanation_from_existing_pipeline_facts() {
        let assessment = Assessment {
            risk: RiskLevel::Danger,
            matched: vec![
                MatchResult {
                    pattern: Arc::new(Pattern {
                        id: Cow::Borrowed("FS-001"),
                        category: Category::Filesystem,
                        risk: RiskLevel::Danger,
                        pattern: Cow::Borrowed("rm -rf"),
                        description: Cow::Borrowed("recursive delete"),
                        safe_alt: None,
                        source: PatternSource::Builtin,
                    }),
                    matched_text: "rm -rf".to_string(),
                    highlight_range: None,
                },
                MatchResult {
                    pattern: Arc::new(Pattern {
                        id: Cow::Borrowed("USR-001"),
                        category: Category::Process,
                        risk: RiskLevel::Warn,
                        pattern: Cow::Borrowed("curl | sh"),
                        description: Cow::Borrowed("custom shell pipe"),
                        safe_alt: None,
                        source: PatternSource::Custom,
                    }),
                    matched_text: "curl | sh".to_string(),
                    highlight_range: None,
                },
            ],
            highlight_ranges: vec![],
            command: Parser::parse("rm -rf target && curl example | sh"),
        };
        let context = DecisionContext::new(
            Mode::Protect,
            ExecutionTransport::Watch,
            true,
            CwdState::Resolved(PathBuf::from("/repo")),
            Some(AllowlistMatch {
                pattern: "cargo test *".to_string(),
                reason: "owned repo automation".to_string(),
                source_layer: AllowlistSourceLayer::Project,
            }),
            vec!["git", "docker"],
        );
        let decision = PolicyDecision {
            decision: PolicyAction::Prompt,
            rationale: PolicyRationale::RequiresConfirmation,
            requires_confirmation: true,
            snapshots_required: true,
            allowlist_effective: false,
        };

        let explanation = CommandExplanation::from_plan_inputs(&assessment, &context, decision);

        assert_eq!(explanation.scan.highest_risk, RiskLevel::Danger);
        assert_eq!(
            explanation.scan.decision_source,
            DecisionSource::CustomPattern
        );
        assert_eq!(explanation.scan.matched_patterns[0].id, "FS-001");
        assert_eq!(
            explanation.scan.matched_patterns[1].matched_text,
            "curl | sh"
        );
        assert_eq!(explanation.policy.action, PolicyAction::Prompt);
        assert_eq!(
            explanation.policy.rationale,
            PolicyRationale::RequiresConfirmation
        );
        assert!(explanation.policy.requires_confirmation);
        assert!(explanation.policy.snapshots_required);
        assert!(!explanation.policy.allowlist_effective);
        assert_eq!(explanation.policy.block_reason, None);
        assert_eq!(explanation.context.mode, Mode::Protect);
        assert_eq!(explanation.context.transport, ExecutionTransport::Watch);
        assert!(explanation.context.ci_detected);
        let allowlist = explanation
            .context
            .allowlist_match
            .as_ref()
            .expect("allowlist match should be present");
        assert_eq!(allowlist.pattern, "cargo test *");
        assert_eq!(allowlist.reason, "owned repo automation");
        assert_eq!(allowlist.source_layer, AllowlistSourceLayer::Project);
        assert_eq!(
            explanation.context.applicable_snapshot_plugins,
            vec!["git".to_string(), "docker".to_string()]
        );
        assert_eq!(explanation.outcome, None);
    }

    #[test]
    fn appends_runtime_outcome_without_rewriting_existing_sections() {
        let base = CommandExplanation {
            scan: ScanExplanation {
                highest_risk: RiskLevel::Warn,
                decision_source: DecisionSource::BuiltinPattern,
                matched_patterns: vec![ExplainedPatternMatch {
                    id: "PKG-001".to_string(),
                    risk: RiskLevel::Warn,
                    description: "package install".to_string(),
                    matched_text: "npm install".to_string(),
                }],
            },
            policy: PolicyExplanation {
                action: PolicyAction::Block,
                rationale: PolicyRationale::StrictPolicy,
                requires_confirmation: false,
                snapshots_required: false,
                allowlist_effective: false,
                block_reason: Some(BlockReason::StrictPolicy),
            },
            context: ExecutionContextExplanation {
                mode: Mode::Strict,
                transport: ExecutionTransport::Shell,
                ci_detected: false,
                allowlist_match: None,
                applicable_snapshot_plugins: vec!["git".to_string()],
            },
            outcome: None,
        };
        let expected_scan = base.scan.clone();
        let expected_policy = base.policy.clone();
        let expected_context = base.context.clone();

        let explained = base.with_runtime_outcome(ExecutionOutcomeExplanation::from_runtime(
            Decision::Blocked,
            &[SnapshotRecord {
                plugin: "git",
                snapshot_id: "stash@{0}".to_string(),
            }],
        ));

        assert_eq!(explained.scan, expected_scan);
        assert_eq!(explained.policy, expected_policy);
        assert_eq!(explained.context, expected_context);
        assert_eq!(
            explained.outcome,
            Some(ExecutionOutcomeExplanation {
                decision: ExecutionDecisionExplanation::Blocked,
                snapshots: vec![SnapshotOutcomeExplanation {
                    plugin: "git".to_string(),
                    snapshot_id: "stash@{0}".to_string(),
                }],
            })
        );
    }
}
