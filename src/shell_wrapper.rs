use std::env;

use tokio::runtime::Handle;

use aegis::config::AllowlistMatch;
use aegis::decision::ExecutionTransport;
use aegis::interceptor::scanner::{Assessment, DecisionSource};
use aegis::planning::{
    CwdState, InterceptionPlan, PlanningOutcome, PreparedPlanner, SetupFailureKind,
    SetupFailurePlan, prepare_and_plan, prepare_planner,
};
use aegis::runtime_gate::is_ci_environment;
use aegis::toggle;

use crate::policy_output;
use crate::shell_compat::{self, ShellLaunchOptions};
use crate::shell_flow;
use crate::{CommandOutputFormat, EXIT_INTERNAL, OutputVerbosity};

pub(crate) fn run_shell_wrapper(
    cmd: &str,
    output: CommandOutputFormat,
    verbosity: OutputVerbosity,
    handle: Handle,
    launch: &ShellLaunchOptions,
) -> i32 {
    let in_ci = is_ci_environment();
    if !in_ci {
        let toggle_state = match toggle::status() {
            Ok(state) => state,
            Err(err) => {
                eprintln!("error: failed to read toggle state: {err}");
                return EXIT_INTERNAL;
            }
        };

        if matches!(toggle_state, toggle::ToggleState::Disabled)
            && matches!(output, CommandOutputFormat::Text)
        {
            return shell_compat::exec_command(cmd, launch);
        }
    }

    let prepared = prepare_planner(verbosity.is_verbose(), handle);
    let cwd_state = match env::current_dir() {
        Ok(path) => CwdState::Resolved(path),
        Err(_) => CwdState::Unavailable,
    };
    let transport = match output {
        CommandOutputFormat::Text => ExecutionTransport::Shell,
        CommandOutputFormat::Json => ExecutionTransport::Evaluation,
    };
    let outcome = prepare_and_plan(
        &prepared,
        aegis::planning::PlanningRequest {
            command: cmd,
            cwd_state,
            transport,
            ci_detected: in_ci,
        },
    );

    if verbosity.is_verbose() && matches!(output, CommandOutputFormat::Text) {
        if in_ci && let PreparedPlanner::Ready(context) = &prepared {
            eprintln!(
                "ci: detected CI environment, ci_policy={:?}",
                context.config().ci_policy
            );
        }
        if let PlanningOutcome::Planned(plan) = &outcome {
            log_assessment(plan.assessment(), plan.decision_context().allowlist_match());
        }
    }

    if matches!(output, CommandOutputFormat::Json) {
        return render_json_outcome(&prepared, &outcome);
    }

    run_shell_text_outcome(cmd, verbosity, &prepared, outcome, launch)
}

fn report_setup_failure(plan: &SetupFailurePlan) -> i32 {
    eprintln!("{}", plan.user_message());
    if matches!(plan.kind(), SetupFailureKind::InvalidConfig) {
        eprintln!("error: Fix or remove the invalid config file and try again.");
    }
    EXIT_INTERNAL
}

fn render_json_outcome(prepared: &PreparedPlanner, outcome: &PlanningOutcome) -> i32 {
    match outcome {
        PlanningOutcome::SetupFailure(plan) => report_setup_failure(plan),
        PlanningOutcome::Planned(plan) => match prepared {
            PreparedPlanner::Ready(context) => {
                emit_policy_evaluation_json(plan, context.config().ci_policy)
            }
            PreparedPlanner::SetupFailure(_) => EXIT_INTERNAL,
        },
    }
}

fn run_shell_text_outcome(
    cmd: &str,
    verbosity: OutputVerbosity,
    prepared: &PreparedPlanner,
    outcome: PlanningOutcome,
    launch: &ShellLaunchOptions,
) -> i32 {
    match outcome {
        PlanningOutcome::SetupFailure(plan) => report_setup_failure(&plan),
        PlanningOutcome::Planned(plan) => shell_flow::run_planned_shell_command(
            cmd,
            verbosity.is_verbose(),
            prepared,
            &plan,
            launch,
        ),
    }
}

fn log_assessment(assessment: &Assessment, allowlist_match: Option<&AllowlistMatch>) {
    let source_label = match assessment.decision_source() {
        DecisionSource::BuiltinPattern => "built-in pattern",
        DecisionSource::CustomPattern => "custom pattern",
        DecisionSource::Fallback => "fallback",
    };

    eprintln!(
        "scan: risk={:?}, executable={}, matched={}, source={}",
        assessment.risk,
        assessment.command.executable.as_deref().unwrap_or("<none>"),
        assessment.matched.len(),
        source_label,
    );

    for m in &assessment.matched {
        eprintln!(
            "match: id={}, category={:?}, risk={:?}, matched={:?}, description={}",
            m.pattern.id, m.pattern.category, m.pattern.risk, m.matched_text, m.pattern.description
        );

        if let Some(safe_alt) = &m.pattern.safe_alt {
            eprintln!("safe alternative: {safe_alt}");
        }
    }

    if let Some(rule) = allowlist_match {
        eprintln!("allowlist: matched rule {:?}", rule.pattern);
    }
}

fn emit_policy_evaluation_json(plan: &InterceptionPlan, ci_policy: aegis::config::CiPolicy) -> i32 {
    match policy_output::render_planned(plan, ci_policy) {
        Ok(json) => {
            println!("{json}");
            policy_output::exit_code_for(plan.policy_decision().decision)
        }
        Err(err) => {
            eprintln!("error: failed to serialize policy evaluation output: {err}");
            EXIT_INTERNAL
        }
    }
}
