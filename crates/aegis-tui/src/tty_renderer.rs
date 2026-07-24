use std::fs::OpenOptions;
use std::io;

use aegis_explanation::CommandExplanation;
use aegis_types::{Assessment, RiskLevel, SnapshotRecord};

use super::block_screen::{render_block, render_policy_block};
use super::stdout_renderer::{PromptDecision, show_confirmation_with_decision};

/// Return the default decision when a TTY is not available.
pub fn tty_unavailable_decision(assessment: &Assessment) -> bool {
    matches!(assessment.risk, RiskLevel::Safe)
}

pub(crate) fn tty_unavailable_prompt_decision(
    assessment: &Assessment,
    explanation: &CommandExplanation,
) -> PromptDecision {
    if explanation.policy.requires_confirmation {
        PromptDecision::Deny
    } else if tty_unavailable_decision(assessment) {
        PromptDecision::Approve
    } else {
        PromptDecision::Deny
    }
}

/// Show the confirmation dialog via `/dev/tty` and return a [`PromptDecision`].
///
/// Opens `/dev/tty` for both input (keystrokes) and output (dialog
/// rendering). If the device cannot be opened, any policy-required
/// confirmation is denied, including Safe language-analysis degradation.
pub fn show_confirmation_via_tty_with_decision(
    assessment: &Assessment,
    explanation: &CommandExplanation,
    snapshots: &[SnapshotRecord],
) -> PromptDecision {
    if std::env::var_os("AEGIS_FORCE_NO_TTY")
        .map(|value| value == "1")
        .unwrap_or(false)
    {
        return tty_unavailable_prompt_decision(assessment, explanation);
    }

    let tty = match OpenOptions::new().read(true).write(true).open("/dev/tty") {
        Ok(f) => f,
        Err(_) => {
            return tty_unavailable_prompt_decision(assessment, explanation);
        }
    };
    let tty_write = match tty.try_clone() {
        Ok(f) => f,
        Err(_) => {
            return tty_unavailable_prompt_decision(assessment, explanation);
        }
    };

    show_confirmation_with_decision(
        assessment,
        explanation,
        snapshots,
        true, // /dev/tty is always interactive
        &mut io::BufReader::new(tty),
        &mut { tty_write },
    )
}

/// Show a policy-block notice via `/dev/tty`.
///
/// If `/dev/tty` cannot be opened, does nothing — the caller must still
/// emit the correct NDJSON result frame.
pub fn show_policy_block_via_tty(assessment: &Assessment, explanation: &CommandExplanation) {
    if let Ok(mut tty) = OpenOptions::new().write(true).open("/dev/tty") {
        render_policy_block(assessment, explanation, &mut tty);
    }
}

/// Show an intrinsic-block notice (RiskLevel::Block pattern) via `/dev/tty`.
///
/// Uses the same `render_block` path as the shell-wrapper mode but routes
/// output to the tty device.  If `/dev/tty` cannot be opened, silent.
pub fn show_block_via_tty(assessment: &Assessment, explanation: &CommandExplanation) {
    if let Ok(mut tty) = OpenOptions::new().write(true).open("/dev/tty") {
        render_block(assessment, explanation, &mut tty);
    }
}
