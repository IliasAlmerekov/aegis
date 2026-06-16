use std::path::Path;

use aegis::audit::Decision;
#[cfg(test)]
use aegis::config::AllowlistMatch;
use aegis::config::amend::{
    AppendOutcome, active_config_path_for_append, append_allow_rule, append_block_rule,
};
use aegis::decision::BlockReason;
#[cfg(test)]
use aegis::decision::{
    ExecutionTransport, PolicyAction, PolicyAllowlistResult, PolicyBlocklistResult, PolicyCiState,
    PolicyConfigFlags, PolicyDecision, PolicyExecutionContext, PolicyInput, evaluate_policy,
};
#[cfg(test)]
use aegis::explanation::{
    AllowlistExplanation, CommandExplanation, ExecutionContextExplanation, PolicyExplanation,
    ScanExplanation,
};
use aegis::interceptor::parser::{extract_prefix, split_tokens};
#[cfg(test)]
use aegis::planning::evaluate_policy_rules;
use aegis::planning::{CwdState, ExecutionDisposition, InterceptionPlan, PreparedPlanner};
use aegis::runtime::AuditWriteOptions;
#[cfg(test)]
use aegis::runtime::RuntimeContext;
use aegis::snapshot::SnapshotRecord;
use aegis::ui::confirm::{
    PromptDecision, show_confirmation, show_confirmation_decision, show_policy_block,
};
use aegis_types::SandboxStatus;

use crate::shell_compat::{ShellLaunchOptions, exec_command};
use crate::{EXIT_BLOCKED, EXIT_DENIED, EXIT_INTERNAL};

fn persist_rule(
    cmd: &str,
    plan: &InterceptionPlan,
    append_fn: impl FnOnce(
        &std::path::Path,
        &[String],
        &std::path::Path,
    ) -> Result<AppendOutcome, aegis::config::ConfigError>,
    label: &str,
) -> Result<(), String> {
    match active_config_path_for_append() {
        Some(config_path) => {
            let tokens = split_tokens(cmd);
            let prefix = extract_prefix(&tokens);
            let cwd = match plan.decision_context().cwd_state() {
                CwdState::Resolved(path) => path.clone(),
                CwdState::Unavailable => std::path::PathBuf::from("."),
            };
            match append_fn(&config_path, &prefix, &cwd) {
                Ok(AppendOutcome::Conflict {
                    pattern,
                    existing_location,
                }) => {
                    let location = match existing_location {
                        aegis::config::allowlist::ConfigSourceLayer::Project => "project",
                        aegis::config::allowlist::ConfigSourceLayer::Global => "global",
                    };
                    eprintln!(
                        "warning: conflicting rule for '{pattern}' already exists in {location} config"
                    );
                }
                Ok(AppendOutcome::SkippedDuplicate | AppendOutcome::Appended) => {}
                Err(err) => return Err(format!("{err}")),
            }
        }
        None => {
            eprintln!("warning: cannot persist {label} rule: no config file found");
        }
    }
    Ok(())
}

pub(crate) fn run_planned_shell_command(
    cmd: &str,
    verbose: bool,
    prepared: &PreparedPlanner,
    plan: &InterceptionPlan,
    launch: &ShellLaunchOptions,
) -> i32 {
    let sandbox_config = match prepared {
        PreparedPlanner::Ready(context) => context.config().sandbox.as_ref(),
        PreparedPlanner::SetupFailure(_) => None,
    };

    match plan.execution_disposition() {
        ExecutionDisposition::Execute => execute_with_snapshots(
            cmd,
            verbose,
            prepared,
            plan,
            launch,
            Decision::AutoApproved,
            sandbox_config,
        ),
        ExecutionDisposition::RequiresApproval => {
            let prompt_decision =
                show_confirmation_decision(plan.assessment(), plan.explanation(), &[]);
            if prompt_decision == PromptDecision::ApproveAlways
                && let Err(err) = persist_rule(cmd, plan, append_allow_rule, "allow")
            {
                eprintln!("error: failed to append allow rule: {err}");
            }
            if prompt_decision == PromptDecision::DenyAlways
                && let Err(err) = persist_rule(cmd, plan, append_block_rule, "block")
            {
                eprintln!("error: failed to append block rule: {err}");
            }
            let approved = matches!(
                prompt_decision,
                PromptDecision::Approve | PromptDecision::ApproveAlways
            );
            if approved {
                execute_with_snapshots(
                    cmd,
                    verbose,
                    prepared,
                    plan,
                    launch,
                    Decision::Approved,
                    sandbox_config,
                )
            } else {
                if let Err(err) = append_shell_audit(
                    prepared,
                    plan,
                    Decision::Denied,
                    &[],
                    SandboxStatus::NotConfigured,
                ) {
                    eprintln!("error: failed to write audit log: {err}");
                    return EXIT_INTERNAL;
                }
                EXIT_DENIED
            }
        }
        ExecutionDisposition::Block => {
            show_block_for_plan(plan);
            if let Err(err) = append_shell_audit(
                prepared,
                plan,
                Decision::Blocked,
                &[],
                SandboxStatus::NotConfigured,
            ) {
                eprintln!("error: failed to write audit log: {err}");
                return EXIT_INTERNAL;
            }
            EXIT_BLOCKED
        }
    }
}

/// Create snapshots, append the audit entry, and execute the command.
///
/// This helper captures the shared ordering for auto-approved and
/// human-approved execution branches: snapshot creation happens after the
/// final approval decision and before both the audit append and the child
/// process start.
fn execute_with_snapshots(
    cmd: &str,
    verbose: bool,
    prepared: &PreparedPlanner,
    plan: &InterceptionPlan,
    launch: &ShellLaunchOptions,
    decision: Decision,
    sandbox_config: Option<&aegis_sandbox::SandboxConfig>,
) -> i32 {
    let snapshots = create_snapshots_for_plan(prepared, plan, verbose);
    if let Err(err) = append_shell_audit(
        prepared,
        plan,
        decision,
        &snapshots,
        sandbox_status_for(sandbox_config),
    ) {
        eprintln!("error: failed to write audit log: {err}");
        return EXIT_INTERNAL;
    }
    exec_command(cmd, launch, sandbox_config)
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
    sandbox_status: SandboxStatus,
) -> Result<(), aegis::error::AegisError> {
    if let PreparedPlanner::Ready(context) = prepared {
        return context.append_audit_entry(
            plan.assessment(),
            decision,
            snapshots,
            plan.explanation(),
            AuditWriteOptions {
                allowlist_match: plan.decision_context().allowlist_match(),
                allowlist_effective: plan.policy_decision().allowlist_effective,
                ci_detected: plan.decision_context().ci_detected(),
                sandbox_status,
            },
        );
    }
    Ok(())
}

/// Map a sandbox config to the status recorded in the audit log.
///
/// `None` (no `[sandbox]` config) → `NotConfigured`; otherwise probe
/// availability via [`aegis_sandbox::sandbox_available_for`]: available →
/// `Active`, configured-but-unavailable → `Unavailable` (an audited bypass).
///
/// TOCTOU caveat: this is a separate availability probe from the one
/// `prepare_for_exec` performs at exec time, so a sandbox that disappears (or
/// appears) in between could make the audited status diverge from what actually
/// happened. The source of truth should ultimately be the real
/// `prepare_for_exec` outcome rather than this pre-probe; threading that result
/// back into the audit entry is a known follow-up.
fn sandbox_status_for(sandbox_config: Option<&aegis_sandbox::SandboxConfig>) -> SandboxStatus {
    SandboxStatus::from(sandbox_config.map(aegis_sandbox::sandbox_available_for))
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
        Some(BlockReason::BlocklistOverride) => {
            show_policy_block(plan.assessment(), plan.explanation());
        }
        Some(BlockReason::PolicyRulesOverride) => {
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
    match policy_decision.decision {
        PolicyAction::AutoApprove => {
            let snapshots = if policy_decision.snapshots_required {
                context.create_snapshots(cwd, &assessment.command.raw, verbose)
            } else {
                Vec::new()
            };
            (
                Decision::AutoApproved,
                snapshots,
                policy_decision.allowlist_effective,
            )
        }
        PolicyAction::Prompt => {
            let prompt_decision = show_confirmation_decision(assessment, explanation, &[]);
            let approved = matches!(
                prompt_decision,
                PromptDecision::Approve | PromptDecision::ApproveAlways
            );
            let decision = if approved {
                Decision::Approved
            } else {
                Decision::Denied
            };
            let snapshots = if approved && policy_decision.snapshots_required {
                context.create_snapshots(cwd, &assessment.command.raw, verbose)
            } else {
                Vec::new()
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
                Some(BlockReason::BlocklistOverride) => {
                    show_policy_block(assessment, explanation);
                }
                Some(BlockReason::PolicyRulesOverride) => {
                    show_policy_block(assessment, explanation);
                }
                None => unreachable!("PolicyAction::Block always carries a BlockReason"),
            }

            (
                Decision::Blocked,
                Vec::new(),
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
                    justification: matched.pattern.justification.as_deref().map(str::to_owned),
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
        blocklist: PolicyBlocklistResult {
            matched: context.is_blocked_for_command(&assessment.command.raw, Some(cwd)),
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
        rules: evaluate_policy_rules(context.policy_rules(), &assessment.command.raw),
    });

    (decision, applicable_snapshot_plugins)
}

#[cfg(test)]
mod snapshot_ordering_tests {
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;
    use tokio::runtime::Handle;

    use super::*;
    use aegis::config::{AegisConfig, AllowlistOverrideLevel, SnapshotPolicy};
    use aegis::decision::{PolicyAction, PolicyDecision, PolicyRationale};
    use aegis::runtime::RuntimeContext;

    fn test_handle() -> Handle {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime build");
        let handle = rt.handle().clone();
        std::mem::forget(rt);
        handle
    }

    fn danger_context() -> RuntimeContext {
        let mut config = AegisConfig::default();
        config.snapshot_policy = SnapshotPolicy::Selective;
        config.auto_snapshot_git = true;
        config.auto_snapshot_docker = false;
        config.allowlist_override_level = AllowlistOverrideLevel::Danger;
        RuntimeContext::new(config, test_handle()).expect("runtime context")
    }

    fn init_git_repo(path: &Path) {
        let init = Command::new("git")
            .arg("init")
            .current_dir(path)
            .output()
            .expect("git init");
        assert!(init.status.success(), "git init failed: {init:?}");

        let commit = Command::new("git")
            .args([
                "-c",
                "user.email=test@aegis.dev",
                "-c",
                "user.name=Aegis Test",
                "commit",
                "--allow-empty",
                "-m",
                "init",
            ])
            .current_dir(path)
            .output()
            .expect("git commit");
        assert!(commit.status.success(), "git commit failed: {commit:?}");
    }

    fn danger_explanation(
        context: &RuntimeContext,
        assessment: &aegis::interceptor::scanner::Assessment,
        policy_decision: PolicyDecision,
        plugins: &[&'static str],
    ) -> CommandExplanation {
        test_command_explanation(
            context,
            assessment,
            policy_decision,
            None,
            false,
            ExecutionTransport::Shell,
            plugins,
        )
    }

    #[test]
    fn test_execute_policy_decision_prompt_denied_records_no_snapshots() {
        let dir = TempDir::new().expect("temp dir");
        init_git_repo(dir.path());

        let context = danger_context();
        let assessment = aegis::interceptor::assess("rm -rf /tmp/aegis-denied-target").unwrap();
        assert_eq!(assessment.risk, aegis::interceptor::RiskLevel::Danger);

        let policy_decision = PolicyDecision {
            decision: PolicyAction::Prompt,
            rationale: PolicyRationale::RequiresConfirmation,
            requires_confirmation: true,
            snapshots_required: true,
            allowlist_effective: false,
        };
        let explanation = danger_explanation(&context, &assessment, policy_decision, &["git"]);

        let (decision, snapshots, _) = execute_policy_decision(
            &context,
            &assessment,
            dir.path(),
            policy_decision,
            &explanation,
            false,
        );

        assert_eq!(decision, Decision::Denied);
        assert!(
            snapshots.is_empty(),
            "denied prompt must not create snapshots, got {snapshots:?}"
        );
    }

    #[test]
    fn test_execute_policy_decision_block_records_no_snapshots() {
        let dir = TempDir::new().expect("temp dir");
        init_git_repo(dir.path());

        let context = danger_context();
        let assessment = aegis::interceptor::assess("rm -rf /").unwrap();
        assert_eq!(assessment.risk, aegis::interceptor::RiskLevel::Block);

        let policy_decision = PolicyDecision {
            decision: PolicyAction::Block,
            rationale: aegis::decision::PolicyRationale::IntrinsicRiskBlock,
            requires_confirmation: false,
            snapshots_required: true,
            allowlist_effective: false,
        };
        let explanation = danger_explanation(&context, &assessment, policy_decision, &[]);

        let (decision, snapshots, _) = execute_policy_decision(
            &context,
            &assessment,
            dir.path(),
            policy_decision,
            &explanation,
            false,
        );

        assert_eq!(decision, Decision::Blocked);
        assert!(
            snapshots.is_empty(),
            "block decision must not create snapshots, got {snapshots:?}"
        );
    }
}
