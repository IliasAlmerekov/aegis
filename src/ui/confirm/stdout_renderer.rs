use std::io::{self, BufRead, Write};

use crate::explanation::CommandExplanation;
use crate::interceptor::RiskLevel;
use crate::interceptor::scanner::Assessment;
use crate::snapshot::SnapshotRecord;

use super::block_screen::{render_block, render_noninteractive_denial, render_policy_block};
use super::confirm_screen::{prompt_danger, prompt_warn, render_dialog};

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
/// - `Danger` → shows the dialog; user must type `y`/`yes` to proceed.
/// - `Warn`   → shows the dialog; user must type `y`/`yes` to proceed.
/// - `Safe`   → auto-approves without rendering anything (should not normally reach here).
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
        // Fail-closed: without a human at the keyboard, deny anything that
        // would normally require a prompt.  This prevents an AI agent from
        // running dangerous commands unattended in CI.
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
        // Safe commands should not reach the dialog — auto-approve.
        _ => true,
    }
}
