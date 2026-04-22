use std::io::Write;

use crossterm::{
    queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
};

use crate::explanation::CommandExplanation;
use crate::interceptor::RiskLevel;
use crate::interceptor::scanner::Assessment;

use super::shared::{
    block_reason_text, confirmation_reason_text, pattern_source_label, print_command_line,
};

pub(super) fn render_block<W: Write>(
    assessment: &Assessment,
    explanation: &CommandExplanation,
    out: &mut W,
) {
    let _ = queue!(
        out,
        SetForegroundColor(Color::Red),
        SetAttribute(Attribute::Bold),
        Print("\n  AEGIS BLOCKED THIS COMMAND\n"),
        ResetColor,
    );

    print_command_line(assessment, out);

    let _ = queue!(
        out,
        Print(format!("  Reason: {}\n", block_reason_text(explanation))),
        Print("  Hint: review the matched patterns below.\n"),
        Print("  Hint: rerun with --output json for machine-readable policy details.\n"),
    );

    if !assessment.matched.is_empty() {
        let _ = queue!(out, Print("\n  Matched patterns:\n"));
        for m in &assessment.matched {
            let source_label = pattern_source_label(m.pattern.source);
            let _ = queue!(
                out,
                SetForegroundColor(Color::Red),
                Print(format!(
                    "    [{}] {:?} | {} ({})\n",
                    m.pattern.id, m.pattern.category, m.pattern.description, source_label
                )),
                ResetColor,
            );
        }
    }

    let _ = out.flush();
}

/// Print a non-interactive denial notice.  Emitted when stdin is not a TTY
/// and the command would have triggered a prompt in interactive mode.
///
/// We use Yellow (not Red) to distinguish from Block, which is truly
/// catastrophic.  This denial is policy-driven, not a safety absolute.
pub(super) fn render_noninteractive_denial<W: Write>(
    assessment: &Assessment,
    explanation: &CommandExplanation,
    out: &mut W,
) {
    let risk_label = match assessment.risk {
        RiskLevel::Danger => "dangerous",
        RiskLevel::Warn => "suspicious",
        _ => "flagged",
    };

    let _ = queue!(
        out,
        SetForegroundColor(Color::Yellow),
        SetAttribute(Attribute::Bold),
        Print(format!(
            "\n  AEGIS: non-interactive mode — {risk_label} command denied\n"
        )),
        ResetColor,
    );

    print_command_line(assessment, out);

    let _ = queue!(
        out,
        Print(format!(
            "  Reason: {}\n",
            confirmation_reason_text(explanation)
        )),
        Print("  Hint: add the command to the allowlist for CI use.\n"),
        Print("  Hint: rerun with --output json for machine-readable policy details.\n"),
    );

    let _ = out.flush();
}

pub(super) fn render_policy_block<W: Write>(
    assessment: &Assessment,
    explanation: &CommandExplanation,
    out: &mut W,
) {
    let _ = queue!(
        out,
        SetForegroundColor(Color::Yellow),
        SetAttribute(Attribute::Bold),
        Print("\n  AEGIS POLICY BLOCKED THIS COMMAND\n\n"),
        ResetColor,
    );

    print_command_line(assessment, out);

    let _ = queue!(
        out,
        Print(format!("  Reason: {}\n", block_reason_text(explanation))),
        Print("  Hint: inspect the allowlist or run aegis config validate.\n"),
        Print("  Hint: rerun with --output json for machine-readable policy details.\n"),
    );
    let _ = out.flush();
}
