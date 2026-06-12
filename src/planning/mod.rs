//! Orchestration layer that wraps the pure policy engine.

pub mod core;
pub mod policy_rules;
pub mod prepare;
pub mod types;

pub use policy_rules::evaluate_policy_rules;

pub use core::{PlanningRequest, plan_with_context};
pub use prepare::{
    PreparedPlanner, prepare_and_plan, prepare_planner, setup_failure_from_runtime_error,
};
pub use types::{
    ApprovalRequirement, AuditFacts, CwdState, DecisionContext, ExecutionDisposition,
    FailClosedAction, InterceptionPlan, PlanningOutcome, SetupFailureKind, SetupFailurePlan,
    SnapshotPlan,
};
