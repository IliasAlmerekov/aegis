# Typed Decision Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce a canonical typed planning boundary for shell-wrapper execution, watch mode, and `--output json` without changing exit codes, audit schema, JSON schema, snapshot ordering, or fail-closed behavior.

**Architecture:** Add a new `src/planning/` module with two layers: a narrow typed preparation wrapper that turns fail-closed setup errors into `SetupFailurePlan`, and a pure planner core that turns an assessed command plus prepared services into `InterceptionPlan`. Then migrate `main.rs`, `policy_output.rs`, and `watch.rs` to consume `PlanningOutcome` instead of assembling decision semantics themselves.

**Tech Stack:** Rust 2024, existing `RuntimeContext` dependency container, `src/decision.rs` as the policy kernel, synchronous parser/scanner in `src/interceptor/`, integration tests in `tests/full_pipeline.rs` and `tests/watch_mode.rs`

---

## File Structure

- `src/planning/mod.rs`
  - Public planning API: `prepare_planner`, `prepare_and_plan`, `plan_with_context`, and type re-exports.
- `src/planning/types.rs`
  - Own `PlanningOutcome`, `InterceptionPlan`, `SetupFailurePlan`, `DecisionContext`, `AuditFacts`, and invariant-preserving constructors.
- `src/planning/core.rs`
  - Own pure planning logic that builds `DecisionContext`, calls `evaluate_policy`, and derives approval/snapshot/execution facts.
- `src/planning/prepare.rs`
  - Own `PreparedPlanner` and the mapping from fail-closed runtime setup errors to `SetupFailurePlan`.
- `src/lib.rs`
  - Export `pub mod planning;`.
- `src/runtime.rs`
  - Remain a dependency container; add only the smallest read-only helpers needed by planning.
- `src/main.rs`
  - Stop assembling `PolicyInput`; adapt `PlanningOutcome` to shell-wrapper and `--output json`.
- `src/policy_output.rs`
  - Render the existing JSON contract from `InterceptionPlan`.
- `src/watch.rs`
  - Stop assembling `PolicyInput`; consume `PreparedPlanner` / `PlanningOutcome` and keep only watch-specific execution, frame emission, and audit projection.
- `tests/full_pipeline.rs`
  - Own shell-wrapper and JSON regression / setup-failure equivalence coverage.
- `tests/watch_mode.rs`
  - Own watch-mode regression / setup-failure equivalence coverage.
- `docs/architecture-decisions.md`
  - Record the new canonical planning boundary once implementation is complete.

## Milestones

1. Add the typed planning module and pure planning core.
2. Add the reusable preparation wrapper that maps fail-closed setup errors into typed results.
3. Migrate shell-wrapper and JSON evaluation to the planner.
4. Migrate watch mode to the planner without unifying watch execution internals.
5. Lock in equivalence with regression tests and a small architecture note.

## Task Graph

- Task 1 (`src/planning/types.rs`, `src/planning/core.rs`) must land before everything else.
- Task 2 (`src/planning/prepare.rs`, `src/runtime.rs`) depends on Task 1.
- Task 3 (`src/main.rs`, `src/policy_output.rs`) depends on Tasks 1–2.
- Task 4 (`src/watch.rs`) depends on Tasks 1–2 and should land after Task 3 so shell/json behavior is already stable.
- Task 5 (tests + architecture note) depends on Tasks 1–4.

## Task Details

### Task 1: Add the typed planning module and pure planning core

**Files:**
- Create: `src/planning/mod.rs`
- Create: `src/planning/types.rs`
- Create: `src/planning/core.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing planner unit tests first**

Create `src/planning/core.rs` with only the tests and a minimal module shell first:

```rust
use crate::config::{AllowlistOverrideLevel, CiPolicy, Config, Mode, SnapshotPolicy};
use crate::decision::ExecutionTransport;
use crate::interceptor::RiskLevel;
use crate::interceptor::parser::Parser as CommandParser;
use crate::interceptor::scanner::Assessment;
use crate::planning::types::{
    ApprovalRequirement, CwdState, DecisionContext, ExecutionDisposition,
    InterceptionPlan, PlanningOutcome, SnapshotPlan,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::RuntimeContext;
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
}
```

- [ ] **Step 2: Run the planner tests to verify RED**

Run:

```bash
rtk cargo test planning::core::tests --lib
```

Expected: compile errors because `src/planning/` and its public types do not exist yet.

- [ ] **Step 3: Add the planning types and invariant-preserving constructors**

Create `src/planning/types.rs`:

```rust
use std::path::PathBuf;

use crate::audit::{AuditSnapshot, Decision, MatchedPattern};
use crate::config::{AllowlistMatch, Mode};
use crate::decision::{BlockReason, ExecutionTransport, PolicyAction, PolicyDecision};
use crate::interceptor::RiskLevel;
use crate::interceptor::scanner::Assessment;

#[derive(Debug, Clone)]
pub enum PlanningOutcome {
    Planned(InterceptionPlan),
    SetupFailure(SetupFailurePlan),
}

#[derive(Debug, Clone)]
pub struct InterceptionPlan {
    assessment: Assessment,
    decision_context: DecisionContext,
    policy_decision: PolicyDecision,
    approval_requirement: ApprovalRequirement,
    snapshot_plan: SnapshotPlan,
    execution_disposition: ExecutionDisposition,
    audit_facts: AuditFacts,
}

impl InterceptionPlan {
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
        let audit_facts = AuditFacts::from_plan_inputs(
            &assessment,
            &decision_context,
            policy_decision.block_reason(),
            policy_decision.allowlist_effective,
        );

        Self {
            assessment,
            decision_context,
            policy_decision,
            approval_requirement,
            snapshot_plan,
            execution_disposition,
            audit_facts,
        }
    }

    pub fn assessment(&self) -> &Assessment { &self.assessment }
    pub fn decision_context(&self) -> &DecisionContext { &self.decision_context }
    pub fn policy_decision(&self) -> PolicyDecision { self.policy_decision }
    pub fn approval_requirement(&self) -> ApprovalRequirement { self.approval_requirement }
    pub fn snapshot_plan(&self) -> SnapshotPlan { self.snapshot_plan.clone() }
    pub fn execution_disposition(&self) -> ExecutionDisposition { self.execution_disposition }
    pub fn audit_facts(&self) -> &AuditFacts { &self.audit_facts }
}

#[derive(Debug, Clone)]
pub struct SetupFailurePlan {
    kind: SetupFailureKind,
    fail_closed_action: FailClosedAction,
    user_message: String,
    audit_facts: Option<AuditFacts>,
}

impl SetupFailurePlan {
    pub(crate) fn new(
        kind: SetupFailureKind,
        fail_closed_action: FailClosedAction,
        user_message: String,
        audit_facts: Option<AuditFacts>,
    ) -> Self {
        Self { kind, fail_closed_action, user_message, audit_facts }
    }

    pub fn kind(&self) -> SetupFailureKind { self.kind }
    pub fn fail_closed_action(&self) -> FailClosedAction { self.fail_closed_action }
    pub fn user_message(&self) -> &str { &self.user_message }
    pub fn audit_facts(&self) -> Option<&AuditFacts> { self.audit_facts.as_ref() }
}

#[derive(Debug, Clone)]
pub struct DecisionContext {
    pub mode: Mode,
    pub transport: ExecutionTransport,
    pub ci_detected: bool,
    pub cwd_state: CwdState,
    pub allowlist_match: Option<AllowlistMatch>,
    pub applicable_snapshot_plugins: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CwdState {
    Resolved(PathBuf),
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalRequirement {
    None,
    HumanConfirmationRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotPlan {
    NotRequired,
    Required { applicable_plugins: Vec<&'static str> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionDisposition {
    Execute,
    RequiresApproval,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailClosedAction {
    Deny,
    Block,
    InternalError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupFailureKind {
    InvalidConfig,
    ScannerUnavailable,
    CwdUnavailableForPolicy,
    AllowlistContextAmbiguous,
    OtherFailClosed,
}

#[derive(Debug, Clone)]
pub struct AuditFacts {
    pub command: String,
    pub risk: RiskLevel,
    pub matched_patterns: Vec<MatchedPattern>,
    pub mode: Mode,
    pub ci_detected: bool,
    pub allowlist_matched: bool,
    pub allowlist_effective: bool,
    pub transport: ExecutionTransport,
    pub block_reason: Option<BlockReason>,
}

impl AuditFacts {
    fn from_plan_inputs(
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
}
```

Create `src/planning/mod.rs`:

```rust
pub mod core;
pub mod types;
pub mod prepare;

pub use core::{PlanningRequest, plan_with_context};
pub use prepare::{PreparedPlanner, prepare_and_plan, prepare_planner};
pub use types::{
    ApprovalRequirement, AuditFacts, CwdState, DecisionContext, ExecutionDisposition,
    FailClosedAction, InterceptionPlan, PlanningOutcome, SetupFailureKind, SetupFailurePlan,
    SnapshotPlan,
};
```

Update `src/lib.rs`:

```rust
pub mod planning;
```

- [ ] **Step 4: Implement the pure planning core**

Replace the stub in `src/planning/core.rs` with:

```rust
use std::path::PathBuf;

use crate::decision::{
    ExecutionTransport, PolicyAllowlistResult, PolicyCiState, PolicyConfigFlags,
    PolicyExecutionContext, PolicyInput, evaluate_policy,
};
use crate::planning::types::{CwdState, DecisionContext, InterceptionPlan, PlanningOutcome};
use crate::runtime::RuntimeContext;

#[derive(Debug, Clone)]
pub struct PlanningRequest<'a> {
    pub command: &'a str,
    pub cwd_state: CwdState,
    pub transport: ExecutionTransport,
    pub ci_detected: bool,
}

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
    let applicable_snapshot_plugins = match &request.cwd_state {
        CwdState::Resolved(path) => context.applicable_snapshot_plugins(path),
        CwdState::Unavailable => Vec::new(),
    };

    let decision_context = DecisionContext {
        mode: context.config().mode,
        transport: request.transport,
        ci_detected: request.ci_detected,
        cwd_state: request.cwd_state,
        allowlist_match,
        applicable_snapshot_plugins,
    };

    let policy_decision = evaluate_policy(PolicyInput {
        assessment: &assessment,
        mode: decision_context.mode,
        ci_state: PolicyCiState {
            detected: decision_context.ci_detected,
        },
        allowlist: PolicyAllowlistResult {
            matched: decision_context.allowlist_match.is_some(),
        },
        config_flags: PolicyConfigFlags {
            ci_policy: context.config().ci_policy,
            allowlist_override_level: context.config().strict_allowlist_override,
            snapshot_policy: context.config().snapshot_policy,
        },
        execution_context: PolicyExecutionContext {
            transport: decision_context.transport,
            applicable_snapshot_plugins: decision_context
                .applicable_snapshot_plugins
                .as_slice(),
        },
    });

    PlanningOutcome::Planned(InterceptionPlan::from_policy(
        assessment,
        decision_context,
        policy_decision,
    ))
}
```

- [ ] **Step 5: Re-run the planner tests to verify GREEN**

Run:

```bash
rtk cargo test planning::core::tests --lib
```

Expected: the new planner tests pass.

- [ ] **Step 6: Commit**

```bash
rtk git add src/planning/mod.rs src/planning/types.rs src/planning/core.rs src/lib.rs
rtk git commit -m "feat: add typed planning core"
```

### Task 2: Add the typed preparation wrapper and setup-failure mapping

**Files:**
- Create: `src/planning/prepare.rs`
- Modify: `src/runtime.rs`

- [ ] **Step 1: Add failing wrapper tests for typed setup-failure reuse**

Create `src/planning/prepare.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AegisError;
    use time::OffsetDateTime;
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

    #[test]
    fn config_errors_become_setup_failure_plans() {
        let plan = super::setup_failure_from_runtime_error(
            &AegisError::Config("bad config".to_string()),
            "echo hi",
            crate::decision::ExecutionTransport::Shell,
        );

        assert_eq!(plan.kind(), crate::planning::SetupFailureKind::InvalidConfig);
        assert_eq!(
            plan.fail_closed_action(),
            crate::planning::FailClosedAction::InternalError
        );
        assert!(plan.user_message().contains("failed to load config"));
    }

    #[test]
    fn prepared_setup_failure_replays_same_planning_outcome_for_every_request() {
        let prepared = PreparedPlanner::SetupFailure(super::setup_failure_from_runtime_error(
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

        assert!(matches!(first, crate::planning::PlanningOutcome::SetupFailure(_)));
        assert!(matches!(second, crate::planning::PlanningOutcome::SetupFailure(_)));
    }
}
```

- [ ] **Step 2: Run the wrapper tests to verify RED**

Run:

```bash
rtk cargo test planning::prepare::tests --lib
```

Expected: compile errors because the preparation wrapper does not exist yet.

- [ ] **Step 3: Implement `PreparedPlanner`, error mapping, and the wrapper API**

Create `src/planning/prepare.rs`:

```rust
use tokio::runtime::Handle;

use crate::decision::ExecutionTransport;
use crate::error::AegisError;
use crate::planning::core::{PlanningRequest, plan_with_context};
use crate::planning::types::{
    AuditFacts, FailClosedAction, PlanningOutcome, SetupFailureKind, SetupFailurePlan,
};
use crate::runtime::RuntimeContext;

#[derive(Debug)]
pub enum PreparedPlanner {
    Ready(RuntimeContext),
    SetupFailure(SetupFailurePlan),
}

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

pub fn prepare_and_plan(
    prepared: &PreparedPlanner,
    request: PlanningRequest<'_>,
) -> PlanningOutcome {
    match prepared {
        PreparedPlanner::Ready(context) => plan_with_context(context, request),
        PreparedPlanner::SetupFailure(plan) => PlanningOutcome::SetupFailure(plan.clone()),
    }
}

pub fn setup_failure_from_runtime_error(
    err: &AegisError,
    command: &str,
    transport: ExecutionTransport,
) -> SetupFailurePlan {
    let kind = match err {
        AegisError::Config(_) => SetupFailureKind::InvalidConfig,
        _ => SetupFailureKind::OtherFailClosed,
    };
    let user_message = format!("error: failed to load config: {err}");
    let audit_facts = (!command.is_empty()).then(|| AuditFacts {
        command: command.to_string(),
        risk: crate::interceptor::RiskLevel::Warn,
        matched_patterns: Vec::new(),
        mode: crate::config::Mode::Protect,
        ci_detected: false,
        allowlist_matched: false,
        allowlist_effective: false,
        transport,
        block_reason: None,
    });

    SetupFailurePlan::new(
        kind,
        FailClosedAction::InternalError,
        user_message,
        audit_facts,
    )
}
```

In `src/runtime.rs`, derive `Debug` for `RuntimeContext` if needed by `PreparedPlanner`, or add the narrow getter methods required by the planner without moving policy logic into runtime.

- [ ] **Step 4: Re-run the wrapper tests to verify GREEN**

Run:

```bash
rtk cargo test planning::prepare::tests --lib
```

Expected: wrapper tests pass, and the planner can replay a typed `SetupFailurePlan`.

- [ ] **Step 5: Commit**

```bash
rtk git add src/planning/prepare.rs src/runtime.rs
rtk git commit -m "feat: add typed planning preparation wrapper"
```

### Task 3: Migrate shell-wrapper and `--output json` to the planner

**Files:**
- Modify: `src/main.rs`
- Modify: `src/policy_output.rs`

- [ ] **Step 1: Add failing shell/json regression tests for planner-backed orchestration**

In `tests/full_pipeline.rs`, add these tests near the existing JSON coverage:

```rust
#[test]
fn invalid_project_config_in_json_mode_preserves_stderr_contract() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    fs::write(
        workspace.path().join(".aegis.toml"),
        "mode = <<<THIS IS NOT VALID TOML\n",
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", "echo hi", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty(), "setup failure must keep current stderr-only contract");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: failed to load config"));
}

#[test]
fn json_mode_still_does_not_write_audit_entries_when_planned() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "safe-command --flag", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(
        !home.path().join(".aegis").join("audit.jsonl").exists(),
        "json mode must stay evaluation-only after planner migration"
    );
}
```

- [ ] **Step 2: Run the focused shell/json tests to verify RED**

Run:

```bash
rtk cargo test --test full_pipeline json_output_safe_command_returns_single_evaluation_object_without_exec_or_audit
rtk cargo test --test full_pipeline json_output_danger_command_returns_prompt_decision_without_stderr_or_audit
rtk cargo test --test full_pipeline invalid_project_config_in_json_mode_preserves_stderr_contract
rtk cargo test --test full_pipeline safe_command_passthroughs_stdout_and_exit_code
```

Expected: at least one test fails or compilation fails because `main.rs` still assembles policy directly.

- [ ] **Step 3: Replace direct policy assembly in `main.rs` with planner calls**

Refactor `src/main.rs` so the shell-wrapper path uses `PreparedPlanner` and `PlanningOutcome`:

```rust
use aegis::planning::{
    ApprovalRequirement, CwdState, ExecutionDisposition, PlanningOutcome, PreparedPlanner,
    SnapshotPlan, prepare_and_plan, prepare_planner,
};
```

Replace the current `run_shell_wrapper(...)` setup with:

```rust
fn run_shell_wrapper(
    cmd: &str,
    output: CommandOutputFormat,
    verbosity: OutputVerbosity,
    handle: Handle,
) -> i32 {
    let prepared = prepare_planner(verbosity.is_verbose(), handle);
    let in_ci = is_ci_environment();
    let cwd_state = match env::current_dir() {
        Ok(path) => CwdState::Resolved(path),
        Err(_) => CwdState::Unavailable,
    };
    let outcome = prepare_and_plan(
        &prepared,
        aegis::planning::PlanningRequest {
            command: cmd,
            cwd_state,
            transport: ExecutionTransport::Shell,
            ci_detected: in_ci,
        },
    );

    match output {
        CommandOutputFormat::Json => return render_json_outcome(&outcome),
        CommandOutputFormat::Text => {}
    }

    run_shell_text_outcome(cmd, verbosity, &prepared, outcome)
}
```

Then add a small adapter helper in `src/main.rs`:

```rust
fn run_shell_text_outcome(
    cmd: &str,
    verbosity: OutputVerbosity,
    prepared: &PreparedPlanner,
    outcome: PlanningOutcome,
) -> i32 {
    match outcome {
        PlanningOutcome::SetupFailure(plan) => {
            eprintln!("{}", plan.user_message());
            EXIT_INTERNAL
        }
        PlanningOutcome::Planned(plan) => {
            let snapshots = match plan.execution_disposition() {
                ExecutionDisposition::RequiresApproval => {
                    let approved = show_confirmation(plan.assessment(), &[]);
                    if !approved {
                        append_shell_audit(prepared, &plan, Decision::Denied, &[]);
                        return EXIT_DENIED;
                    }
                    create_snapshots_for_plan(prepared, &plan, verbosity.is_verbose())
                }
                ExecutionDisposition::Block => {
                    show_block_for_plan(&plan);
                    append_shell_audit(prepared, &plan, Decision::Blocked, &[]);
                    return EXIT_BLOCKED;
                }
                ExecutionDisposition::Execute => create_snapshots_for_plan(
                    prepared,
                    &plan,
                    verbosity.is_verbose(),
                ),
            };

            append_shell_audit(prepared, &plan, Decision::AutoApproved, &snapshots);
            exec_command(cmd)
        }
    }
}
```

Keep the final shell semantics identical to current behavior; if the helper above needs two branches so `Approved` and `AutoApproved` are logged distinctly, split it exactly that way rather than compressing behavior.

- [ ] **Step 4: Render the existing JSON contract from `InterceptionPlan`**

In `src/policy_output.rs`, replace the old `render(...)` entry point with a plan-driven one:

```rust
use aegis::planning::{InterceptionPlan, PlanningOutcome};

pub(crate) fn render_planned(plan: &InterceptionPlan) -> Result<String, serde_json::Error> {
    let decision = plan.policy_decision();
    let assessment = plan.assessment();
    let decision_context = plan.decision_context();

    let output = PolicyEvaluationOutput {
        schema_version: 1,
        command: assessment.command.raw.clone(),
        risk: assessment.risk.to_string(),
        decision: decision_string(decision.decision).to_string(),
        exit_code: exit_code_for(decision.decision),
        mode: mode_string(decision_context.mode).to_string(),
        ci_state: CiState {
            detected: decision_context.ci_detected,
            policy: ci_policy_string(plan.policy_decision().rationale.block_reason()
                .map(|_| aegis::config::CiPolicy::Block)
                .unwrap_or(aegis::config::CiPolicy::Allow))
            .to_string(),
        },
        matched_patterns: assessment
            .matched
            .iter()
            .map(|matched| MatchedPatternOutput {
                id: matched.pattern.id.to_string(),
                category: category_string(matched.pattern.category).to_string(),
                risk: matched.pattern.risk.to_string(),
                matched_text: matched.matched_text.clone(),
                description: matched.pattern.description.to_string(),
                safe_alternative: matched.pattern.safe_alt.as_ref().map(ToString::to_string),
                source: pattern_source_string(matched.pattern.source).to_string(),
            })
            .collect(),
        allowlist_match: AllowlistMatchOutput {
            matched: decision_context.allowlist_match.is_some(),
            effective: plan.audit_facts().allowlist_effective,
            pattern: decision_context.allowlist_match.as_ref().map(|m| m.pattern.clone()),
            reason: decision_context.allowlist_match.as_ref().map(|m| m.reason.clone()),
        },
        snapshots_created: Vec::new(),
        snapshot_plan: SnapshotPlanOutput {
            requested: matches!(plan.snapshot_plan(), aegis::planning::SnapshotPlan::Required { .. }),
            applicable_plugins: match plan.snapshot_plan() {
                aegis::planning::SnapshotPlan::NotRequired => Vec::new(),
                aegis::planning::SnapshotPlan::Required { applicable_plugins } => {
                    applicable_plugins.into_iter().map(str::to_string).collect()
                }
            },
        },
        execution: ExecutionOutput {
            mode: "evaluation_only",
            will_execute: false,
        },
        block_reason: plan
            .policy_decision()
            .block_reason()
            .map(|reason| block_reason_string(reason).to_string()),
        decision_source: decision_source_string(assessment.decision_source()).to_string(),
    };

    serde_json::to_string_pretty(&output)
}
```

If computing `ci_state.policy` from the plan is awkward, pass the `ci_policy` value into `AuditFacts` / `DecisionContext` instead of trying to reconstruct it later.

- [ ] **Step 5: Re-run the focused shell/json tests to verify GREEN**

Run:

```bash
rtk cargo test --test full_pipeline json_output_safe_command_returns_single_evaluation_object_without_exec_or_audit
rtk cargo test --test full_pipeline json_output_danger_command_returns_prompt_decision_without_stderr_or_audit
rtk cargo test --test full_pipeline invalid_project_config_in_json_mode_preserves_stderr_contract
rtk cargo test --test full_pipeline safe_command_passthroughs_stdout_and_exit_code
```

Expected: all four tests pass with unchanged externally visible behavior.

- [ ] **Step 6: Commit**

```bash
rtk git add src/main.rs src/policy_output.rs tests/full_pipeline.rs
rtk git commit -m "refactor: drive shell and json decisions from planner"
```

### Task 4: Migrate watch mode to the planner without changing watch execution internals

**Files:**
- Modify: `src/main.rs`
- Modify: `src/watch.rs`

- [ ] **Step 1: Add failing watch-mode regression tests for planner-backed setup failures**

In `tests/watch_mode.rs`, add:

```rust
#[test]
fn invalid_project_config_in_watch_still_fails_before_emitting_frames() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    fs::write(
        workspace.path().join(".aegis.toml"),
        "mode = <<<THIS IS NOT VALID TOML\n",
    )
    .unwrap();

    let output = aegis_watch_in(home.path(), workspace.path(), b"{\"cmd\":\"echo hi\"}\n");

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: failed to load config"));
}

#[test]
fn watch_invalid_cwd_keeps_current_error_frame_contract_after_planner_migration() {
    let output = aegis_watch(b"{\"cmd\":\"echo x\",\"cwd\":\"/nonexistent/path/xyz\"}\n");
    let frames = parse_frames(&output.stdout);
    let error = frames.iter().find(|f| f["type"] == "error").unwrap();
    assert_eq!(error["exit_code"], 4);
    assert_eq!(error["message"], "invalid cwd");
}
```

- [ ] **Step 2: Run the focused watch tests to verify RED**

Run:

```bash
rtk cargo test --test watch_mode safe_command_emits_result_approved
rtk cargo test --test watch_mode invalid_project_config_in_watch_still_fails_before_emitting_frames
rtk cargo test --test watch_mode watch_invalid_cwd_keeps_current_error_frame_contract_after_planner_migration
```

Expected: compile failures or behavior failures because watch still assembles policy directly.

- [ ] **Step 3: Replace direct policy assembly in `watch.rs` with planner calls**

In `src/main.rs`, change the watch path from loading `RuntimeContext` directly to preparing a planner once:

```rust
Some(Commands::Watch) => {
    let prepared = aegis::planning::prepare_planner(verbosity.is_verbose(), handle);
    rt.block_on(aegis::watch::run(&prepared))
}
```

Then in `src/watch.rs`, remove direct `evaluate_policy(...)` assembly and consume `PlanningOutcome`:

```rust
use crate::planning::{
    CwdState, ExecutionDisposition, PlanningOutcome, PreparedPlanner, SnapshotPlan,
    prepare_and_plan,
};

pub async fn run(prepared: &PreparedPlanner) -> i32 {
    let mut reader = TokioBufReader::new(tokio::io::stdin());

    loop {
        match read_bounded_line(&mut reader, MAX_FRAME_BYTES).await {
            // existing EOF / oversized / parse handling stays as-is
            Ok(ReadLineResult::Line(line)) => {
                if line.trim().is_empty() {
                    continue;
                }
                process_frame(line, prepared).await;
            }
            // keep the rest of the branches unchanged
        }
    }
}

async fn process_frame(line: String, prepared: &PreparedPlanner) {
    let frame: InputFrame = match serde_json::from_str(&line) {
        Ok(f) => f,
        Err(e) => {
            emit_frame(&OutputFrame::Error {
                id: None,
                exit_code: 4,
                message: format!("invalid JSON: {e}"),
            })
            .unwrap_or_else(|_| std::process::exit(4));
            return;
        }
    };

    if frame.cmd.trim().is_empty() {
        emit_frame(&OutputFrame::Error {
            id: frame.id.clone(),
            exit_code: 4,
            message: "missing or empty cmd".to_string(),
        })
        .unwrap_or_else(|_| std::process::exit(4));
        return;
    }

    let cwd_state = match frame.cwd.as_ref() {
        Some(cwd) => {
            let path = PathBuf::from(cwd);
            if !path.is_dir() {
                emit_frame(&OutputFrame::Error {
                    id: frame.id.clone(),
                    exit_code: 4,
                    message: "invalid cwd".to_string(),
                })
                .unwrap_or_else(|_| std::process::exit(4));
                return;
            }
            CwdState::Resolved(path)
        }
        None => match std::env::current_dir() {
            Ok(path) => CwdState::Resolved(path),
            Err(_) => CwdState::Unavailable,
        },
    };

    let outcome = prepare_and_plan(
        prepared,
        crate::planning::PlanningRequest {
            command: &frame.cmd,
            cwd_state,
            transport: crate::decision::ExecutionTransport::Watch,
            ci_detected: false,
        },
    );

    match outcome {
        PlanningOutcome::SetupFailure(plan) => {
            eprintln!("{}", plan.user_message());
            std::process::exit(4);
        }
        PlanningOutcome::Planned(plan) => {
            run_watch_plan(frame, plan).await;
        }
    }
}
```

Keep `execute_and_emit(...)` as the watch-specific executor; only remove duplicated decision assembly.

- [ ] **Step 4: Re-run the focused watch tests to verify GREEN**

Run:

```bash
rtk cargo test --test watch_mode safe_command_emits_result_approved
rtk cargo test --test watch_mode invalid_project_config_in_watch_still_fails_before_emitting_frames
rtk cargo test --test watch_mode watch_invalid_cwd_keeps_current_error_frame_contract_after_planner_migration
rtk cargo test --test watch_mode watch_mode_audit_entry_sets_transport_watch
```

Expected: watch continues to preserve existing contracts while using the shared planner.

- [ ] **Step 5: Commit**

```bash
rtk git add src/main.rs src/watch.rs tests/watch_mode.rs
rtk git commit -m "refactor: drive watch decisions from planner"
```

### Task 5: Lock in equivalence with regression tests and an architecture note

**Files:**
- Modify: `tests/full_pipeline.rs`
- Modify: `tests/watch_mode.rs`
- Modify: `docs/architecture-decisions.md`

- [ ] **Step 1: Add final regression tests for planner-specific setup-failure and JSON/audit invariants**

In `tests/full_pipeline.rs`, add:

```rust
#[test]
fn planner_migration_keeps_json_block_reason_contract() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "rm -rf /", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["decision"], "block");
    assert_eq!(json["block_reason"], "intrinsic_risk_block");
}

#[test]
fn planner_migration_keeps_shell_audit_projection_fields() {
    let home = TempDir::new().unwrap();

    let output = base_command(home.path())
        .args(["-c", "printf hello"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let entries = read_audit_entries(home.path());
    assert_eq!(entries[0]["mode"], "Protect");
    assert_eq!(entries[0]["ci_detected"], serde_json::json!(false));
    assert_eq!(entries[0]["allowlist_matched"], serde_json::json!(false));
    assert_eq!(entries[0]["allowlist_effective"], serde_json::json!(false));
}
```

In `docs/architecture-decisions.md`, add a short paragraph under the interception flow section:

```md
Phase 1 of the typed decision pipeline introduces a canonical planning boundary:
`prepare_planner` maps fail-closed setup errors into typed setup-failure plans,
and `plan_with_context` produces a typed `InterceptionPlan` for shell-wrapper,
watch mode, and evaluation-only JSON. UI, snapshots, execution, and audit
append remain downstream adapters and do not own policy semantics.
```

- [ ] **Step 2: Run the focused equivalence tests to verify RED**

Run:

```bash
rtk cargo test --test full_pipeline planner_migration_keeps_json_block_reason_contract
rtk cargo test --test full_pipeline planner_migration_keeps_shell_audit_projection_fields
rtk cargo test --test watch_mode malformed_project_config_aborts_watch_startup_with_clear_error
```

Expected: at least one new assertion fails before the final equivalence pass is complete.

- [ ] **Step 3: Tighten any remaining adapters until the new regressions pass**

Apply the smallest fixes needed so that:

- JSON rendering reads only from the plan for planned paths
- shell audit projection reads from `AuditFacts + runtime outcome`
- watch setup failures continue to preserve the current stderr/no-frame startup contract

If you need a small helper for audit projection, add one in `src/main.rs` / `src/watch.rs` first rather than refactoring `src/audit/logger.rs`.

- [ ] **Step 4: Re-run the focused equivalence tests to verify GREEN**

Run:

```bash
rtk cargo test --test full_pipeline planner_migration_keeps_json_block_reason_contract
rtk cargo test --test full_pipeline planner_migration_keeps_shell_audit_projection_fields
rtk cargo test --test watch_mode malformed_project_config_aborts_watch_startup_with_clear_error
```

Expected: all three tests pass.

- [ ] **Step 5: Commit**

```bash
rtk git add tests/full_pipeline.rs tests/watch_mode.rs docs/architecture-decisions.md
rtk git commit -m "test: lock in typed planning equivalence"
```

---

## Verification Plan

After all tasks complete, run:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
```

If parser/scanner files remain untouched, the scanner benchmark is optional for this phase. If any implementation detail spills into `src/interceptor/`, add:

```bash
rtk cargo bench --bench scanner_bench
```

If config/runtime loading changes pull in dependency or security-adjacent fallout, also run:

```bash
rtk cargo audit
rtk cargo deny check
```

---

## Rollback Plan

If the planner migration introduces regressions:

1. revert `src/main.rs`, `src/policy_output.rs`, and `src/watch.rs` to the last green commit
2. keep `src/planning/` on a branch until equivalence failures are understood
3. preserve the new regression tests so they continue to document the intended behavior

If setup-failure projection proves harder than expected:

1. keep the pure planning core (`src/planning/core.rs`, `src/planning/types.rs`)
2. temporarily gate `PreparedPlanner` usage behind shell-wrapper only
3. do **not** merge partial watch-specific semantics drift

Do **not** “fix” rollback pressure by:

- weakening `Block`
- suppressing config/setup failures into silent allow
- changing audit schema
- changing `--output json`

---

## Self-Review

### Spec coverage

- Canonical planning boundary: Tasks 1–2
- Typed preparation wrapper / `SetupFailurePlan`: Task 2
- Shell-wrapper and JSON on one planner: Task 3
- Watch on the same planner with separate execution adapter: Task 4
- JSON golden / equivalence checks and setup-failure coverage: Task 5
- No external contract changes: Tasks 3–5 assertions

### Placeholder scan

- No `TODO`, `TBD`, or “similar to Task N” placeholders remain.
- Every code-changing step includes concrete code or an exact code shape.
- Every verification step has an exact `rtk` command.

### Type consistency

- `PlanningOutcome`, `InterceptionPlan`, `SetupFailurePlan`, `PreparedPlanner`, and `PlanningRequest` are named consistently across all tasks.
- `ExecutionDisposition` uses the approved phase-1 enum: `Execute | RequiresApproval | Block`.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-04-12-typed-decision-pipeline.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**
