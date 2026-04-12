use tokio::runtime::Handle;

use crate::decision::ExecutionTransport;
use crate::error::AegisError;
use crate::planning::core::{PlanningRequest, plan_with_context};
use crate::planning::types::{
    FailClosedAction, PlanningOutcome, SetupFailureKind, SetupFailurePlan,
};
use crate::runtime::RuntimeContext;

/// Prepared planning dependency state shared across multiple planning requests.
pub enum PreparedPlanner {
    /// Runtime preparation succeeded and planning can proceed normally.
    Ready(RuntimeContext),
    /// Runtime preparation failed and every request must fail closed the same way.
    SetupFailure(SetupFailurePlan),
}

/// Prepare planner dependencies once and return a typed ready/fail-closed wrapper.
pub fn prepare_planner(verbose: bool, handle: Handle) -> PreparedPlanner {
    match RuntimeContext::load(verbose, handle) {
        Ok(context) => PreparedPlanner::Ready(context),
        Err(err) => PreparedPlanner::SetupFailure(setup_failure_from_runtime_error(
            &err,
            "",
            ExecutionTransport::Shell,
        )),
    }
}

/// Consume one planning request using an already prepared planner state.
pub fn prepare_and_plan(
    prepared: &PreparedPlanner,
    request: PlanningRequest<'_>,
) -> PlanningOutcome {
    match prepared {
        PreparedPlanner::Ready(context) => plan_with_context(context, request),
        PreparedPlanner::SetupFailure(plan) => {
            let _ = request;
            PlanningOutcome::SetupFailure(plan.clone())
        }
    }
}

/// Map a runtime setup failure into a typed fail-closed setup plan.
pub fn setup_failure_from_runtime_error(
    err: &AegisError,
    command: &str,
    transport: ExecutionTransport,
) -> SetupFailurePlan {
    let _ = (command, transport);

    let (kind, user_message) = match err {
        AegisError::Config(_) => (
            SetupFailureKind::InvalidConfig,
            format!("error: failed to load config: {err}"),
        ),
        _ => (
            SetupFailureKind::OtherFailClosed,
            format!("error: failed to initialize runtime: {err}"),
        ),
    };

    SetupFailurePlan::new(kind, FailClosedAction::InternalError, user_message, None)
}

#[cfg(test)]
mod tests {
    use crate::error::AegisError;

    #[test]
    fn config_errors_become_setup_failure_plans() {
        let plan = super::setup_failure_from_runtime_error(
            &AegisError::Config("bad config".to_string()),
            "echo hi",
            crate::decision::ExecutionTransport::Shell,
        );

        assert_eq!(
            plan.kind(),
            crate::planning::SetupFailureKind::InvalidConfig
        );
        assert_eq!(
            plan.fail_closed_action(),
            crate::planning::FailClosedAction::InternalError
        );
        assert!(plan.user_message().contains("failed to load config"));
    }

    #[test]
    fn prepared_setup_failure_replays_same_planning_outcome_for_every_request() {
        let prepared =
            super::PreparedPlanner::SetupFailure(super::setup_failure_from_runtime_error(
                &AegisError::Config("bad config".to_string()),
                "echo hi",
                crate::decision::ExecutionTransport::Shell,
            ));

        let first = super::prepare_and_plan(
            &prepared,
            crate::planning::PlanningRequest {
                command: "echo one",
                cwd_state: crate::planning::CwdState::Resolved(std::path::PathBuf::from(".")),
                transport: crate::decision::ExecutionTransport::Shell,
                ci_detected: false,
            },
        );
        let second = super::prepare_and_plan(
            &prepared,
            crate::planning::PlanningRequest {
                command: "echo two",
                cwd_state: crate::planning::CwdState::Resolved(std::path::PathBuf::from(".")),
                transport: crate::decision::ExecutionTransport::Shell,
                ci_detected: false,
            },
        );

        assert!(matches!(
            first,
            crate::planning::PlanningOutcome::SetupFailure(_)
        ));
        assert!(matches!(
            second,
            crate::planning::PlanningOutcome::SetupFailure(_)
        ));
    }
}
