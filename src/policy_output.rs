use aegis::config::{CiPolicy, Mode};
use aegis::decision::{BlockReason, PolicyAction};
use aegis::interceptor::scanner::DecisionSource;
use aegis::planning::{InterceptionPlan, SnapshotPlan};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct PolicyEvaluationOutput {
    schema_version: u32,
    command: String,
    risk: String,
    decision: String,
    exit_code: i32,
    mode: String,
    ci_state: CiState,
    matched_patterns: Vec<MatchedPatternOutput>,
    allowlist_match: AllowlistMatchOutput,
    snapshots_created: Vec<SnapshotCreatedOutput>,
    snapshot_plan: SnapshotPlanOutput,
    execution: ExecutionOutput,
    #[serde(skip_serializing_if = "Option::is_none")]
    block_reason: Option<String>,
    decision_source: String,
}

#[derive(Debug, Serialize)]
struct CiState {
    detected: bool,
    policy: String,
}

#[derive(Debug, Serialize)]
struct MatchedPatternOutput {
    id: String,
    category: String,
    risk: String,
    matched_text: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    safe_alternative: Option<String>,
    source: String,
}

#[derive(Debug, Serialize)]
struct AllowlistMatchOutput {
    matched: bool,
    effective: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct SnapshotCreatedOutput {
    plugin: String,
    snapshot_id: String,
}

#[derive(Debug, Serialize)]
struct SnapshotPlanOutput {
    requested: bool,
    applicable_plugins: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ExecutionOutput {
    mode: &'static str,
    will_execute: bool,
}

pub(crate) fn render_planned(
    plan: &InterceptionPlan,
    ci_policy: CiPolicy,
    snapshot_plugins_override: Option<Vec<&'static str>>,
) -> Result<String, serde_json::Error> {
    let assessment = plan.assessment();
    let decision = plan.policy_decision();
    let decision_context = plan.decision_context();
    let allowlist_match = decision_context.allowlist_match();
    let plan_snapshot = plan.snapshot_plan();
    let (snapshot_requested, applicable_snapshot_plugins) = match snapshot_plugins_override {
        Some(applicable_plugins) => (true, applicable_plugins),
        None => match &plan_snapshot {
            SnapshotPlan::NotRequired => (false, Vec::new()),
            SnapshotPlan::Required { applicable_plugins } => (true, applicable_plugins.clone()),
        },
    };
    let decision_label = decision_string(decision.decision);
    let exit_code = exit_code_for(decision.decision);
    let output = PolicyEvaluationOutput {
        schema_version: 1,
        command: assessment.command.raw.clone(),
        risk: assessment.risk.to_string(),
        decision: decision_label.to_string(),
        exit_code,
        mode: mode_string(decision_context.mode()).to_string(),
        ci_state: CiState {
            detected: decision_context.ci_detected(),
            policy: ci_policy_string(ci_policy).to_string(),
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
            matched: allowlist_match.is_some(),
            effective: decision.allowlist_effective,
            pattern: allowlist_match.map(|matched| matched.pattern.clone()),
            reason: allowlist_match.map(|matched| matched.reason.clone()),
        },
        snapshots_created: Vec::new(),
        snapshot_plan: SnapshotPlanOutput {
            requested: snapshot_requested,
            applicable_plugins: applicable_snapshot_plugins
                .into_iter()
                .map(str::to_string)
                .collect(),
        },
        execution: ExecutionOutput {
            mode: "evaluation_only",
            will_execute: false,
        },
        block_reason: decision
            .block_reason()
            .map(|reason| block_reason_string(reason).to_string()),
        decision_source: decision_source_string(assessment.decision_source()).to_string(),
    };

    serde_json::to_string_pretty(&output)
}

pub(crate) fn exit_code_for(action: PolicyAction) -> i32 {
    match action {
        PolicyAction::AutoApprove => 0,
        PolicyAction::Prompt => crate::EXIT_DENIED,
        PolicyAction::Block => crate::EXIT_BLOCKED,
    }
}

fn decision_string(action: PolicyAction) -> &'static str {
    match action {
        PolicyAction::AutoApprove => "auto_approve",
        PolicyAction::Prompt => "prompt",
        PolicyAction::Block => "block",
    }
}

fn mode_string(mode: Mode) -> &'static str {
    match mode {
        Mode::Protect => "protect",
        Mode::Audit => "audit",
        Mode::Strict => "strict",
    }
}

fn ci_policy_string(policy: CiPolicy) -> &'static str {
    match policy {
        CiPolicy::Block => "block",
        CiPolicy::Allow => "allow",
    }
}

fn block_reason_string(reason: BlockReason) -> &'static str {
    match reason {
        BlockReason::IntrinsicRiskBlock => "intrinsic_risk_block",
        BlockReason::StrictPolicy => "strict_policy",
        BlockReason::ProtectCiPolicy => "protect_ci_policy",
    }
}

fn decision_source_string(source: DecisionSource) -> &'static str {
    match source {
        DecisionSource::BuiltinPattern => "builtin_pattern",
        DecisionSource::CustomPattern => "custom_pattern",
        DecisionSource::Fallback => "fallback",
    }
}

fn category_string(category: aegis::interceptor::patterns::Category) -> &'static str {
    match category {
        aegis::interceptor::patterns::Category::Filesystem => "filesystem",
        aegis::interceptor::patterns::Category::Git => "git",
        aegis::interceptor::patterns::Category::Database => "database",
        aegis::interceptor::patterns::Category::Cloud => "cloud",
        aegis::interceptor::patterns::Category::Docker => "docker",
        aegis::interceptor::patterns::Category::Process => "process",
        aegis::interceptor::patterns::Category::Package => "package",
    }
}

fn pattern_source_string(source: aegis::interceptor::patterns::PatternSource) -> &'static str {
    match source {
        aegis::interceptor::patterns::PatternSource::Builtin => "builtin",
        aegis::interceptor::patterns::PatternSource::Custom => "custom",
    }
}
