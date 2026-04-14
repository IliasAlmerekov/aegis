# Structured Explainability and Audit Reasons Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce a thin internal explanation contract that flows from scanner and policy into audit and UI without changing Aegis decision semantics.

**Architecture:** Add a focused `src/explanation.rs` module that normalizes scan facts, policy rationale, execution-context facts, and optional runtime outcome. Build the base explanation during planning, enrich it with runtime outcome at audit time, and migrate audit/UI to consume the same explanation object instead of rebuilding reasons ad hoc.

**Tech Stack:** Rust 2024, existing `serde` derives, Aegis scanner/policy/planning/runtime/audit/UI modules, `rtk cargo test`

---

## File Map

- `src/explanation.rs` (new): internal typed explanation model, builders, and focused unit tests
- `src/lib.rs`: export the new explanation module
- `src/decision.rs`: serde-friendly derives and any small helpers needed to embed canonical policy rationale in the explanation model without duplicating truth
- `src/interceptor/scanner/assessment.rs`: serde-friendly derives for scanner-owned explanation facts such as `DecisionSource`
- `src/config/allowlist.rs`: serde-friendly derives for allowlist provenance used by execution-context explanation fields
- `src/planning/types.rs`: replace `AuditFacts` storage on `InterceptionPlan` with `CommandExplanation`
- `src/audit/logger.rs`: add nested explanation serialization while preserving current top-level audit fields
- `src/runtime.rs`: enrich explanations with runtime outcome before append and pass them into the audit logger
- `src/ui/confirm.rs`: render confirmation/policy-block messaging from the explanation model instead of stitching facts separately
- `src/main.rs`: pass plan explanations into shell-mode UI and audit paths
- `src/watch.rs`: pass plan explanations into watch-mode UI and audit paths
- `tests/full_pipeline.rs` (if needed): end-to-end regression coverage for structured explanation behavior that is easier to express as an integration test than as unit tests

## Rollout Constraints

- Keep explanation **descriptive, not authoritative**
- Do **not** re-derive decisions after policy evaluation
- Keep scanner ownership to scan facts and policy ownership to final rationale
- Preserve existing allowlist precedence, block reasons, snapshot requirements, and CI/mode semantics
- Keep safe-path work proportional to facts the current pipeline already computes
- Preserve audit append-only compatibility by adding structured explanation data without removing existing top-level fields

## Task 1: Add the thin explanation model

**Files:**
- Create: `src/explanation.rs`
- Modify: `src/lib.rs`
- Modify: `src/decision.rs`
- Modify: `src/interceptor/scanner/assessment.rs`
- Modify: `src/config/allowlist.rs`

- [ ] **Step 1: Write the failing model tests**

Add focused tests at the bottom of `src/explanation.rs` that pin the approved ownership boundaries:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AllowlistMatch, AllowlistSourceLayer, Mode};
    use crate::decision::{
        BlockReason, ExecutionTransport, PolicyAction, PolicyDecision, PolicyRationale,
    };
    use crate::interceptor::scanner::{Assessment, DecisionSource};

    #[test]
    fn builds_base_explanation_from_existing_pipeline_facts() {
        let assessment = test_assessment();
        let decision_context = test_decision_context(Some(AllowlistMatch {
            pattern: "cargo test *".to_string(),
            reason: "trusted ci command".to_string(),
            source_layer: AllowlistSourceLayer::Project,
        }));
        let policy = PolicyDecision {
            decision: PolicyAction::Prompt,
            rationale: PolicyRationale::RequiresConfirmation,
            requires_confirmation: true,
            snapshots_required: true,
            allowlist_effective: false,
        };

        let explanation =
            CommandExplanation::from_plan_inputs(&assessment, &decision_context, policy);

        assert_eq!(explanation.scan.decision_source, DecisionSource::BuiltinPattern);
        assert_eq!(explanation.scan.highest_risk, assessment.risk);
        assert_eq!(explanation.policy.rationale, PolicyRationale::RequiresConfirmation);
        assert_eq!(explanation.context.mode, Mode::Protect);
        assert_eq!(explanation.context.transport, ExecutionTransport::Shell);
        assert!(explanation.outcome.is_none());
    }

    #[test]
    fn appends_runtime_outcome_without_rewriting_existing_sections() {
        let explanation = test_explanation();
        let enriched = explanation.clone().with_runtime_outcome(
            ExecutionOutcomeExplanation {
                decision: ExecutionDecisionExplanation::Denied,
                snapshots: Vec::new(),
            },
        );

        assert_eq!(enriched.scan, explanation.scan);
        assert_eq!(enriched.policy, explanation.policy);
        assert_eq!(enriched.context, explanation.context);
        assert_eq!(
            enriched.outcome.as_ref().map(|outcome| outcome.decision),
            Some(ExecutionDecisionExplanation::Denied)
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```bash
rtk cargo test explanation::tests::builds_base_explanation_from_existing_pipeline_facts --lib
rtk cargo test explanation::tests::appends_runtime_outcome_without_rewriting_existing_sections --lib
```

Expected: compile failure because `src/explanation.rs`, `CommandExplanation`, and the runtime-outcome types do not exist yet.

- [ ] **Step 3: Implement the minimal explanation model**

Create `src/explanation.rs` with a thin, serde-friendly contract and add only the derives/helpers needed to embed canonical scanner/policy/context facts directly:

```rust
use serde::{Deserialize, Serialize};

use crate::config::{AllowlistMatch, Mode};
use crate::decision::{
    BlockReason, ExecutionTransport, PolicyAction, PolicyDecision, PolicyRationale,
};
use crate::interceptor::RiskLevel;
use crate::interceptor::scanner::{Assessment, DecisionSource};
use crate::snapshot::SnapshotRecord;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandExplanation {
    pub scan: ScanExplanation,
    pub policy: PolicyExplanation,
    pub context: ExecutionContextExplanation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ExecutionOutcomeExplanation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanExplanation {
    pub highest_risk: RiskLevel,
    pub decision_source: DecisionSource,
    pub matched_patterns: Vec<ExplainedPatternMatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplainedPatternMatch {
    pub id: String,
    pub risk: RiskLevel,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyExplanation {
    pub action: PolicyAction,
    pub rationale: PolicyRationale,
    pub requires_confirmation: bool,
    pub snapshots_required: bool,
    pub allowlist_effective: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<BlockReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionContextExplanation {
    pub mode: Mode,
    pub transport: ExecutionTransport,
    pub ci_detected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowlist_match: Option<AllowlistExplanation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub applicable_snapshot_plugins: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowlistExplanation {
    pub pattern: String,
    pub reason: String,
    pub source_layer: crate::config::AllowlistSourceLayer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionOutcomeExplanation {
    pub decision: ExecutionDecisionExplanation,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snapshots: Vec<SnapshotOutcomeExplanation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionDecisionExplanation {
    Approved,
    Denied,
    AutoApproved,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotOutcomeExplanation {
    pub plugin: String,
    pub snapshot_id: String,
}

impl CommandExplanation {
    pub fn from_plan_inputs(
        assessment: &Assessment,
        decision_context: &crate::planning::DecisionContext,
        policy_decision: PolicyDecision,
    ) -> Self {
        Self {
            scan: ScanExplanation::from_assessment(assessment),
            policy: PolicyExplanation::from(policy_decision),
            context: ExecutionContextExplanation::from_plan_context(decision_context),
            outcome: None,
        }
    }

    pub fn with_runtime_outcome(mut self, outcome: ExecutionOutcomeExplanation) -> Self {
        self.outcome = Some(outcome);
        self
    }
}

impl ExecutionOutcomeExplanation {
    pub fn from_runtime(
        decision: ExecutionDecisionExplanation,
        snapshots: &[SnapshotRecord],
    ) -> Self {
        Self {
            decision,
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
```

Also:

- add `pub mod explanation;` to `src/lib.rs`
- derive `Serialize, Deserialize` for `PolicyAction`, `PolicyRationale`, `BlockReason`, `ExecutionTransport`, `DecisionSource`, and `AllowlistSourceLayer`
- keep every new `pub` item documented with `///` comments

- [ ] **Step 4: Run the focused model tests**

Run:

```bash
rtk cargo test explanation::tests::builds_base_explanation_from_existing_pipeline_facts --lib
rtk cargo test explanation::tests::appends_runtime_outcome_without_rewriting_existing_sections --lib
```

Expected: PASS, confirming the model is thin, ownership-preserving, and explicit about missing runtime outcome.

- [ ] **Step 5: Commit the explanation model**

Run:

```bash
rtk git add src/explanation.rs src/lib.rs src/decision.rs src/interceptor/scanner/assessment.rs src/config/allowlist.rs
rtk git commit -m "feat: add explanation model"
```

## Task 2: Build explanations during planning

**Files:**
- Modify: `src/planning/types.rs`
- Test: `src/planning/types.rs`

- [ ] **Step 1: Write the failing planning tests**

Add tests next to `InterceptionPlan` that pin model-first construction:

```rust
#[test]
fn from_policy_builds_command_explanation_once() {
    let assessment = test_assessment();
    let context = test_decision_context();
    let policy = PolicyDecision {
        decision: PolicyAction::Prompt,
        rationale: PolicyRationale::RequiresConfirmation,
        requires_confirmation: true,
        snapshots_required: true,
        allowlist_effective: false,
    };

    let plan = InterceptionPlan::from_policy(assessment, context, policy);

    assert_eq!(
        plan.explanation().policy.rationale,
        PolicyRationale::RequiresConfirmation
    );
    assert!(plan.explanation().outcome.is_none());
}

#[test]
fn planning_keeps_allowlist_provenance_in_context_section() {
    let plan = planned_command_with_allowlist_match();

    let allowlist = plan
        .explanation()
        .context
        .allowlist_match
        .as_ref()
        .expect("allowlist explanation should be present");

    assert_eq!(allowlist.pattern, "cargo test *");
    assert_eq!(allowlist.reason, "trusted ci command");
    assert_eq!(allowlist.source_layer, AllowlistSourceLayer::Project);
}
```

- [ ] **Step 2: Run the planning tests to verify they fail**

Run:

```bash
rtk cargo test planning::types::tests::from_policy_builds_command_explanation_once --lib
rtk cargo test planning::types::tests::planning_keeps_allowlist_provenance_in_context_section --lib
```

Expected: compile failure because `InterceptionPlan::explanation()` and the underlying explanation field do not exist yet.

- [ ] **Step 3: Replace plan-local audit facts with the explanation contract**

Update `src/planning/types.rs` so `InterceptionPlan` owns the unified explanation object instead of a separate `AuditFacts` payload:

```rust
use crate::explanation::CommandExplanation;

pub struct InterceptionPlan {
    assessment: Box<Assessment>,
    decision_context: DecisionContext,
    policy_decision: PolicyDecision,
    approval_requirement: ApprovalRequirement,
    snapshot_plan: SnapshotPlan,
    execution_disposition: ExecutionDisposition,
    explanation: Box<CommandExplanation>,
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
        let explanation =
            CommandExplanation::from_plan_inputs(&assessment, &decision_context, policy_decision);

        Self {
            assessment: Box::new(assessment),
            decision_context,
            policy_decision,
            approval_requirement,
            snapshot_plan,
            execution_disposition,
            explanation: Box::new(explanation),
        }
    }

    pub fn explanation(&self) -> &CommandExplanation {
        self.explanation.as_ref()
    }
}
```

Keep `SetupFailurePlan.audit_facts` unchanged in this iteration unless a compile error forces a tiny compatibility shim; setup-failure explainability is outside this spec.

- [ ] **Step 4: Run the planning tests**

Run:

```bash
rtk cargo test planning::types::tests::from_policy_builds_command_explanation_once --lib
rtk cargo test planning::types::tests::planning_keeps_allowlist_provenance_in_context_section --lib
```

Expected: PASS, confirming the explanation is built once from already-known planning inputs and stays explicit about absent runtime outcome.

- [ ] **Step 5: Commit the planning integration**

Run:

```bash
rtk git add src/planning/types.rs
rtk git commit -m "refactor: store explanations in plans"
```

## Task 3: Migrate audit logging to consume the explanation model

**Files:**
- Modify: `src/audit/logger.rs`
- Modify: `src/runtime.rs`
- Modify: `src/main.rs`
- Modify: `src/watch.rs`
- Test: `src/audit/logger.rs`
- Test: `src/runtime.rs`

- [ ] **Step 1: Write the failing audit tests**

Add audit-focused tests that prove the logger is a consumer, not a new explanation source:

```rust
#[test]
fn audit_entry_serializes_nested_explanation_sections() {
    let entry = AuditEntry::new(
        "rm -rf build".to_string(),
        RiskLevel::Danger,
        vec![matched_pattern_fixture()],
        Decision::Denied,
        Vec::new(),
        None,
        None,
    )
    .with_explanation(command_explanation_fixture());

    let json = serde_json::to_value(&entry).expect("entry should serialize");

    assert!(json.get("explanation").is_some());
    assert!(json["explanation"].get("scan").is_some());
    assert!(json["explanation"].get("policy").is_some());
    assert!(json["explanation"].get("context").is_some());
}

#[test]
fn audit_entry_keeps_existing_top_level_fields_for_backward_compatibility() {
    let entry = AuditEntry::new(
        "rm -rf build".to_string(),
        RiskLevel::Danger,
        vec![matched_pattern_fixture()],
        Decision::Denied,
        Vec::new(),
        None,
        None,
    )
    .with_explanation(command_explanation_fixture());

    assert_eq!(entry.command, "rm -rf build");
    assert_eq!(entry.risk, RiskLevel::Danger);
    assert_eq!(entry.pattern_ids, vec!["FS-001".to_string()]);
}
```

- [ ] **Step 2: Run the audit tests to verify they fail**

Run:

```bash
rtk cargo test audit::logger::tests::audit_entry_serializes_nested_explanation_sections --lib
rtk cargo test audit::logger::tests::audit_entry_keeps_existing_top_level_fields_for_backward_compatibility --lib
```

Expected: compile failure because `AuditEntry::with_explanation` and the nested explanation field do not exist yet.

- [ ] **Step 3: Add nested explanation serialization and runtime enrichment**

Update the audit and runtime path without flattening ownership boundaries:

```rust
// src/audit/logger.rs
use crate::explanation::CommandExplanation;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: AuditTimestamp,
    pub sequence: u64,
    pub command: String,
    pub risk: RiskLevel,
    pub matched_patterns: Vec<MatchedPattern>,
    #[serde(default)]
    pub pattern_ids: Vec<String>,
    pub decision: Decision,
    pub snapshots: Vec<AuditSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<CommandExplanation>,
    // existing fields stay here unchanged
}

impl AuditEntry {
    pub fn with_explanation(mut self, explanation: CommandExplanation) -> Self {
        self.explanation = Some(explanation);
        self
    }
}

// src/runtime.rs
use crate::explanation::{CommandExplanation, ExecutionDecisionExplanation, ExecutionOutcomeExplanation};

impl RuntimeContext {
    pub fn append_audit_entry(
        &self,
        assessment: &Assessment,
        explanation: &CommandExplanation,
        decision: Decision,
        snapshots: &[SnapshotRecord],
        options: AuditWriteOptions<'_>,
    ) {
        let outcome = ExecutionOutcomeExplanation::from_runtime(
            match decision {
                Decision::Approved => ExecutionDecisionExplanation::Approved,
                Decision::Denied => ExecutionDecisionExplanation::Denied,
                Decision::AutoApproved => ExecutionDecisionExplanation::AutoApproved,
                Decision::Blocked => ExecutionDecisionExplanation::Blocked,
            },
            snapshots,
        );

        let entry = AuditEntry::new(
            assessment.command.raw.clone(),
            assessment.risk,
            assessment.matched.iter().map(Into::into).collect(),
            decision,
            snapshots.iter().map(Into::into).collect(),
            allowlist_pattern,
            allowlist_reason,
        )
        .with_policy_context(
            self.runtime_config.mode,
            options.ci_detected,
            options.allowlist_match.is_some(),
            options.allowlist_effective,
        )
        .with_explanation(explanation.clone().with_runtime_outcome(outcome));

        if let Err(err) = self.audit_logger.append(entry)
            && options.verbose
        {
            eprintln!("warning: failed to append audit log entry: {err}");
        }
    }
}
```

Update `src/main.rs` and `src/watch.rs` call sites to pass `plan.explanation()` into the runtime append helpers.

- [ ] **Step 4: Run the audit-focused tests and the runtime regression slice**

Run:

```bash
rtk cargo test audit::logger::tests::audit_entry_serializes_nested_explanation_sections --lib
rtk cargo test audit::logger::tests::audit_entry_keeps_existing_top_level_fields_for_backward_compatibility --lib
rtk cargo test runtime::tests::append_audit_entry --lib
```

Expected: PASS, confirming the logger serializes the unified explanation as a nested consumer view while preserving the current top-level audit contract.

- [ ] **Step 5: Commit the audit migration**

Run:

```bash
rtk git add src/audit/logger.rs src/runtime.rs src/main.rs src/watch.rs
rtk git commit -m "feat: serialize explanations in audit"
```

## Task 4: Migrate confirmation and block UI to consume explanations

**Files:**
- Modify: `src/ui/confirm.rs`
- Modify: `src/main.rs`
- Modify: `src/watch.rs`
- Test: `src/ui/confirm.rs`

- [ ] **Step 1: Write the failing UI tests**

Add focused tests showing that UI reads explanation data instead of inventing it:

```rust
#[test]
fn confirmation_renders_policy_reason_from_explanation() {
    let assessment = test_danger_assessment();
    let explanation = test_prompt_explanation();
    let mut output = Vec::new();

    let approved = show_confirmation_with_input(
        &assessment,
        &explanation,
        &[],
        true,
        &mut b"no\n".as_ref(),
        &mut output,
    );

    assert!(!approved);
    let rendered = String::from_utf8(output).expect("ui output should be utf8");
    assert!(rendered.contains("Reason: requires confirmation"));
}

#[test]
fn policy_block_renders_from_canonical_block_reason() {
    let assessment = test_warn_assessment();
    let explanation = test_strict_block_explanation();
    let mut output = Vec::new();

    render_policy_block(&assessment, &explanation, &mut output);

    let rendered = String::from_utf8(output).expect("ui output should be utf8");
    assert!(rendered.contains("blocked by strict mode"));
    assert!(!rendered.contains("inspect the allowlist or run aegis config validate"));
}
```

- [ ] **Step 2: Run the UI tests to verify they fail**

Run:

```bash
rtk cargo test ui::confirm::tests::confirmation_renders_policy_reason_from_explanation --lib
rtk cargo test ui::confirm::tests::policy_block_renders_from_canonical_block_reason --lib
```

Expected: compile failure because the confirmation helpers do not accept `CommandExplanation` yet.

- [ ] **Step 3: Thread explanations through the confirmation surface**

Change the UI signatures so rendering becomes a projection of the unified explanation model:

```rust
use crate::explanation::CommandExplanation;

pub fn show_confirmation(
    assessment: &Assessment,
    explanation: &CommandExplanation,
    snapshots: &[SnapshotRecord],
) -> bool {
    use std::io::IsTerminal;
    let forced = std::env::var_os("AEGIS_FORCE_INTERACTIVE")
        .map(|v| v == "1")
        .unwrap_or(false);
    let is_interactive = forced || io::stdin().is_terminal();
    show_confirmation_with_input(
        assessment,
        explanation,
        snapshots,
        is_interactive,
        &mut io::stdin().lock(),
        &mut io::stderr(),
    )
}

pub fn show_confirmation_with_input<R: BufRead, W: Write>(
    assessment: &Assessment,
    explanation: &CommandExplanation,
    snapshots: &[SnapshotRecord],
    is_interactive: bool,
    input: &mut R,
    output: &mut W,
) -> bool {
    match assessment.risk {
        RiskLevel::Block => {
            render_block(assessment, explanation, output);
            false
        }
        RiskLevel::Danger | RiskLevel::Warn if !is_interactive => {
            render_noninteractive_denial(assessment, explanation, output);
            false
        }
        RiskLevel::Danger => {
            render_dialog(assessment, explanation, snapshots, output);
            prompt_danger(input, output)
        }
        RiskLevel::Warn => {
            render_dialog(assessment, explanation, snapshots, output);
            prompt_warn(input, output)
        }
        _ => true,
    }
}

pub fn show_policy_block(assessment: &Assessment, explanation: &CommandExplanation) {
    let mut stderr = io::stderr();
    render_policy_block(assessment, explanation, &mut stderr);
}
```

Use `explanation.policy.rationale`, `explanation.policy.block_reason`, and `explanation.context.allowlist_match` to render concise, canonical reasons. Keep command text and highlight spans sourced from `assessment`; do not add any new discovery work in the UI layer.

Update `src/main.rs` and `src/watch.rs` call sites so prompt/block rendering always receives `plan.explanation()`.

- [ ] **Step 4: Run the focused UI tests**

Run:

```bash
rtk cargo test ui::confirm::tests::confirmation_renders_policy_reason_from_explanation --lib
rtk cargo test ui::confirm::tests::policy_block_renders_from_canonical_block_reason --lib
```

Expected: PASS, confirming the UI now consumes the model rather than stitching reasons from scattered sources.

- [ ] **Step 5: Commit the UI migration**

Run:

```bash
rtk git add src/ui/confirm.rs src/main.rs src/watch.rs
rtk git commit -m "refactor: render confirmation from explanation"
```

## Task 5: Add regression coverage for ownership boundaries and compatibility

**Files:**
- Modify: `src/explanation.rs`
- Modify: `src/audit/logger.rs`
- Modify: `src/ui/confirm.rs`
- Modify: `tests/full_pipeline.rs` (only if an integration test is the smallest way to prove the behavior)

- [ ] **Step 1: Write the failing regression tests**

Add tests that pin the most important non-drift guarantees from the spec:

```rust
#[test]
fn explanation_json_preserves_layer_boundaries() {
    let explanation = test_explanation().with_runtime_outcome(
        ExecutionOutcomeExplanation {
            decision: ExecutionDecisionExplanation::Approved,
            snapshots: vec![SnapshotOutcomeExplanation {
                plugin: "git".to_string(),
                snapshot_id: "snap-123".to_string(),
            }],
        },
    );

    let json = serde_json::to_value(&explanation).expect("explanation should serialize");

    assert!(json.get("scan").is_some());
    assert!(json.get("policy").is_some());
    assert!(json.get("context").is_some());
    assert!(json.get("outcome").is_some());
}

#[test]
fn ui_rendering_does_not_need_to_synthesize_missing_optional_sections() {
    let assessment = test_warn_assessment();
    let explanation = test_warn_explanation_without_outcome();
    let mut output = Vec::new();

    let denied = show_confirmation_with_input(
        &assessment,
        &explanation,
        &[],
        false,
        &mut b"yes\n".as_ref(),
        &mut output,
    );

    assert!(!denied);
    let rendered = String::from_utf8(output).expect("ui output should be utf8");
    assert!(!rendered.contains("snapshot_id"));
}
```

- [ ] **Step 2: Run the regression slice to verify it fails**

Run:

```bash
rtk cargo test explanation_json_preserves_layer_boundaries --lib
rtk cargo test ui_rendering_does_not_need_to_synthesize_missing_optional_sections --lib
```

Expected: FAIL until the final serialization and UI projection details are fully wired.

- [ ] **Step 3: Implement the smallest compatibility fixes required by the tests**

Make only the narrow fixes required to preserve the approved boundaries:

```rust
// example compatibility helpers
impl PolicyExplanation {
    pub fn concise_reason_label(&self) -> &'static str {
        match self.rationale {
            PolicyRationale::AuditMode => "audit mode auto-approved this command",
            PolicyRationale::SafeCommand => "safe command",
            PolicyRationale::AllowlistOverride => "allowlist override applied",
            PolicyRationale::RequiresConfirmation => "requires confirmation",
            PolicyRationale::IntrinsicRiskBlock => "blocked by intrinsic risk",
            PolicyRationale::ProtectCiPolicy => "blocked by protect-mode CI policy",
            PolicyRationale::StrictPolicy => "blocked by strict mode",
        }
    }
}
```

If a regression requires adding one more helper or serializer attribute, do it in the owner module (`src/explanation.rs`, `src/audit/logger.rs`, or `src/ui/confirm.rs`) instead of adding ad hoc fallback logic in consumers.

- [ ] **Step 4: Run the library + integration verification slice**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test explanation --lib
rtk cargo test planning::types --lib
rtk cargo test audit::logger --lib
rtk cargo test ui::confirm --lib
rtk cargo test --test full_pipeline
```

Expected: PASS, with no semantic drift in approval/deny/block behavior and no new warnings.

- [ ] **Step 5: Commit the regression coverage**

Run:

```bash
rtk git add src/explanation.rs src/audit/logger.rs src/ui/confirm.rs tests/full_pipeline.rs
rtk git commit -m "test: add explainability regressions"
```

## Verification Notes

- Keep audit top-level fields (`command`, `risk`, `matched_patterns`, `pattern_ids`, `decision`, `snapshots`) intact for backward compatibility
- Prefer unit tests for ownership boundaries and only add `tests/full_pipeline.rs` coverage if a unit test cannot prove the behavior cleanly
- Do not claim stronger guarantees in UI or audit text than Aegis already enforces
- If a performance concern appears, document it in the implementation summary instead of adding lazy discovery work to the safe path

## Rollback Notes

- If Task 1 causes serde churn that touches too many public enums, stop and split the derives into the minimal set needed by the explanation model
- If Task 3 threatens audit backward compatibility, keep the new nested `explanation` field optional and preserve every existing top-level field exactly
- If Task 4 grows beyond reason rendering and starts moving decision logic into UI helpers, roll that task back and keep the UI migration limited to projection-only changes
