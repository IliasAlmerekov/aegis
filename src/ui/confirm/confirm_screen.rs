use std::io::{BufRead, Write};

use crossterm::{
    queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
};

use crate::explanation::CommandExplanation;
use crate::interceptor::RiskLevel;
use crate::interceptor::scanner::Assessment;
use crate::snapshot::SnapshotRecord;

use super::shared::{
    confirmation_reason_text, decision_source_label, pattern_source_label, print_command_line,
    print_dangerous_action,
};

pub(super) fn render_dialog<W: Write>(
    assessment: &Assessment,
    explanation: &CommandExplanation,
    snapshots: &[SnapshotRecord],
    out: &mut W,
) {
    let (color, label) = match assessment.risk {
        RiskLevel::Danger => (Color::Red, "AEGIS INTERCEPTED A DANGEROUS COMMAND"),
        RiskLevel::Warn => (Color::Yellow, "AEGIS INTERCEPTED A SUSPICIOUS COMMAND"),
        _ => (Color::Yellow, "AEGIS WARNING"),
    };

    let _ = queue!(
        out,
        SetForegroundColor(color),
        SetAttribute(Attribute::Bold),
        Print(format!("\n  {label}\n\n")),
        ResetColor,
    );

    print_command_line(assessment, out);
    let _ = queue!(
        out,
        Print(format!(
            "  Reason: {}\n",
            confirmation_reason_text(explanation)
        )),
    );

    if assessment.risk == RiskLevel::Danger {
        print_dangerous_action(assessment, out);
    }

    // Matched patterns + safe alternatives + diagnostics
    if !assessment.matched.is_empty() {
        let _ = queue!(out, Print("\n  Matched rules:\n"));
        for m in &assessment.matched {
            let source_label = pattern_source_label(m.pattern.source);
            let _ = queue!(
                out,
                SetForegroundColor(color),
                Print(format!(
                    "    [{}] {:?} | {}",
                    m.pattern.id, m.pattern.category, m.pattern.description
                )),
                ResetColor,
            );
            if let Some(alt) = &m.pattern.safe_alt {
                let _ = queue!(
                    out,
                    SetForegroundColor(Color::Green),
                    Print(format!("  ->  safe alternative: {alt}")),
                    ResetColor,
                );
            }
            let _ = queue!(
                out,
                SetForegroundColor(Color::DarkGrey),
                Print(format!(
                    "\n         matched: {:?}  source: {}",
                    m.matched_text, source_label
                )),
                ResetColor,
                Print("\n"),
            );
        }

        let decision_label = decision_source_label(assessment.decision_source());
        let _ = queue!(
            out,
            Print(format!("\n  Decision source: {decision_label}\n")),
        );
    }

    // Snapshots
    if !snapshots.is_empty() {
        let _ = queue!(out, Print("\n  Snapshots created:\n"));
        for s in snapshots {
            let _ = queue!(
                out,
                SetForegroundColor(Color::Cyan),
                Print(format!("    {} -> {}\n", s.plugin, s.snapshot_id)),
                ResetColor,
            );
        }
    }

    let _ = out.flush();
}

pub(super) fn prompt_danger<R: BufRead, W: Write>(input: &mut R, out: &mut W) -> bool {
    let _ = queue!(
        out,
        SetForegroundColor(Color::Red),
        SetAttribute(Attribute::Bold),
        Print("\n  Execute dangerous command? [y/N]: "),
        ResetColor,
    );
    let _ = out.flush();

    let mut line = String::new();
    if input.read_line(&mut line).is_err() {
        return false;
    }

    let answer = line.trim().to_ascii_lowercase();
    if answer == "y" || answer == "yes" {
        true
    } else {
        let _ = queue!(
            out,
            SetForegroundColor(Color::Yellow),
            Print("  Command cancelled.\n"),
            ResetColor,
        );
        let _ = out.flush();
        false
    }
}

/// Warn prompt: the user must type `y`/`yes` to proceed.
pub(super) fn prompt_warn<R: BufRead, W: Write>(input: &mut R, out: &mut W) -> bool {
    let _ = queue!(
        out,
        SetForegroundColor(Color::Yellow),
        Print("\n  Execute suspicious command? [y/N]: "),
        ResetColor,
    );
    let _ = out.flush();

    let mut line = String::new();
    if input.read_line(&mut line).is_err() {
        let _ = queue!(
            out,
            SetForegroundColor(Color::Yellow),
            Print("  Command cancelled.\n"),
            ResetColor,
        );
        let _ = out.flush();
        return false;
    }

    let answer = line.trim().to_ascii_lowercase();
    if answer == "y" || answer == "yes" {
        true
    } else {
        let _ = queue!(
            out,
            SetForegroundColor(Color::Yellow),
            Print("  Command cancelled.\n"),
            ResetColor,
        );
        let _ = out.flush();
        false
    }
}

// ── Command highlighting ───────────────────────────────────────────────────────
