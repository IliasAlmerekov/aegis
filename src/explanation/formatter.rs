//! Explanation formatter: render human-readable decision explanations.

use super::templates::{
    AllowlistExplanation, CommandExplanation, ExecutionContextExplanation,
    ExecutionDecisionExplanation, ExecutionOutcomeExplanation, ExplainedPatternMatch,
    PolicyExplanation, ScanExplanation, SnapshotOutcomeExplanation,
};

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static FROM_PLAN_INPUTS_CALL_COUNT: Cell<usize> = const { Cell::new(0) };
}

use crate::audit::Decision;
use crate::config::AllowlistMatch;
use crate::decision::PolicyDecision;
use crate::interceptor::scanner::{Assessment, MatchResult};
use crate::planning::DecisionContext;
use crate::snapshot::SnapshotRecord;

/// Build a [`CommandExplanation`] from planning-time inputs.
#[must_use]
pub fn build_explanation_from_plan(
    assessment: &Assessment,
    context: &DecisionContext,
    decision: PolicyDecision,
) -> CommandExplanation {
    #[cfg(test)]
    FROM_PLAN_INPUTS_CALL_COUNT.with(|count| count.set(count.get() + 1));

    CommandExplanation {
        scan: ScanExplanation {
            highest_risk: assessment.risk,
            decision_source: assessment.decision_source(),
            matched_patterns: assessment
                .matched
                .iter()
                .map(explained_pattern_match_from)
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
            allowlist_match: context.allowlist_match().map(allowlist_explanation_from),
            applicable_snapshot_plugins: context
                .applicable_snapshot_plugins()
                .iter()
                .map(|plugin| (*plugin).to_string())
                .collect(),
        },
        outcome: None,
    }
}

/// Extension trait for [`CommandExplanation`] that adds builder-style methods
/// needed in the binary crate.
///
/// Using an extension trait is required because `CommandExplanation` is defined
/// in the `aegis-explanation` crate and inherent `impl` blocks for external
/// types are forbidden by the orphan rule.
pub trait CommandExplanationExt {
    /// Return this explanation with runtime outcome facts appended.
    #[must_use]
    fn with_runtime_outcome(self, outcome: ExecutionOutcomeExplanation) -> Self;
}

impl CommandExplanationExt for CommandExplanation {
    fn with_runtime_outcome(mut self, outcome: ExecutionOutcomeExplanation) -> Self {
        self.outcome = Some(outcome);
        self
    }
}

/// Build the runtime outcome explanation from execution results.
#[must_use]
pub fn build_outcome_explanation(
    decision: Decision,
    snapshots: &[SnapshotRecord],
) -> ExecutionOutcomeExplanation {
    ExecutionOutcomeExplanation {
        decision: match decision {
            Decision::Approved => ExecutionDecisionExplanation::Approved,
            Decision::Denied => ExecutionDecisionExplanation::Denied,
            Decision::AutoApproved => ExecutionDecisionExplanation::AutoApproved,
            Decision::Blocked | Decision::Pruned => ExecutionDecisionExplanation::Blocked,
            // Fail closed for any future unknown decision variant.
            _ => ExecutionDecisionExplanation::Blocked,
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

/// Build an [`ExplainedPatternMatch`] from a scanner [`MatchResult`].
#[must_use]
pub fn explained_pattern_match_from(value: &MatchResult) -> ExplainedPatternMatch {
    ExplainedPatternMatch {
        id: value.pattern.id.to_string(),
        risk: value.pattern.risk,
        description: value.pattern.description.to_string(),
        matched_text: value.matched_text.clone(),
        justification: value.pattern.justification.as_deref().map(str::to_owned),
    }
}

/// Build an [`AllowlistExplanation`] from a config [`AllowlistMatch`].
#[must_use]
pub fn allowlist_explanation_from(value: &AllowlistMatch) -> AllowlistExplanation {
    AllowlistExplanation {
        pattern: value.pattern.clone(),
        reason: value.reason.clone(),
        source_layer: value.source_layer,
    }
}

#[cfg(test)]
pub(crate) fn reset_from_plan_inputs_call_count_for_tests() {
    FROM_PLAN_INPUTS_CALL_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn from_plan_inputs_call_count_for_tests() -> usize {
    FROM_PLAN_INPUTS_CALL_COUNT.with(Cell::get)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::thread;

    use crate::audit::Decision;
    use crate::config::{AllowlistMatch, ConfigSourceLayer, Mode};
    use crate::decision::{BlockReason, ExecutionTransport, PolicyAction, PolicyRationale};
    use crate::interceptor::RiskLevel;
    use crate::interceptor::parser::Parser;
    use crate::interceptor::patterns::{Category, Pattern, PatternSource};
    use crate::interceptor::scanner::{DecisionSource, MatchResult};
    use crate::planning::CwdState;
    use crate::snapshot::SnapshotRecord;

    use super::super::templates::{
        CommandExplanation, ExecutionContextExplanation, ExecutionDecisionExplanation,
        ExecutionOutcomeExplanation, ExplainedPatternMatch, PolicyExplanation, ScanExplanation,
        SnapshotOutcomeExplanation,
    };
    use super::*;

    fn test_explanation() -> CommandExplanation {
        CommandExplanation {
            scan: ScanExplanation {
                highest_risk: RiskLevel::Warn,
                decision_source: DecisionSource::BuiltinPattern,
                matched_patterns: vec![ExplainedPatternMatch {
                    id: "GIT-001".to_string(),
                    risk: RiskLevel::Warn,
                    description: "hard reset".to_string(),
                    matched_text: "git reset --hard".to_string(),
                    justification: None,
                }],
            },
            policy: PolicyExplanation {
                action: PolicyAction::Prompt,
                rationale: PolicyRationale::RequiresConfirmation,
                requires_confirmation: true,
                snapshots_required: false,
                allowlist_effective: false,
                block_reason: None,
            },
            context: ExecutionContextExplanation {
                mode: Mode::Protect,
                transport: ExecutionTransport::Shell,
                ci_detected: false,
                allowlist_match: None,
                applicable_snapshot_plugins: vec!["git".to_string()],
            },
            outcome: None,
        }
    }

    #[test]
    fn builds_base_explanation_from_existing_pipeline_facts() {
        let assessment = crate::interceptor::scanner::Assessment {
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
                        justification: None,
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
                        justification: None,
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
                source_layer: ConfigSourceLayer::Project,
            }),
            vec!["git", "docker"],
        );
        let decision = crate::decision::PolicyDecision {
            decision: PolicyAction::Prompt,
            rationale: PolicyRationale::RequiresConfirmation,
            requires_confirmation: true,
            snapshots_required: true,
            allowlist_effective: false,
        };

        let explanation = build_explanation_from_plan(&assessment, &context, decision);

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
        assert_eq!(allowlist.source_layer, ConfigSourceLayer::Project);
        assert_eq!(
            explanation.context.applicable_snapshot_plugins,
            vec!["git".to_string(), "docker".to_string()]
        );
        assert_eq!(explanation.outcome, None);
    }

    #[test]
    fn from_plan_inputs_counter_is_isolated_per_thread() {
        let assessment = crate::interceptor::assess("echo hello").unwrap();
        let context = DecisionContext::new(
            Mode::Protect,
            ExecutionTransport::Shell,
            false,
            CwdState::Resolved(PathBuf::from(".")),
            None,
            Vec::new(),
        );
        let decision = crate::decision::PolicyDecision {
            decision: PolicyAction::AutoApprove,
            rationale: PolicyRationale::SafeCommand,
            requires_confirmation: false,
            snapshots_required: false,
            allowlist_effective: false,
        };

        reset_from_plan_inputs_call_count_for_tests();
        let _ = build_explanation_from_plan(&assessment, &context, decision);
        thread::spawn(move || {
            let _ = build_explanation_from_plan(&assessment, &context, decision);
        })
        .join()
        .unwrap();

        assert_eq!(from_plan_inputs_call_count_for_tests(), 1);
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
                    justification: None,
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

        let explained = base.with_runtime_outcome(build_outcome_explanation(
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

    #[test]
    fn explanation_json_preserves_layer_boundaries() {
        let explanation = test_explanation().with_runtime_outcome(ExecutionOutcomeExplanation {
            decision: ExecutionDecisionExplanation::Approved,
            snapshots: vec![SnapshotOutcomeExplanation {
                plugin: "git".to_string(),
                snapshot_id: "snap-123".to_string(),
            }],
        });

        let json = serde_json::to_value(&explanation).expect("explanation should serialize");
        let object = json
            .as_object()
            .expect("command explanation should serialize as a JSON object");
        let keys = object.keys().map(String::as_str).collect::<Vec<_>>();

        assert_eq!(keys.len(), 4);
        assert!(json.get("scan").is_some());
        assert!(json.get("policy").is_some());
        assert!(json.get("context").is_some());
        assert!(json.get("outcome").is_some());
        assert!(json.get("highest_risk").is_none());
        assert!(json.get("action").is_none());
        assert!(json.get("transport").is_none());
        assert!(json.get("decision").is_none());
    }

    #[test]
    fn concise_reason_label_uses_canonical_policy_labels() {
        let labels = [
            (
                PolicyRationale::AuditMode,
                "audit mode auto-approved this command",
            ),
            (PolicyRationale::SafeCommand, "safe command"),
            (
                PolicyRationale::AllowlistOverride,
                "allowlist override applied",
            ),
            (
                PolicyRationale::RequiresConfirmation,
                "requires confirmation",
            ),
            (
                PolicyRationale::IntrinsicRiskBlock,
                "blocked by intrinsic risk",
            ),
            (
                PolicyRationale::ProtectCiPolicy,
                "blocked by protect-mode CI policy",
            ),
            (PolicyRationale::StrictPolicy, "blocked by strict mode"),
        ];

        for (rationale, expected) in labels {
            let explanation = PolicyExplanation {
                action: PolicyAction::Prompt,
                rationale,
                requires_confirmation: matches!(rationale, PolicyRationale::RequiresConfirmation),
                snapshots_required: false,
                allowlist_effective: false,
                block_reason: rationale.block_reason(),
            };

            assert_eq!(explanation.concise_reason_label(), expected);
        }
    }
}
