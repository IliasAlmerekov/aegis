use std::path::Path;

use crate::decision::{
    ExecutionTransport, PolicyAllowlistResult, PolicyCiState, PolicyConfigFlags,
    PolicyExecutionContext, PolicyInput, evaluate_policy,
};
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
    let assessment = context.assess(request.command);
    let allowlist_match = match &request.cwd_state {
        CwdState::Resolved(path) => {
            context.allowlist_match_for_command(request.command, Some(path.as_path()))
        }
        CwdState::Unavailable => context.allowlist_match_for_command(request.command, None),
    };
    let applicable_snapshot_plugins = if assessment.risk == crate::interceptor::RiskLevel::Danger
        && context.config().snapshot_policy != crate::config::SnapshotPolicy::None
    {
        match &request.cwd_state {
            CwdState::Resolved(path) => context.applicable_snapshot_plugins(path),
            CwdState::Unavailable => context.applicable_snapshot_plugins(Path::new(".")),
        }
    } else {
        Vec::new()
    };

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
        config_flags: PolicyConfigFlags {
            ci_policy: context.config().ci_policy,
            allowlist_override_level: context.config().strict_allowlist_override,
            snapshot_policy: context.config().snapshot_policy,
        },
        execution_context: PolicyExecutionContext {
            transport: decision_context.transport(),
            applicable_snapshot_plugins: decision_context.applicable_snapshot_plugins(),
        },
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
    use crate::config::{Config, Mode, SnapshotPolicy};
    use crate::decision::ExecutionTransport;
    use crate::planning::types::{
        ApprovalRequirement, ExecutionDisposition, PlanningOutcome, SnapshotPlan,
    };
    use crate::runtime::RuntimeContext;
    use tempfile::TempDir;
    use tokio::runtime::Handle;

    fn test_handle() -> Handle {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = rt.handle().clone();
        std::mem::forget(rt);
        handle
    }

    static CURRENT_DIR_TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn context(mode: Mode, snapshot_policy: SnapshotPolicy) -> RuntimeContext {
        let mut config = Config::default();
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

        let mut config = Config::default();
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

        let mut config = Config::default();
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

        let mut config = Config::default();
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

        let mut config = Config::default();
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
}
