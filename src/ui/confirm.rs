// crossterm TUI confirmation dialog

use std::io::{self, BufRead, Write};

use crossterm::{
    queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
};

use crate::explanation::CommandExplanation;
use crate::interceptor::RiskLevel;
use crate::interceptor::patterns::PatternSource;
#[cfg(test)]
use crate::interceptor::scanner::MatchResult;
use crate::interceptor::scanner::{Assessment, DecisionSource, HighlightRange};
use crate::snapshot::SnapshotRecord;

// ── Public API ─────────────────────────────────────────────────────────────────

/// Show the confirmation dialog for the given assessment and wait for user input.
///
/// Writes to stderr and reads from stdin.
///
/// Returns `true` if the command should proceed, `false` if it was denied or blocked.
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

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Print a hard-blocked message.  No prompt is shown; the command is rejected.
fn render_block<W: Write>(assessment: &Assessment, explanation: &CommandExplanation, out: &mut W) {
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
fn render_noninteractive_denial<W: Write>(
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

fn render_policy_block<W: Write>(
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

/// Print the interactive confirmation dialog (used for Danger and Warn).
fn render_dialog<W: Write>(
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

/// Print the `  Command:` section with dangerous fragments highlighted in red.
fn print_command_line<W: Write>(assessment: &Assessment, out: &mut W) {
    let _ = queue!(out, Print("  Command:\n    "));
    let highlighted = build_highlighted_command_from_ranges(
        &assessment.command.raw,
        &assessment.highlight_ranges,
    );
    let _ = queue!(out, Print(highlighted));
    let _ = queue!(out, Print("\n"));
}

fn print_dangerous_action<W: Write>(assessment: &Assessment, out: &mut W) {
    let action = dangerous_action_text(assessment);
    let _ = queue!(out, Print("  Dangerous action:\n    "));
    let _ = queue!(out, Print(action));
    let _ = queue!(out, Print("\n"));
}

// ── Prompts ───────────────────────────────────────────────────────────────────

/// Danger prompt: the user must type `y`/`yes` to proceed.
fn prompt_danger<R: BufRead, W: Write>(input: &mut R, out: &mut W) -> bool {
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
fn prompt_warn<R: BufRead, W: Write>(input: &mut R, out: &mut W) -> bool {
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

/// Build a copy of `cmd` with scanner-provided match fragments wrapped in ANSI bold-red codes.
#[cfg(test)]
fn build_highlighted_command(cmd: &str, matches: &[MatchResult]) -> String {
    let ranges = sorted_highlight_ranges_for_tests(cmd, matches);
    build_highlighted_command_from_ranges(cmd, &ranges)
}

/// Build a copy of `cmd` with already-sorted highlight ranges wrapped in ANSI bold-red codes.
///
/// Overlapping ranges are merged before highlighting so the output is well-formed.
fn build_highlighted_command_from_ranges(cmd: &str, ranges: &[HighlightRange]) -> String {
    if ranges.is_empty() {
        return cmd.to_string();
    }

    // Reconstruct the string with ANSI bold-red around each highlighted span.
    let mut result = String::with_capacity(cmd.len() + ranges.len() * 9);
    let mut pos = 0;
    for range in ranges {
        if range.start > pos {
            result.push_str(&cmd[pos..range.start]);
        }
        result.push_str("\x1b[1;31m");
        result.push_str(&cmd[range.start..range.end]);
        result.push_str("\x1b[0m");
        pos = range.end;
    }
    if pos < cmd.len() {
        result.push_str(&cmd[pos..]);
    }
    result
}

#[cfg(test)]
fn sorted_highlight_ranges_for_tests(cmd: &str, matches: &[MatchResult]) -> Vec<HighlightRange> {
    let mut ranges = Vec::with_capacity(matches.len());

    for matched in matches {
        if let Some(range) = matched.highlight_range
            && cmd.get(range.start..range.end).is_some()
        {
            ranges.push(range);
            continue;
        }

        let fragment = matched.matched_text.trim();
        if fragment.is_empty() {
            continue;
        }

        if let Some(start) = cmd.find(fragment) {
            ranges.push(HighlightRange {
                start,
                end: start + fragment.len(),
            });
        }
    }

    if ranges.len() <= 1 {
        return ranges;
    }

    ranges.sort_unstable_by_key(|range| range.start);
    let mut merged: Vec<HighlightRange> = Vec::with_capacity(ranges.len());

    for range in ranges {
        if let Some(last) = merged.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
            continue;
        }

        merged.push(range);
    }

    merged
}

fn dangerous_action_text(assessment: &Assessment) -> String {
    let Some(fragment) = assessment
        .matched
        .iter()
        .filter_map(|m| {
            let trimmed = m.matched_text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .max_by_key(|fragment| fragment.len())
    else {
        return build_highlighted_command_from_ranges(
            &assessment.command.raw,
            &assessment.highlight_ranges,
        );
    };

    format!("\x1b[1;31m{fragment}\x1b[0m")
}

fn confirmation_reason_text(explanation: &CommandExplanation) -> String {
    match (
        explanation.policy.rationale,
        explanation.context.allowlist_match.as_ref(),
    ) {
        (crate::decision::PolicyRationale::RequiresConfirmation, Some(rule)) => format!(
            "requires confirmation despite matching allowlist rule: {}",
            rule.reason
        ),
        (crate::decision::PolicyRationale::RequiresConfirmation, None) => {
            explanation.policy.concise_reason_label().to_string()
        }
        (crate::decision::PolicyRationale::AllowlistOverride, Some(rule)) => {
            format!("allowlist override approved: {}", rule.reason)
        }
        (crate::decision::PolicyRationale::AllowlistOverride, None) => {
            explanation.policy.concise_reason_label().to_string()
        }
        (crate::decision::PolicyRationale::SafeCommand, _)
        | (crate::decision::PolicyRationale::AuditMode, _) => {
            explanation.policy.concise_reason_label().to_string()
        }
        (crate::decision::PolicyRationale::IntrinsicRiskBlock, _)
        | (crate::decision::PolicyRationale::ProtectCiPolicy, _)
        | (crate::decision::PolicyRationale::StrictPolicy, _) => {
            explanation.policy.concise_reason_label().to_string()
        }
    }
}

fn block_reason_text(explanation: &CommandExplanation) -> &'static str {
    match explanation
        .policy
        .block_reason
        .or_else(|| explanation.policy.rationale.block_reason())
    {
        Some(crate::decision::BlockReason::IntrinsicRiskBlock) => {
            "blocked by an explicit block-level pattern"
        }
        Some(crate::decision::BlockReason::StrictPolicy) => {
            "blocked by strict mode (non-safe commands require an allowlist override)"
        }
        Some(crate::decision::BlockReason::ProtectCiPolicy) => {
            "blocked by CI policy (Protect mode + ci_policy=Block)"
        }
        None => "blocked by policy",
    }
}

// ── Label helpers ─────────────────────────────────────────────────────────────

fn pattern_source_label(source: PatternSource) -> &'static str {
    match source {
        PatternSource::Builtin => "built-in",
        PatternSource::Custom => "custom",
    }
}

fn decision_source_label(source: DecisionSource) -> &'static str {
    match source {
        DecisionSource::BuiltinPattern => "built-in pattern",
        DecisionSource::CustomPattern => "custom pattern",
        DecisionSource::Fallback => "fallback (no patterns matched)",
    }
}

// ── /dev/tty UI helpers (watch mode) ─────────────────────────────────────────

/// The fail-closed decision when `/dev/tty` is unavailable.
///
/// Only `Safe` commands are approved without a TTY; everything else is
/// denied or blocked.  Exported so that callers can emit the correct result
/// frame without duplicating the policy.
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
    use std::fs::OpenOptions;

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
    use std::fs::OpenOptions;

    if let Ok(mut tty) = OpenOptions::new().write(true).open("/dev/tty") {
        render_policy_block(assessment, explanation, &mut tty);
    }
}

/// Show an intrinsic-block notice (RiskLevel::Block pattern) via `/dev/tty`.
///
/// Uses the same `render_block` path as the shell-wrapper mode but routes
/// output to the tty device.  If `/dev/tty` cannot be opened, silent.
pub fn show_block_via_tty(assessment: &Assessment, explanation: &CommandExplanation) {
    use std::fs::OpenOptions;

    if let Ok(mut tty) = OpenOptions::new().write(true).open("/dev/tty") {
        render_block(assessment, explanation, &mut tty);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use std::sync::Arc;

    use super::*;
    use crate::config::{AllowlistSourceLayer, Mode};
    use crate::decision::{BlockReason, ExecutionTransport, PolicyAction, PolicyRationale};
    use crate::explanation::{
        AllowlistExplanation, CommandExplanation, ExecutionContextExplanation, PolicyExplanation,
        ScanExplanation,
    };
    use crate::interceptor::parser::Parser;
    use crate::interceptor::patterns::{Category, Pattern, PatternSource};
    use crate::interceptor::scanner::MatchResult;

    // ── helpers ───────────────────────────────────────────────────────────────

    fn make_match(
        id: &'static str,
        risk: RiskLevel,
        pattern: &'static str,
        description: &'static str,
        safe_alt: Option<&'static str>,
    ) -> MatchResult {
        MatchResult {
            pattern: Arc::new(Pattern {
                id: Cow::Borrowed(id),
                category: Category::Filesystem,
                risk,
                pattern: Cow::Borrowed(pattern),
                description: Cow::Borrowed(description),
                safe_alt: safe_alt.map(Cow::Borrowed),
                source: PatternSource::Builtin,
            }),
            matched_text: String::new(),
            highlight_range: None,
        }
    }

    fn make_match_with_text(
        id: &'static str,
        risk: RiskLevel,
        pattern: &'static str,
        description: &'static str,
        matched_text: &'static str,
    ) -> MatchResult {
        MatchResult {
            pattern: Arc::new(Pattern {
                id: Cow::Borrowed(id),
                category: Category::Filesystem,
                risk,
                pattern: Cow::Borrowed(pattern),
                description: Cow::Borrowed(description),
                safe_alt: None,
                source: PatternSource::Builtin,
            }),
            matched_text: matched_text.to_string(),
            highlight_range: None,
        }
    }

    fn make_match_with_text_and_range(
        id: &'static str,
        risk: RiskLevel,
        pattern: &'static str,
        description: &'static str,
        matched_text: &'static str,
        start: usize,
    ) -> MatchResult {
        MatchResult {
            pattern: Arc::new(Pattern {
                id: Cow::Borrowed(id),
                category: Category::Filesystem,
                risk,
                pattern: Cow::Borrowed(pattern),
                description: Cow::Borrowed(description),
                safe_alt: None,
                source: PatternSource::Builtin,
            }),
            matched_text: matched_text.to_string(),
            highlight_range: Some(HighlightRange {
                start,
                end: start + matched_text.len(),
            }),
        }
    }

    fn make_assessment(cmd: &str, risk: RiskLevel, matches: Vec<MatchResult>) -> Assessment {
        Assessment {
            risk,
            highlight_ranges: sorted_highlight_ranges_for_tests(cmd, &matches),
            matched: matches,
            command: Parser::parse(cmd),
        }
    }

    fn make_explanation(
        assessment: &Assessment,
        rationale: PolicyRationale,
        block_reason: Option<BlockReason>,
        allowlist_match: Option<AllowlistExplanation>,
    ) -> CommandExplanation {
        CommandExplanation {
            scan: ScanExplanation {
                highest_risk: assessment.risk,
                decision_source: assessment.decision_source(),
                matched_patterns: Vec::new(),
            },
            policy: PolicyExplanation {
                action: match block_reason {
                    Some(_) => PolicyAction::Block,
                    None => PolicyAction::Prompt,
                },
                rationale,
                requires_confirmation: block_reason.is_none(),
                snapshots_required: assessment.risk == RiskLevel::Danger,
                allowlist_effective: false,
                block_reason,
            },
            context: ExecutionContextExplanation {
                mode: Mode::Protect,
                transport: ExecutionTransport::Shell,
                ci_detected: false,
                allowlist_match,
                applicable_snapshot_plugins: Vec::new(),
            },
            outcome: None,
        }
    }

    fn default_explanation_for_assessment(assessment: &Assessment) -> CommandExplanation {
        match assessment.risk {
            RiskLevel::Safe => {
                make_explanation(assessment, PolicyRationale::SafeCommand, None, None)
            }
            RiskLevel::Warn | RiskLevel::Danger => make_explanation(
                assessment,
                PolicyRationale::RequiresConfirmation,
                None,
                None,
            ),
            RiskLevel::Block => make_explanation(
                assessment,
                PolicyRationale::IntrinsicRiskBlock,
                Some(BlockReason::IntrinsicRiskBlock),
                None,
            ),
        }
    }

    /// Strip ANSI escape sequences from a string so we can do plain-text assertions.
    fn strip_ansi(s: &str) -> String {
        let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
        re.replace_all(s, "").to_string()
    }

    // ── Block ─────────────────────────────────────────────────────────────────

    #[test]
    fn block_always_returns_false() {
        let p = make_match(
            "PS-006",
            RiskLevel::Block,
            "rm",
            "Deletes root filesystem",
            None,
        );
        let assessment = make_assessment("rm -rf /", RiskLevel::Block, vec![p]);

        // Even if the user somehow types "yes", Block must return false.
        let result = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"yes\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(!result, "Block must always return false");
    }

    #[test]
    fn block_output_contains_reason() {
        let p = make_match(
            "PS-006",
            RiskLevel::Block,
            "rm",
            "Kills the root filesystem",
            None,
        );
        let assessment = make_assessment("rm -rf /", RiskLevel::Block, vec![p]);

        let mut output = Vec::new();
        show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"".as_ref(),
            &mut output,
        );

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("Kills the root filesystem"),
            "Block output must contain the pattern description; got:\n{text}"
        );
    }

    #[test]
    fn block_output_contains_command() {
        let p = make_match("PS-006", RiskLevel::Block, "rm", "Root delete", None);
        let assessment = make_assessment("rm -rf /", RiskLevel::Block, vec![p]);

        let mut output = Vec::new();
        show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"".as_ref(),
            &mut output,
        );

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("rm -rf /"),
            "Block output must contain the command; got:\n{text}"
        );
    }

    // ── Danger ────────────────────────────────────────────────────────────────

    #[test]
    fn danger_yes_approves() {
        let p = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive force delete",
            Some("git clean -fd"),
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let approved = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"yes\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(approved, "'yes' must approve a Danger command");
    }

    #[test]
    fn danger_y_approves() {
        let p = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let denied = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"y\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(denied, "'y' must approve a Danger command");
    }

    #[test]
    fn danger_uppercase_y_approves() {
        let p = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let approved = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"Y\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(approved, "'Y' must approve a Danger command");
    }

    #[test]
    fn danger_empty_does_not_approve() {
        let p = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let denied = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(!denied, "empty Enter must NOT approve a Danger command");
    }

    #[test]
    fn danger_anything_else_denies() {
        let p = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        for input in [b"nope\n".as_ref(), b"ok\n".as_ref(), b"cancel\n".as_ref()] {
            let denied = show_confirmation_with_input(
                &assessment,
                &default_explanation_for_assessment(&assessment),
                &[],
                true,
                &mut { input },
                &mut Vec::new(),
            );
            assert!(!denied, "only 'yes' approves; other inputs must deny");
        }
    }

    #[test]
    fn danger_no_denies() {
        let p = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let denied = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"no\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(!denied, "'no' must deny a Danger command");
    }

    #[test]
    fn danger_uppercase_no_denies() {
        let p = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let denied = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"NO\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(!denied, "'NO' must deny a Danger command");
    }

    // ── Warn ──────────────────────────────────────────────────────────────────

    #[test]
    fn warn_enter_denies() {
        let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

        let denied = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(!denied, "Enter must deny a Warn command");
    }

    #[test]
    fn warn_y_approves() {
        let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

        let approved = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"y\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(approved, "'y' must approve a Warn command");
    }

    #[test]
    fn warn_uppercase_y_approves() {
        let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

        let approved = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"Y\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(approved, "'Y' must approve a Warn command");
    }

    #[test]
    fn warn_yes_approves_after_trim() {
        let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

        let approved = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b" yes \n".as_ref(),
            &mut Vec::new(),
        );
        assert!(approved, "' yes ' must approve a Warn command");
    }

    #[test]
    fn warn_n_denies() {
        let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

        let denied = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"n\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(!denied, "'n' must deny a Warn command");
    }

    #[test]
    fn warn_no_denies() {
        let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

        let denied = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"no\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(!denied, "'no' must deny a Warn command");
    }

    #[test]
    fn warn_any_other_input_denies() {
        let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

        for input in [b"maybe\n".as_ref(), b"1\n".as_ref(), b"ok\n".as_ref()] {
            let denied = show_confirmation_with_input(
                &assessment,
                &default_explanation_for_assessment(&assessment),
                &[],
                true,
                &mut { input },
                &mut Vec::new(),
            );
            assert!(!denied, "unexpected input must deny a Warn command");
        }
    }

    #[test]
    fn warn_output_contains_explicit_yes_no_prompt() {
        let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

        let mut output = Vec::new();
        show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"no\n".as_ref(),
            &mut output,
        );

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("Execute suspicious command? [y/N]:"),
            "Warn dialog must use the explicit yes/no prompt; got:\n{text}"
        );
    }

    // ── Dialog content ────────────────────────────────────────────────────────

    #[test]
    fn danger_output_contains_command() {
        let p = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let mut output = Vec::new();
        show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"no\n".as_ref(),
            &mut output,
        );

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("rm -rf /home/user"),
            "dialog must show the full command; got:\n{text}"
        );
    }

    #[test]
    fn danger_output_contains_pattern_description() {
        let p = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive force delete",
            Some("git clean -fd"),
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let mut output = Vec::new();
        show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"no\n".as_ref(),
            &mut output,
        );

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("Recursive force delete"),
            "dialog must show pattern description; got:\n{text}"
        );
        assert!(
            text.contains("git clean -fd"),
            "dialog must show safe_alt suggestion; got:\n{text}"
        );
    }

    #[test]
    fn danger_output_contains_dangerous_action_section() {
        let p = make_match_with_text(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+-rf\s+/var/tmp/cache",
            "Recursive force delete",
            "rm -rf /var/tmp/cache",
        );
        let assessment = make_assessment("sudo rm -rf /var/tmp/cache", RiskLevel::Danger, vec![p]);

        let mut output = Vec::new();
        show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"no\n".as_ref(),
            &mut output,
        );

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("Dangerous action:"),
            "dialog must show a dedicated dangerous action section; got:\n{text}"
        );
        assert!(
            text.contains("rm -rf /var/tmp/cache"),
            "dialog must show the dangerous action fragment; got:\n{text}"
        );
    }

    #[test]
    fn danger_output_contains_explicit_yes_no_prompt() {
        let p = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let mut output = Vec::new();
        show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut b"no\n".as_ref(),
            &mut output,
        );

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("Execute dangerous command? [y/N]:"),
            "dialog must use the explicit yes/no prompt; got:\n{text}"
        );
    }

    #[test]
    fn dialog_shows_snapshot_records() {
        let p = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);
        let snap = SnapshotRecord {
            plugin: "git",
            snapshot_id: "stash@{0}".to_string(),
        };

        let mut output = Vec::new();
        show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[snap],
            true,
            &mut b"no\n".as_ref(),
            &mut output,
        );

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("git") && text.contains("stash@{0}"),
            "dialog must list snapshot records; got:\n{text}"
        );
    }

    // ── Non-interactive mode ──────────────────────────────────────────────────
    //
    // When stdin is not a TTY (CI, pipes, agent runners) Aegis must fail-closed:
    // any command that would trigger a prompt is denied without asking.
    //
    // Rule table:
    //   Safe   → auto-approved  (same as interactive)
    //   Warn   → denied         (no human present to confirm)
    //   Danger → denied         (no human present to confirm)
    //   Block  → denied         (same as interactive — always hard-stopped)

    #[test]
    fn noninteractive_warn_is_denied() {
        let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

        // Even with an "approving" response on stdin, is_interactive=false must deny.
        let result = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            false,
            &mut b"\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(!result, "Warn must be denied in non-interactive mode");
    }

    #[test]
    fn noninteractive_danger_is_denied() {
        let p = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let result = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            false,
            &mut b"yes\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(!result, "Danger must be denied in non-interactive mode");
    }

    #[test]
    fn noninteractive_block_is_denied() {
        let p = make_match("PS-006", RiskLevel::Block, "rm", "Root delete", None);
        let assessment = make_assessment("rm -rf /", RiskLevel::Block, vec![p]);

        let result = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            false,
            &mut b"yes\n".as_ref(),
            &mut Vec::new(),
        );
        assert!(!result, "Block must be denied in non-interactive mode");
    }

    #[test]
    fn noninteractive_output_explains_denial() {
        let p = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let mut output = Vec::new();
        show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            false,
            &mut b"yes\n".as_ref(),
            &mut output,
        );

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("non-interactive"),
            "non-interactive denial must mention 'non-interactive'; got:\n{text}"
        );
        assert!(
            text.contains("allowlist"),
            "non-interactive denial must mention 'allowlist' as the escape hatch; got:\n{text}"
        );
    }

    #[test]
    fn noninteractive_safe_is_still_approved() {
        // Safe commands must never be blocked regardless of TTY state.
        let assessment = make_assessment("ls -la", RiskLevel::Safe, vec![]);
        let result = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            false,
            &mut b"".as_ref(),
            &mut Vec::new(),
        );
        assert!(
            result,
            "Safe commands must be approved even in non-interactive mode"
        );
    }

    #[test]
    fn render_policy_block_mentions_reason() {
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![]);
        let explanation = make_explanation(
            &assessment,
            PolicyRationale::StrictPolicy,
            Some(BlockReason::StrictPolicy),
            None,
        );
        let mut output = Vec::new();

        render_policy_block(&assessment, &explanation, &mut output);

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("AEGIS POLICY BLOCKED THIS COMMAND"),
            "policy block output must contain the headline; got:\n{text}"
        );
        assert!(
            text.contains(
                "Reason: blocked by strict mode (non-safe commands require an allowlist override)"
            ),
            "policy block output must contain the reason; got:\n{text}"
        );
    }

    #[test]
    fn confirmation_renders_policy_reason_from_explanation() {
        let matched = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /tmp/demo", RiskLevel::Danger, vec![matched]);
        let explanation = make_explanation(
            &assessment,
            PolicyRationale::RequiresConfirmation,
            None,
            Some(AllowlistExplanation {
                pattern: "rm -rf /tmp/*".to_string(),
                reason: "temporary workspace cleanup".to_string(),
                source_layer: AllowlistSourceLayer::Project,
            }),
        );

        let mut output = Vec::new();
        show_confirmation_with_input(
            &assessment,
            &explanation,
            &[],
            true,
            &mut b"no\n".as_ref(),
            &mut output,
        );

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("Reason: requires confirmation despite matching allowlist rule: temporary workspace cleanup"),
            "confirmation output must use the canonical explanation reason; got:\n{text}"
        );
    }

    #[test]
    fn policy_block_renders_from_canonical_block_reason() {
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![]);
        let explanation = make_explanation(
            &assessment,
            PolicyRationale::StrictPolicy,
            Some(BlockReason::StrictPolicy),
            None,
        );
        let mut output = Vec::new();

        render_policy_block(&assessment, &explanation, &mut output);

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains(
                "Reason: blocked by strict mode (non-safe commands require an allowlist override)"
            ),
            "policy block output must use the canonical block reason; got:\n{text}"
        );
    }

    #[test]
    fn policy_block_renders_ci_policy_reason_from_explanation() {
        let assessment = make_assessment(
            "terraform destroy -target=module.prod.api",
            RiskLevel::Danger,
            vec![],
        );
        let explanation = make_explanation(
            &assessment,
            PolicyRationale::ProtectCiPolicy,
            Some(BlockReason::ProtectCiPolicy),
            None,
        );
        let mut output = Vec::new();

        render_policy_block(&assessment, &explanation, &mut output);

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("Reason: blocked by CI policy (Protect mode + ci_policy=Block)"),
            "policy block output must use the CI policy reason from explanation; got:\n{text}"
        );
    }

    #[test]
    fn ui_rendering_does_not_need_to_synthesize_missing_optional_sections() {
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![]);
        let explanation = make_explanation(
            &assessment,
            PolicyRationale::RequiresConfirmation,
            None,
            None,
        );
        let mut output = Vec::new();

        let denied = show_confirmation_with_input(
            &assessment,
            &explanation,
            &[],
            true,
            &mut b"no\n".as_ref(),
            &mut output,
        );

        assert!(!denied);

        let rendered = strip_ansi(&String::from_utf8(output).expect("ui output should be utf8"));
        assert!(
            rendered.contains("Execute suspicious command? [y/N]:"),
            "test must exercise the interactive confirmation dialog path; got:\n{rendered}"
        );
        assert!(
            rendered.contains("Reason: requires confirmation"),
            "ui should render the canonical concise policy reason; got:\n{rendered}"
        );
        assert!(
            !rendered.contains("requires explicit confirmation"),
            "ui should not synthesize an alternative reason label when optional sections are absent; got:\n{rendered}"
        );
        assert!(
            !rendered.contains("Snapshots created:"),
            "ui should not synthesize missing runtime outcome sections; got:\n{rendered}"
        );
        assert!(
            !rendered.contains("allowlist rule"),
            "ui should not synthesize a missing allowlist section; got:\n{rendered}"
        );
        assert!(
            !rendered.contains("outcome"),
            "ui should not synthesize a missing runtime outcome section; got:\n{rendered}"
        );
    }

    // ── Highlighting ──────────────────────────────────────────────────────────

    #[test]
    fn highlighting_wraps_match_in_ansi() {
        let p = make_match_with_text("FS-001", RiskLevel::Danger, r"rm\s+-rf", "desc", "rm -rf");
        let patterns = vec![p];
        let result = build_highlighted_command("rm -rf /home", &patterns);
        assert!(
            result.contains("\x1b[1;31m"),
            "highlighted output must contain bold-red ANSI code"
        );
        assert!(
            result.contains("rm -rf"),
            "the matched fragment must appear in the output"
        );
    }

    #[test]
    fn highlighting_uses_scanner_matched_text_without_recompiling_regex() {
        let p = make_match_with_text("FS-001", RiskLevel::Danger, "(", "desc", "rm -rf");
        let result = build_highlighted_command("rm -rf /home", &[p]);

        assert!(
            result.contains("\x1b[1;31mrm -rf\x1b[0m"),
            "highlighting must use scanner-provided match metadata even when the pattern regex is unusable in the UI"
        );
    }

    #[test]
    fn highlighting_does_not_duplicate_single_match_across_repeated_fragments() {
        let cmd = "rm -rf /tmp/one && echo safe && rm -rf /tmp/two";
        let start = cmd.rfind("rm -rf").unwrap();
        let p = make_match_with_text_and_range(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+-rf",
            "desc",
            "rm -rf",
            start,
        );

        let result = build_highlighted_command(cmd, &[p]);

        assert_eq!(
            result.matches("\x1b[1;31m").count(),
            1,
            "a single scanner match must highlight one concrete command span, not every identical fragment in the command"
        );
    }

    #[test]
    fn highlighting_large_heredoc_like_input_marks_single_match_once() {
        let repeated_line = "rm -rf /tmp/cache\n";
        let mut cmd = String::from("cat <<'EOF'\n");
        for _ in 0..256 {
            cmd.push_str("echo keep\n");
        }
        for _ in 0..128 {
            cmd.push_str(repeated_line);
        }
        cmd.push_str("EOF");
        let start = cmd.rfind("rm -rf /tmp/cache").unwrap();
        let p = make_match_with_text_and_range(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+-rf",
            "desc",
            "rm -rf /tmp/cache",
            start,
        );

        let result = build_highlighted_command(&cmd, &[p]);

        assert_eq!(
            result.matches("\x1b[1;31m").count(),
            1,
            "large heredoc-like inputs must still honor the scanner's concrete match span instead of highlighting every repeated copy"
        );
    }

    #[test]
    fn highlighting_no_match_returns_unchanged() {
        let p = make_match_with_text(
            "FS-001",
            RiskLevel::Danger,
            r"terraform",
            "desc",
            "terraform",
        );
        let patterns = vec![p];
        let cmd = "echo hello";
        let result = build_highlighted_command(cmd, &patterns);
        assert_eq!(result, cmd, "no match should return the command unchanged");
    }

    #[test]
    fn highlighting_merges_overlapping_ranges() {
        // Two patterns that overlap on "rm -rf"
        let p1 = make_match_with_text("A", RiskLevel::Danger, r"rm\s+-rf /", "desc", "rm -rf /");
        let p2 = make_match_with_text("B", RiskLevel::Danger, r"rm\s+-rf", "desc", "rm -rf");
        let result = build_highlighted_command("rm -rf /home", &[p1, p2]);
        // Should not have double ANSI codes inside the overlap.
        let opens = result.matches("\x1b[1;31m").count();
        assert_eq!(
            opens, 1,
            "overlapping ranges must be merged into one highlight span"
        );
    }

    // ── /dev/tty helpers ──────────────────────────────────────────────────────

    #[test]
    fn tty_unavailable_safe_is_approved() {
        let assessment = make_assessment("ls -la", RiskLevel::Safe, vec![]);
        assert!(
            tty_unavailable_decision(&assessment),
            "Safe must be approved when /dev/tty is unavailable"
        );
    }

    #[test]
    fn tty_unavailable_warn_is_denied() {
        let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);
        assert!(
            !tty_unavailable_decision(&assessment),
            "Warn must be denied when /dev/tty is unavailable"
        );
    }

    #[test]
    fn tty_unavailable_danger_is_denied() {
        let p = make_match(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);
        assert!(
            !tty_unavailable_decision(&assessment),
            "Danger must be denied when /dev/tty is unavailable"
        );
    }

    #[test]
    fn tty_unavailable_block_is_denied() {
        let p = make_match("PS-006", RiskLevel::Block, "rm", "Root delete", None);
        let assessment = make_assessment("rm -rf /", RiskLevel::Block, vec![p]);
        assert!(
            !tty_unavailable_decision(&assessment),
            "Block must be denied when /dev/tty is unavailable"
        );
    }
}
