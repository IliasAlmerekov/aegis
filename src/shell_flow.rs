use std::path::Path;

use aegis::audit::Decision;
#[cfg(test)]
use aegis::config::AllowlistMatch;
use aegis::decision::BlockReason;
#[cfg(test)]
use aegis::decision::{
    ExecutionTransport, PolicyAction, PolicyAllowlistResult, PolicyCiState, PolicyConfigFlags,
    PolicyDecision, PolicyExecutionContext, PolicyInput, evaluate_policy,
};
#[cfg(test)]
use aegis::explanation::{
    AllowlistExplanation, CommandExplanation, ExecutionContextExplanation, PolicyExplanation,
    ScanExplanation,
};
use aegis::planning::{CwdState, ExecutionDisposition, InterceptionPlan, PreparedPlanner};
use aegis::runtime::AuditWriteOptions;
#[cfg(test)]
use aegis::runtime::RuntimeContext;
use aegis::snapshot::SnapshotRecord;
use aegis::ui::confirm::{show_confirmation, show_policy_block};

use crate::shell_compat::{ShellLaunchOptions, exec_command};
use crate::{EXIT_BLOCKED, EXIT_DENIED};

pub(crate) fn run_planned_shell_command(
    cmd: &str,
    verbose: bool,
    prepared: &PreparedPlanner,
    plan: &InterceptionPlan,
    launch: &ShellLaunchOptions,
) -> i32 {
    match plan.execution_disposition() {
        ExecutionDisposition::Execute => {
            let snapshots = create_snapshots_for_plan(prepared, plan, verbose);
            append_shell_audit(prepared, plan, Decision::AutoApproved, &snapshots, verbose);
            exec_command(cmd, launch)
        }
        ExecutionDisposition::RequiresApproval => {
            let snapshots = create_snapshots_for_plan(prepared, plan, verbose);
            let approved = show_confirmation(plan.assessment(), plan.explanation(), &snapshots);
            let decision = if approved {
                Decision::Approved
            } else {
                Decision::Denied
            };
            append_shell_audit(prepared, plan, decision, &snapshots, verbose);
            if approved {
                exec_command(cmd, launch)
            } else {
                EXIT_DENIED
            }
        }
        ExecutionDisposition::Block => {
            show_block_for_plan(plan);
            append_shell_audit(prepared, plan, Decision::Blocked, &[], verbose);
            EXIT_BLOCKED
        }
    }
}

fn create_snapshots_for_plan(
    prepared: &PreparedPlanner,
    plan: &InterceptionPlan,
    verbose: bool,
) -> Vec<SnapshotRecord> {
    if matches!(
        plan.snapshot_plan(),
        aegis::planning::SnapshotPlan::NotRequired
    ) {
        return Vec::new();
    }

    match prepared {
        PreparedPlanner::Ready(context) => match plan.decision_context().cwd_state() {
            CwdState::Resolved(path) => {
                context.create_snapshots(path.as_path(), &plan.assessment().command.raw, verbose)
            }
            CwdState::Unavailable => {
                context.create_snapshots(Path::new("."), &plan.assessment().command.raw, verbose)
            }
        },
        PreparedPlanner::SetupFailure(_) => Vec::new(),
    }
}

fn append_shell_audit(
    prepared: &PreparedPlanner,
    plan: &InterceptionPlan,
    decision: Decision,
    snapshots: &[SnapshotRecord],
    verbose: bool,
) {
    if let PreparedPlanner::Ready(context) = prepared {
        context.append_audit_entry(
            plan.assessment(),
            decision,
            snapshots,
            plan.explanation(),
            AuditWriteOptions {
                allowlist_match: plan.decision_context().allowlist_match(),
                allowlist_effective: plan.policy_decision().allowlist_effective,
                ci_detected: plan.decision_context().ci_detected(),
                verbose,
            },
        );
    }
}

fn show_block_for_plan(plan: &InterceptionPlan) {
    match plan.policy_decision().block_reason() {
        Some(BlockReason::ProtectCiPolicy) => {
            show_policy_block(plan.assessment(), plan.explanation())
        }
        Some(BlockReason::IntrinsicRiskBlock) => {
            show_confirmation(plan.assessment(), plan.explanation(), &[]);
        }
        Some(BlockReason::StrictPolicy) => {
            show_policy_block(plan.assessment(), plan.explanation());
        }
        None => {}
    }
}

#[cfg(test)]
pub(crate) fn decide_command(
    context: &RuntimeContext,
    assessment: &aegis::interceptor::scanner::Assessment,
    cwd: &Path,
    verbose: bool,
    allowlist_match: Option<&AllowlistMatch>,
    in_ci: bool,
) -> (Decision, Vec<SnapshotRecord>, bool) {
    let (policy_decision, applicable_snapshot_plugins) = evaluate_policy_decision(
        context,
        assessment,
        cwd,
        allowlist_match,
        in_ci,
        ExecutionTransport::Shell,
    );
    let explanation = test_command_explanation(
        context,
        assessment,
        policy_decision,
        allowlist_match,
        in_ci,
        ExecutionTransport::Shell,
        &applicable_snapshot_plugins,
    );
    execute_policy_decision(
        context,
        assessment,
        cwd,
        policy_decision,
        &explanation,
        verbose,
    )
}

#[cfg(test)]
fn execute_policy_decision(
    context: &RuntimeContext,
    assessment: &aegis::interceptor::scanner::Assessment,
    cwd: &Path,
    policy_decision: PolicyDecision,
    explanation: &CommandExplanation,
    verbose: bool,
) -> (Decision, Vec<SnapshotRecord>, bool) {
    let snapshots = if policy_decision.snapshots_required {
        context.create_snapshots(cwd, &assessment.command.raw, verbose)
    } else {
        Vec::new()
    };

    match policy_decision.decision {
        PolicyAction::AutoApprove => (
            Decision::AutoApproved,
            snapshots,
            policy_decision.allowlist_effective,
        ),
        PolicyAction::Prompt => {
            let approved = show_confirmation(assessment, explanation, &snapshots);
            let decision = if approved {
                Decision::Approved
            } else {
                Decision::Denied
            };

            (decision, snapshots, policy_decision.allowlist_effective)
        }
        PolicyAction::Block => {
            match policy_decision.block_reason() {
                Some(BlockReason::ProtectCiPolicy) => show_policy_block(assessment, explanation),
                Some(BlockReason::IntrinsicRiskBlock) => {
                    show_confirmation(assessment, explanation, &[]);
                }
                Some(BlockReason::StrictPolicy) => {
                    show_policy_block(assessment, explanation);
                }
                None => unreachable!("PolicyAction::Block always carries a BlockReason"),
            }

            (
                Decision::Blocked,
                snapshots,
                policy_decision.allowlist_effective,
            )
        }
    }
}

#[cfg(test)]
fn test_command_explanation(
    context: &RuntimeContext,
    assessment: &aegis::interceptor::scanner::Assessment,
    policy_decision: PolicyDecision,
    allowlist_match: Option<&AllowlistMatch>,
    in_ci: bool,
    transport: ExecutionTransport,
    applicable_snapshot_plugins: &[&'static str],
) -> CommandExplanation {
    CommandExplanation {
        scan: ScanExplanation {
            highest_risk: assessment.risk,
            decision_source: assessment.decision_source(),
            matched_patterns: assessment
                .matched
                .iter()
                .map(|matched| aegis::explanation::ExplainedPatternMatch {
                    id: matched.pattern.id.to_string(),
                    risk: matched.pattern.risk,
                    description: matched.pattern.description.to_string(),
                    matched_text: matched.matched_text.clone(),
                })
                .collect(),
        },
        policy: PolicyExplanation {
            action: policy_decision.decision,
            rationale: policy_decision.rationale,
            requires_confirmation: policy_decision.requires_confirmation,
            snapshots_required: policy_decision.snapshots_required,
            allowlist_effective: policy_decision.allowlist_effective,
            block_reason: policy_decision.block_reason(),
        },
        context: ExecutionContextExplanation {
            mode: context.config().mode,
            transport,
            ci_detected: in_ci,
            allowlist_match: allowlist_match.map(|rule| AllowlistExplanation {
                pattern: rule.pattern.clone(),
                reason: rule.reason.clone(),
                source_layer: rule.source_layer,
            }),
            applicable_snapshot_plugins: applicable_snapshot_plugins
                .iter()
                .map(|plugin| (*plugin).to_string())
                .collect(),
        },
        outcome: None,
    }
}

#[cfg(test)]
fn evaluate_policy_decision(
    context: &RuntimeContext,
    assessment: &aegis::interceptor::scanner::Assessment,
    cwd: &Path,
    allowlist_match: Option<&AllowlistMatch>,
    in_ci: bool,
    transport: ExecutionTransport,
) -> (PolicyDecision, Vec<&'static str>) {
    let applicable_snapshot_plugins = if assessment.risk == aegis::interceptor::RiskLevel::Danger
        && context.config().snapshot_policy != aegis::config::SnapshotPolicy::None
    {
        context.applicable_snapshot_plugins(cwd)
    } else {
        Vec::new()
    };
    let decision = evaluate_policy(PolicyInput {
        assessment,
        mode: context.config().mode,
        ci_state: PolicyCiState { detected: in_ci },
        allowlist: PolicyAllowlistResult {
            matched: allowlist_match.is_some(),
        },
        config_flags: PolicyConfigFlags {
            ci_policy: context.config().ci_policy,
            allowlist_override_level: context.config().strict_allowlist_override,
            snapshot_policy: context.config().snapshot_policy,
        },
        execution_context: PolicyExecutionContext {
            transport,
            applicable_snapshot_plugins: applicable_snapshot_plugins.as_slice(),
        },
    });

    (decision, applicable_snapshot_plugins)
}
