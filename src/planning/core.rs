//! Pure planning logic: build a planning outcome from a runtime context.

use std::path::Path;

use crate::decision::{
    ExecutionTransport, PolicyAllowlistResult, PolicyBlocklistResult, PolicyCiState,
    PolicyConfigFlags, PolicyExecutionContext, PolicyInput, evaluate_policy,
};
use crate::planning::policy_rules::evaluate_policy_rules;
use crate::planning::types::{CwdState, DecisionContext, InterceptionPlan, PlanningOutcome};
use crate::runtime::RuntimeContext;

/// Typed request for pure planning against an already prepared runtime context.
#[derive(Debug, Clone)]
pub struct PlanningRequest<'a> {
    /// Raw shell command to assess and plan.
    pub command: &'a str,
    /// Working-directory resolution state for the command.
    pub cwd_state: CwdState,
    /// Transport requesting the plan.
    pub transport: ExecutionTransport,
    /// Whether CI was detected for this invocation.
    pub ci_detected: bool,
}

/// Build a typed planning outcome from runtime context plus one request.
pub fn plan_with_context(
    context: &RuntimeContext,
    request: PlanningRequest<'_>,
) -> PlanningOutcome {
    let assessment = context
        .assess_with_language_analysis_in_cwd(request.command, analysis_cwd(&request.cwd_state));
    let allowlist_match = match &request.cwd_state {
        CwdState::Resolved(path) => {
            context.allowlist_match_for_command(request.command, Some(path.as_path()))
        }
        CwdState::Unavailable => context.allowlist_match_for_command(request.command, None),
    };
    let blocklist_match = match &request.cwd_state {
        CwdState::Resolved(path) => {
            context.is_blocked_for_command(request.command, Some(path.as_path()))
        }
        CwdState::Unavailable => context.is_blocked_for_command(request.command, None),
    };
    let applicable_snapshot_plugins =
        if recovery_backstop_applies(&assessment, context.config().snapshot_policy) {
            match &request.cwd_state {
                CwdState::Resolved(path) => context.applicable_snapshot_plugins(path),
                CwdState::Unavailable => context.applicable_snapshot_plugins(Path::new(".")),
            }
        } else {
            Vec::new()
        };

    build_planning_outcome(
        context,
        request,
        assessment,
        allowlist_match,
        blocklist_match,
        applicable_snapshot_plugins,
    )
}

/// Whether a recovery (pre-exec snapshot) backstop must be considered for this
/// command. ADR-016: recovery is the primary v1 backstop for *both* `Danger`
/// commands and effect-opaque execution — the two axes are orthogonal, so
/// either one warrants resolving applicable snapshot plugins.
/// `SnapshotPolicy::None` (the trusted/global opt-out) suppresses both.
fn recovery_backstop_applies(
    assessment: &crate::interceptor::scanner::Assessment,
    snapshot_policy: crate::config::SnapshotPolicy,
) -> bool {
    (assessment.risk == crate::interceptor::RiskLevel::Danger || assessment.effect_opaque)
        && snapshot_policy != crate::config::SnapshotPolicy::None
}

/// Async variant of `plan_with_context` for callers already inside an async
/// runtime. Avoids the nested `block_on` panic when resolving applicable
/// snapshot plugins.
pub async fn plan_with_context_async(
    context: &RuntimeContext,
    request: PlanningRequest<'_>,
) -> PlanningOutcome {
    let assessment = context
        .assess_with_language_analysis_async_in_cwd(
            request.command,
            analysis_cwd(&request.cwd_state),
        )
        .await;
    let allowlist_match = match &request.cwd_state {
        CwdState::Resolved(path) => {
            context.allowlist_match_for_command(request.command, Some(path.as_path()))
        }
        CwdState::Unavailable => context.allowlist_match_for_command(request.command, None),
    };
    let blocklist_match = match &request.cwd_state {
        CwdState::Resolved(path) => {
            context.is_blocked_for_command(request.command, Some(path.as_path()))
        }
        CwdState::Unavailable => context.is_blocked_for_command(request.command, None),
    };
    let applicable_snapshot_plugins =
        if recovery_backstop_applies(&assessment, context.config().snapshot_policy) {
            match &request.cwd_state {
                CwdState::Resolved(path) => context.applicable_snapshot_plugins_async(path).await,
                CwdState::Unavailable => {
                    context
                        .applicable_snapshot_plugins_async(Path::new("."))
                        .await
                }
            }
        } else {
            Vec::new()
        };

    build_planning_outcome(
        context,
        request,
        assessment,
        allowlist_match,
        blocklist_match,
        applicable_snapshot_plugins,
    )
}

fn analysis_cwd(cwd_state: &CwdState) -> crate::analysis::AnalysisCwd<'_> {
    match cwd_state {
        CwdState::Resolved(path) => crate::analysis::AnalysisCwd::Resolved(path.as_path()),
        CwdState::Unavailable => crate::analysis::AnalysisCwd::Unavailable,
    }
}

fn build_planning_outcome(
    context: &RuntimeContext,
    request: PlanningRequest<'_>,
    assessment: crate::interceptor::scanner::Assessment,
    allowlist_match: Option<crate::config::AllowlistMatch>,
    blocklist_match: bool,
    applicable_snapshot_plugins: Vec<&'static str>,
) -> PlanningOutcome {
    let decision_context = DecisionContext::new(
        context.config().mode,
        request.transport,
        request.ci_detected,
        request.cwd_state,
        allowlist_match,
        applicable_snapshot_plugins,
    );

    let policy_decision = evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode: decision_context.mode(),
        ci_state: PolicyCiState {
            detected: decision_context.ci_detected(),
        },
        allowlist: PolicyAllowlistResult {
            matched: decision_context.allowlist_match().is_some(),
        },
        blocklist: PolicyBlocklistResult {
            matched: blocklist_match,
        },
        config_flags: PolicyConfigFlags {
            ci_policy: context.config().ci_policy,
            allowlist_override_level: context.config().strict_allowlist_override,
            snapshot_policy: context.config().snapshot_policy,
        },
        execution_context: PolicyExecutionContext {
            transport: decision_context.transport(),
            applicable_snapshot_plugins: decision_context.applicable_snapshot_plugins(),
        },
        rules: evaluate_policy_rules(context.policy_rules(), request.command),
    });

    PlanningOutcome::Planned(InterceptionPlan::from_policy(
        assessment,
        decision_context,
        policy_decision,
    ))
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::Mutex;

    use super::*;
    use crate::config::{AegisConfig, Mode, SnapshotPolicy};
    use crate::decision::ExecutionTransport;
    use crate::planning::types::{
        ApprovalRequirement, ExecutionDisposition, PlanningOutcome, SnapshotPlan,
    };
    use crate::runtime::RuntimeContext;
    use tempfile::TempDir;
    use tokio::runtime::Handle;

    fn test_handle() -> Handle {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = rt.handle().clone();
        std::mem::forget(rt);
        handle
    }

    static CURRENT_DIR_TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn context(mode: Mode, snapshot_policy: SnapshotPolicy) -> RuntimeContext {
        let mut config = AegisConfig::default();
        config.mode = mode;
        config.snapshot_policy = snapshot_policy;
        config.auto_snapshot_git = false;
        config.auto_snapshot_docker = false;
        RuntimeContext::new(config, test_handle()).unwrap()
    }

    #[test]
    fn safe_command_plans_execute_without_approval() {
        let context = context(Mode::Protect, SnapshotPolicy::Selective);
        let outcome = super::plan_with_context(
            &context,
            super::PlanningRequest {
                command: "echo hello",
                cwd_state: CwdState::Resolved(std::path::PathBuf::from(".")),
                transport: ExecutionTransport::Shell,
                ci_detected: false,
            },
        );

        let PlanningOutcome::Planned(plan) = outcome else {
            panic!("safe command must produce a normal plan");
        };
        assert_eq!(plan.execution_disposition(), ExecutionDisposition::Execute);
        assert_eq!(plan.approval_requirement(), ApprovalRequirement::None);
        assert_eq!(plan.snapshot_plan(), SnapshotPlan::NotRequired);
    }

    #[test]
    fn safe_command_plan_does_not_materialize_snapshot_registry() {
        crate::snapshot::reset_snapshot_registry_build_count_for_tests();

        let mut config = AegisConfig::default();
        config.mode = Mode::Protect;
        config.snapshot_policy = SnapshotPolicy::Selective;
        config.auto_snapshot_git = true;
        config.auto_snapshot_docker = false;
        let context = RuntimeContext::new(config, test_handle()).unwrap();

        let outcome = super::plan_with_context(
            &context,
            super::PlanningRequest {
                command: "echo hello",
                cwd_state: CwdState::Resolved(std::path::PathBuf::from(".")),
                transport: ExecutionTransport::Shell,
                ci_detected: false,
            },
        );

        let PlanningOutcome::Planned(plan) = outcome else {
            panic!("safe command must produce a normal plan");
        };
        assert_eq!(plan.snapshot_plan(), SnapshotPlan::NotRequired);
        assert_eq!(
            crate::snapshot::snapshot_registry_build_count_for_tests(),
            0
        );
    }

    #[test]
    fn protect_warn_plans_requires_approval() {
        let context = context(Mode::Protect, SnapshotPolicy::Selective);
        let outcome = super::plan_with_context(
            &context,
            super::PlanningRequest {
                command: "git stash clear",
                cwd_state: CwdState::Resolved(std::path::PathBuf::from(".")),
                transport: ExecutionTransport::Shell,
                ci_detected: false,
            },
        );

        let PlanningOutcome::Planned(plan) = outcome else {
            panic!("warn command must produce a normal plan");
        };
        assert_eq!(
            plan.execution_disposition(),
            ExecutionDisposition::RequiresApproval
        );
        assert_eq!(
            plan.approval_requirement(),
            ApprovalRequirement::HumanConfirmationRequired
        );
    }

    #[test]
    fn warn_command_plan_keeps_snapshot_registry_unmaterialized() {
        crate::snapshot::reset_snapshot_registry_build_count_for_tests();

        let mut config = AegisConfig::default();
        config.mode = Mode::Protect;
        config.snapshot_policy = SnapshotPolicy::Selective;
        config.auto_snapshot_git = true;
        let context = RuntimeContext::new(config, test_handle()).unwrap();

        let outcome = super::plan_with_context(
            &context,
            super::PlanningRequest {
                command: "git stash clear",
                cwd_state: CwdState::Resolved(std::path::PathBuf::from(".")),
                transport: ExecutionTransport::Shell,
                ci_detected: false,
            },
        );

        let PlanningOutcome::Planned(plan) = outcome else {
            panic!("warn command must produce a normal plan");
        };
        assert_eq!(plan.snapshot_plan(), SnapshotPlan::NotRequired);
        assert_eq!(
            crate::snapshot::snapshot_registry_build_count_for_tests(),
            0
        );
    }

    #[test]
    fn block_command_plans_block_without_approval() {
        let context = context(Mode::Strict, SnapshotPolicy::Full);
        let outcome = super::plan_with_context(
            &context,
            super::PlanningRequest {
                command: "rm -rf /",
                cwd_state: CwdState::Resolved(std::path::PathBuf::from(".")),
                transport: ExecutionTransport::Shell,
                ci_detected: false,
            },
        );

        let PlanningOutcome::Planned(plan) = outcome else {
            panic!("block command must produce a normal plan");
        };
        assert_eq!(plan.execution_disposition(), ExecutionDisposition::Block);
        assert_eq!(plan.approval_requirement(), ApprovalRequirement::None);
    }

    #[test]
    fn unavailable_cwd_uses_legacy_snapshot_plugin_fallback_in_plan() {
        let _guard = CURRENT_DIR_TEST_MUTEX.lock().unwrap();
        let original_cwd = std::env::current_dir().unwrap();
        let workspace = TempDir::new().unwrap();
        Command::new("git")
            .arg("init")
            .current_dir(workspace.path())
            .output()
            .unwrap();
        std::env::set_current_dir(workspace.path()).unwrap();

        let mut config = AegisConfig::default();
        config.mode = Mode::Protect;
        config.snapshot_policy = SnapshotPolicy::Selective;
        config.auto_snapshot_git = true;
        config.auto_snapshot_docker = false;
        let context = RuntimeContext::new(config, test_handle()).unwrap();

        let outcome = super::plan_with_context(
            &context,
            super::PlanningRequest {
                command: "terraform destroy -target=module.prod.api",
                cwd_state: CwdState::Unavailable,
                transport: ExecutionTransport::Shell,
                ci_detected: false,
            },
        );

        std::env::set_current_dir(original_cwd).unwrap();

        let PlanningOutcome::Planned(plan) = outcome else {
            panic!("danger command must produce a normal plan");
        };
        assert_eq!(
            plan.snapshot_plan(),
            SnapshotPlan::Required {
                applicable_plugins: vec!["git"],
            }
        );
    }

    #[test]
    fn danger_command_plan_materializes_snapshot_registry_once() {
        crate::snapshot::reset_snapshot_registry_build_count_for_tests();

        let _guard = CURRENT_DIR_TEST_MUTEX.lock().unwrap();
        let original_cwd = std::env::current_dir().unwrap();
        let workspace = TempDir::new().unwrap();
        Command::new("git")
            .arg("init")
            .current_dir(workspace.path())
            .output()
            .unwrap();
        std::env::set_current_dir(workspace.path()).unwrap();

        let mut config = AegisConfig::default();
        config.mode = Mode::Protect;
        config.snapshot_policy = SnapshotPolicy::Selective;
        config.auto_snapshot_git = true;
        config.auto_snapshot_docker = false;
        let context = RuntimeContext::new(config, test_handle()).unwrap();

        let outcome = super::plan_with_context(
            &context,
            super::PlanningRequest {
                command: "terraform destroy -target=module.prod.api",
                cwd_state: CwdState::Unavailable,
                transport: ExecutionTransport::Shell,
                ci_detected: false,
            },
        );

        std::env::set_current_dir(original_cwd).unwrap();

        let PlanningOutcome::Planned(plan) = outcome else {
            panic!("danger command must produce a normal plan");
        };
        assert!(matches!(
            plan.snapshot_plan(),
            SnapshotPlan::Required { .. }
        ));
        assert_eq!(
            crate::snapshot::snapshot_registry_build_count_for_tests(),
            1
        );
    }

    #[test]
    fn effect_opaque_script_degradation_requires_approval_and_recovery_snapshot() {
        // ADR-016 + ADR-022: recovery and language-analysis approval are
        // orthogonal. A script that exceeds its read budget needs a one-time
        // approval, while effect opacity still requires the recovery snapshot.
        crate::snapshot::reset_snapshot_registry_build_count_for_tests();

        let _guard = CURRENT_DIR_TEST_MUTEX.lock().unwrap();
        let original_cwd = std::env::current_dir().unwrap();
        let workspace = TempDir::new().unwrap();
        Command::new("git")
            .arg("init")
            .current_dir(workspace.path())
            .output()
            .unwrap();
        std::fs::write(workspace.path().join("cleanup.sh"), "echo ok\n").unwrap();
        std::env::set_current_dir(workspace.path()).unwrap();

        let mut config = AegisConfig::default();
        config.mode = Mode::Protect;
        config.snapshot_policy = SnapshotPolicy::Selective;
        config.auto_snapshot_git = true;
        config.auto_snapshot_docker = false;
        config.language_analysis.script_file_limit_bytes = 1;
        let context = RuntimeContext::new(config, test_handle()).unwrap();

        let outcome = super::plan_with_context(
            &context,
            super::PlanningRequest {
                command: "sh ./cleanup.sh",
                cwd_state: CwdState::Unavailable,
                transport: ExecutionTransport::Shell,
                ci_detected: false,
            },
        );

        std::env::set_current_dir(original_cwd).unwrap();

        let PlanningOutcome::Planned(plan) = outcome else {
            panic!("effect-opaque safe command must produce a normal plan");
        };
        assert_eq!(
            plan.execution_disposition(),
            ExecutionDisposition::RequiresApproval
        );
        assert_eq!(
            plan.approval_requirement(),
            ApprovalRequirement::HumanConfirmationRequired
        );
        // Recovery backstop: a pre-exec snapshot is requested from the git plugin.
        assert_eq!(
            plan.snapshot_plan(),
            SnapshotPlan::Required {
                applicable_plugins: vec!["git"],
            }
        );
    }

    #[test]
    fn effect_opaque_safe_command_plans_required_recovery_without_plugins() {
        let context = context(Mode::Protect, SnapshotPolicy::Selective);
        let outcome = super::plan_with_context(
            &context,
            super::PlanningRequest {
                command: "sh ./cleanup.sh",
                cwd_state: CwdState::Resolved(std::path::PathBuf::from(".")),
                transport: ExecutionTransport::Shell,
                ci_detected: false,
            },
        );

        let PlanningOutcome::Planned(plan) = outcome else {
            panic!("effect-opaque command must produce a normal plan");
        };
        assert_eq!(
            plan.snapshot_plan(),
            SnapshotPlan::Required {
                applicable_plugins: Vec::new(),
            }
        );
    }
}
