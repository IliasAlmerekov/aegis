use std::fs::OpenOptions;
use std::io;

use crate::explanation::CommandExplanation;
use crate::interceptor::RiskLevel;
use crate::interceptor::scanner::Assessment;
use crate::snapshot::SnapshotRecord;

use super::block_screen::{render_block, render_policy_block};
use super::stdout_renderer::show_confirmation_with_input;

pub fn tty_unavailable_decision(assessment: &Assessment) -> bool {
    matches!(assessment.risk, RiskLevel::Safe)
}

/// Show the confirmation dialog via `/dev/tty`.
///
/// Opens `/dev/tty` for both input (keystrokes) and output (dialog
/// rendering).  If the device cannot be opened, returns
/// `tty_unavailable_decision(assessment)` — fail-closed for Warn/Danger.
pub fn show_confirmation_via_tty(
    assessment: &Assessment,
    explanation: &CommandExplanation,
    snapshots: &[SnapshotRecord],
) -> bool {
    let tty = match OpenOptions::new().read(true).write(true).open("/dev/tty") {
        Ok(f) => f,
        Err(_) => return tty_unavailable_decision(assessment),
    };
    let tty_write = match tty.try_clone() {
        Ok(f) => f,
        Err(_) => return tty_unavailable_decision(assessment),
    };

    show_confirmation_with_input(
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
