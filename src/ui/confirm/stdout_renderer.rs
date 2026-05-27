use std::io::{self, BufRead, Write};

use crate::explanation::CommandExplanation;
use crate::interceptor::RiskLevel;
use crate::interceptor::scanner::Assessment;
use crate::snapshot::SnapshotRecord;

use super::block_screen::{render_block, render_noninteractive_denial, render_policy_block};
use super::confirm_screen::render_dialog;

/// The user's decision in an interactive confirmation dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptDecision {
    /// Approve this command once.
    Approve,
    /// Approve this command and persist an allow rule for its prefix.
    ApproveAlways,
    /// Deny this command.
    Deny,
    /// Deny this command and persist a block rule for its prefix.
    DenyAlways,
}

pub fn show_confirmation(
    assessment: &Assessment,
    explanation: &CommandExplanation,
    snapshots: &[SnapshotRecord],
) -> bool {
    use std::io::IsTerminal;
    // `AEGIS_FORCE_INTERACTIVE=1` lets test harnesses and scripted setups
    // opt into interactive mode even when stdin is a pipe.  It must never
    // be set in production CI; it exists solely for integration-test use.
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

/// Like [`show_confirmation`] but returns a [`PromptDecision`] so callers can
/// distinguish a one-time approval from an "Always allow" choice.
pub fn show_confirmation_decision(
    assessment: &Assessment,
    explanation: &CommandExplanation,
    snapshots: &[SnapshotRecord],
) -> PromptDecision {
    use std::io::IsTerminal;
    let forced = std::env::var_os("AEGIS_FORCE_INTERACTIVE")
        .map(|v| v == "1")
        .unwrap_or(false);
    let is_interactive = forced || io::stdin().is_terminal();
    show_confirmation_with_decision(
        assessment,
        explanation,
        snapshots,
        is_interactive,
        &mut io::stdin().lock(),
        &mut io::stderr(),
    )
}

/// Show a focused policy-block message for runtime policy decisions.
pub fn show_policy_block(assessment: &Assessment, explanation: &CommandExplanation) {
    let mut stderr = io::stderr();
    render_policy_block(assessment, explanation, &mut stderr);
}

/// Testable inner version — accepts any `BufRead` for input and `Write` for output.
///
/// `is_interactive` must be `true` when stdin is a TTY (the user can type a
/// response) and `false` in CI pipelines, agent runners, or any other context
/// where there is no human at the keyboard.
///
/// Non-interactive behavior (fail-closed):
/// - `Block`  → always denied; same as interactive.
/// - `Danger` → denied immediately; no prompt shown.
/// - `Warn`   → denied immediately; no prompt shown.
/// - `Safe`   → auto-approved; same as interactive.
///
/// Interactive behavior:
/// - `Block`  → prints the reason and returns `false` immediately; no prompt shown.
/// - `Danger` → shows the dialog; user must type `y`/`yes`/`a` to proceed.
/// - `Warn`   → shows the dialog; user must type `y`/`yes`/`a` to proceed.
/// - `Safe`   → auto-approves without rendering anything (should not normally reach here).
pub fn show_confirmation_with_input<R: BufRead, W: Write>(
    assessment: &Assessment,
    explanation: &CommandExplanation,
    snapshots: &[SnapshotRecord],
    is_interactive: bool,
    input: &mut R,
    output: &mut W,
) -> bool {
    matches!(
        show_confirmation_with_decision(
            assessment,
            explanation,
            snapshots,
            is_interactive,
            input,
            output
        ),
        PromptDecision::Approve | PromptDecision::ApproveAlways
    )
}

/// Like [`show_confirmation_with_input`] but returns a [`PromptDecision`] so
/// callers can distinguish a one-time approval from an "Always allow" choice.
pub fn show_confirmation_with_decision<R: BufRead, W: Write>(
    assessment: &Assessment,
    explanation: &CommandExplanation,
    snapshots: &[SnapshotRecord],
    is_interactive: bool,
    input: &mut R,
    output: &mut W,
) -> PromptDecision {
    match assessment.risk {
        RiskLevel::Block => {
            render_block(assessment, explanation, output);
            PromptDecision::Deny
        }
        // Fail-closed: without a human at the keyboard, deny anything that
        // would normally require a prompt.  This prevents an AI agent from
        // running dangerous commands unattended in CI.
        RiskLevel::Danger | RiskLevel::Warn if !is_interactive => {
            render_noninteractive_denial(assessment, explanation, output);
            PromptDecision::Deny
        }
        RiskLevel::Danger => {
            render_dialog(assessment, explanation, snapshots, output);
            super::confirm_screen::prompt_danger_with_always(input, output)
        }
        RiskLevel::Warn => {
            render_dialog(assessment, explanation, snapshots, output);
            super::confirm_screen::prompt_warn_with_always(input, output)
        }
        // Safe commands should not reach the dialog — auto-approve.
        _ => PromptDecision::Approve,
    }
}
