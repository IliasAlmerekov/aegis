// crossterm TUI confirmation dialog

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use crossterm::{
    queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
};
use regex::Regex;

use crate::interceptor::RiskLevel;
use crate::interceptor::patterns::Pattern;
use crate::interceptor::scanner::Assessment;
use crate::snapshot::SnapshotRecord;

// ── Public API ─────────────────────────────────────────────────────────────────

/// Show the confirmation dialog for the given assessment and wait for user input.
///
/// Writes to stderr and reads from stdin.
///
/// Returns `true` if the command should proceed, `false` if it was denied or blocked.
pub fn show_confirmation(assessment: &Assessment, snapshots: &[SnapshotRecord]) -> bool {
    show_confirmation_with_input(
        assessment,
        snapshots,
        &mut io::stdin().lock(),
        &mut io::stderr(),
    )
}

/// Testable inner version — accepts any `BufRead` for input and `Write` for output.
///
/// - `Block`  → prints the reason and returns `false` immediately; no prompt shown.
/// - `Danger` → shows the dialog; user must type `yes` exactly to proceed.
/// - `Warn`   → shows the dialog; Enter (or any non-`n` input) approves; `n` denies.
/// - `Safe`   → auto-approves without rendering anything (should not normally reach here).
pub fn show_confirmation_with_input<R: BufRead, W: Write>(
    assessment: &Assessment,
    snapshots: &[SnapshotRecord],
    input: &mut R,
    output: &mut W,
) -> bool {
    match assessment.risk {
        RiskLevel::Block => {
            render_block(assessment, output);
            false
        }
        RiskLevel::Danger => {
            render_dialog(assessment, snapshots, output);
            prompt_danger(input, output)
        }
        RiskLevel::Warn => {
            render_dialog(assessment, snapshots, output);
            prompt_warn(input, output)
        }
        // Safe commands should not reach the dialog — auto-approve.
        _ => true,
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Print a hard-blocked message.  No prompt is shown; the command is rejected.
fn render_block<W: Write>(assessment: &Assessment, out: &mut W) {
    let _ = queue!(
        out,
        SetForegroundColor(Color::Red),
        SetAttribute(Attribute::Bold),
        Print("\n  AEGIS BLOCKED THIS COMMAND\n"),
        ResetColor,
    );

    print_command_line(assessment, out);

    if !assessment.matched.is_empty() {
        let _ = queue!(out, Print("\n  Reason:\n"));
        for p in &assessment.matched {
            let _ = queue!(
                out,
                SetForegroundColor(Color::Red),
                Print(format!("    [{}] {}\n", p.id, p.description)),
                ResetColor,
            );
        }
    }

    let _ = out.flush();
}

/// Print the interactive confirmation dialog (used for Danger and Warn).
fn render_dialog<W: Write>(assessment: &Assessment, snapshots: &[SnapshotRecord], out: &mut W) {
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

    // Matched patterns + safe alternatives
    if !assessment.matched.is_empty() {
        let _ = queue!(out, Print("\n  Matched rules:\n"));
        for p in &assessment.matched {
            let _ = queue!(
                out,
                SetForegroundColor(color),
                Print(format!("    [{}] {}", p.id, p.description)),
                ResetColor,
            );
            if let Some(alt) = &p.safe_alt {
                let _ = queue!(
                    out,
                    SetForegroundColor(Color::Green),
                    Print(format!("  ->  safe alternative: {alt}")),
                    ResetColor,
                );
            }
            let _ = queue!(out, Print("\n"));
        }
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
    let highlighted = build_highlighted_command(&assessment.command.raw, &assessment.matched);
    let _ = queue!(out, Print(highlighted));
    let _ = queue!(out, Print("\n"));
}

// ── Prompts ───────────────────────────────────────────────────────────────────

/// Danger prompt: the user must type `yes` exactly to proceed.
fn prompt_danger<R: BufRead, W: Write>(input: &mut R, out: &mut W) -> bool {
    let _ = queue!(
        out,
        SetForegroundColor(Color::Red),
        SetAttribute(Attribute::Bold),
        Print("\n  Type 'yes' to proceed, anything else cancels: "),
        ResetColor,
    );
    let _ = out.flush();

    let mut line = String::new();
    if input.read_line(&mut line).is_err() {
        return false;
    }

    if line.trim() == "yes" {
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

/// Warn prompt: Enter (or any non-`n` response) approves; `n` or `no` denies.
fn prompt_warn<R: BufRead, W: Write>(input: &mut R, out: &mut W) -> bool {
    let _ = queue!(
        out,
        SetForegroundColor(Color::Yellow),
        Print("\n  Press Enter to continue, 'n' to cancel: "),
        ResetColor,
    );
    let _ = out.flush();

    let mut line = String::new();
    if input.read_line(&mut line).is_err() {
        return false;
    }

    let answer = line.trim().to_ascii_lowercase();
    if answer == "n" || answer == "no" {
        let _ = queue!(
            out,
            SetForegroundColor(Color::Yellow),
            Print("  Command cancelled.\n"),
            ResetColor,
        );
        let _ = out.flush();
        false
    } else {
        true
    }
}

// ── Command highlighting ───────────────────────────────────────────────────────

/// Build a copy of `cmd` with matched regex ranges wrapped in ANSI bold-red codes.
///
/// Overlapping ranges are merged before highlighting so the output is well-formed.
fn build_highlighted_command(cmd: &str, patterns: &[Arc<Pattern>]) -> String {
    // Collect all match ranges from every pattern's regex.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for p in patterns {
        if let Ok(rx) = Regex::new(&p.pattern) {
            for m in rx.find_iter(cmd) {
                ranges.push((m.start(), m.end()));
            }
        }
    }

    if ranges.is_empty() {
        return cmd.to_string();
    }

    // Sort then merge overlapping / adjacent ranges.
    ranges.sort_unstable_by_key(|r| r.0);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
            continue;
        }
        merged.push((start, end));
    }

    // Reconstruct the string with ANSI bold-red around each highlighted span.
    let mut result = String::with_capacity(cmd.len() + merged.len() * 16);
    let mut pos = 0;
    for (start, end) in &merged {
        if *start > pos {
            result.push_str(&cmd[pos..*start]);
        }
        result.push_str("\x1b[1;31m");
        result.push_str(&cmd[*start..*end]);
        result.push_str("\x1b[0m");
        pos = *end;
    }
    if pos < cmd.len() {
        result.push_str(&cmd[pos..]);
    }
    result
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;
    use crate::interceptor::parser::Parser;
    use crate::interceptor::patterns::{Category, Pattern};

    // ── helpers ───────────────────────────────────────────────────────────────

    fn make_pattern(
        id: &'static str,
        risk: RiskLevel,
        pattern: &'static str,
        description: &'static str,
        safe_alt: Option<&'static str>,
    ) -> Arc<Pattern> {
        Arc::new(Pattern {
            id: Cow::Borrowed(id),
            category: Category::Filesystem,
            risk,
            pattern: Cow::Borrowed(pattern),
            description: Cow::Borrowed(description),
            safe_alt: safe_alt.map(Cow::Borrowed),
        })
    }

    fn make_assessment(cmd: &str, risk: RiskLevel, patterns: Vec<Arc<Pattern>>) -> Assessment {
        Assessment {
            risk,
            matched: patterns,
            command: Parser::parse(cmd),
        }
    }

    /// Strip ANSI escape sequences from a string so we can do plain-text assertions.
    fn strip_ansi(s: &str) -> String {
        let re = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
        re.replace_all(s, "").to_string()
    }

    // ── Block ─────────────────────────────────────────────────────────────────

    #[test]
    fn block_always_returns_false() {
        let p = make_pattern(
            "PS-006",
            RiskLevel::Block,
            "rm",
            "Deletes root filesystem",
            None,
        );
        let assessment = make_assessment("rm -rf /", RiskLevel::Block, vec![p]);

        // Even if the user somehow types "yes", Block must return false.
        let result =
            show_confirmation_with_input(&assessment, &[], &mut b"yes\n".as_ref(), &mut Vec::new());
        assert!(!result, "Block must always return false");
    }

    #[test]
    fn block_output_contains_reason() {
        let p = make_pattern(
            "PS-006",
            RiskLevel::Block,
            "rm",
            "Kills the root filesystem",
            None,
        );
        let assessment = make_assessment("rm -rf /", RiskLevel::Block, vec![p]);

        let mut output = Vec::new();
        show_confirmation_with_input(&assessment, &[], &mut b"".as_ref(), &mut output);

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("Kills the root filesystem"),
            "Block output must contain the pattern description; got:\n{text}"
        );
    }

    #[test]
    fn block_output_contains_command() {
        let p = make_pattern("PS-006", RiskLevel::Block, "rm", "Root delete", None);
        let assessment = make_assessment("rm -rf /", RiskLevel::Block, vec![p]);

        let mut output = Vec::new();
        show_confirmation_with_input(&assessment, &[], &mut b"".as_ref(), &mut output);

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("rm -rf /"),
            "Block output must contain the command; got:\n{text}"
        );
    }

    // ── Danger ────────────────────────────────────────────────────────────────

    #[test]
    fn danger_yes_approves() {
        let p = make_pattern(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive force delete",
            Some("git clean -fd"),
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let approved =
            show_confirmation_with_input(&assessment, &[], &mut b"yes\n".as_ref(), &mut Vec::new());
        assert!(approved, "'yes' must approve a Danger command");
    }

    #[test]
    fn danger_y_does_not_approve() {
        let p = make_pattern(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let denied =
            show_confirmation_with_input(&assessment, &[], &mut b"y\n".as_ref(), &mut Vec::new());
        assert!(!denied, "'y' must NOT approve — only 'yes' does");
    }

    #[test]
    fn danger_empty_does_not_approve() {
        let p = make_pattern(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let denied =
            show_confirmation_with_input(&assessment, &[], &mut b"\n".as_ref(), &mut Vec::new());
        assert!(!denied, "empty Enter must NOT approve a Danger command");
    }

    #[test]
    fn danger_anything_else_denies() {
        let p = make_pattern(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        for input in [b"no\n".as_ref(), b"nope\n".as_ref(), b"YES\n".as_ref()] {
            let denied =
                show_confirmation_with_input(&assessment, &[], &mut { input }, &mut Vec::new());
            assert!(!denied, "only 'yes' approves; other inputs must deny");
        }
    }

    // ── Warn ──────────────────────────────────────────────────────────────────

    #[test]
    fn warn_enter_approves() {
        let p = make_pattern("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

        let approved =
            show_confirmation_with_input(&assessment, &[], &mut b"\n".as_ref(), &mut Vec::new());
        assert!(approved, "Enter must approve a Warn command");
    }

    #[test]
    fn warn_n_denies() {
        let p = make_pattern("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

        let denied =
            show_confirmation_with_input(&assessment, &[], &mut b"n\n".as_ref(), &mut Vec::new());
        assert!(!denied, "'n' must deny a Warn command");
    }

    #[test]
    fn warn_no_denies() {
        let p = make_pattern("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

        let denied =
            show_confirmation_with_input(&assessment, &[], &mut b"no\n".as_ref(), &mut Vec::new());
        assert!(!denied, "'no' must deny a Warn command");
    }

    #[test]
    fn warn_any_other_input_approves() {
        let p = make_pattern("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
        let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

        let approved =
            show_confirmation_with_input(&assessment, &[], &mut b"ok\n".as_ref(), &mut Vec::new());
        assert!(approved, "non-n input must approve a Warn command");
    }

    // ── Dialog content ────────────────────────────────────────────────────────

    #[test]
    fn danger_output_contains_command() {
        let p = make_pattern(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive delete",
            None,
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let mut output = Vec::new();
        show_confirmation_with_input(&assessment, &[], &mut b"no\n".as_ref(), &mut output);

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("rm -rf /home/user"),
            "dialog must show the full command; got:\n{text}"
        );
    }

    #[test]
    fn danger_output_contains_pattern_description() {
        let p = make_pattern(
            "FS-001",
            RiskLevel::Danger,
            r"rm\s+",
            "Recursive force delete",
            Some("git clean -fd"),
        );
        let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

        let mut output = Vec::new();
        show_confirmation_with_input(&assessment, &[], &mut b"no\n".as_ref(), &mut output);

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
    fn dialog_shows_snapshot_records() {
        let p = make_pattern(
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
        show_confirmation_with_input(&assessment, &[snap], &mut b"no\n".as_ref(), &mut output);

        let text = strip_ansi(&String::from_utf8_lossy(&output));
        assert!(
            text.contains("git") && text.contains("stash@{0}"),
            "dialog must list snapshot records; got:\n{text}"
        );
    }

    // ── Highlighting ──────────────────────────────────────────────────────────

    #[test]
    fn highlighting_wraps_match_in_ansi() {
        let p = make_pattern("FS-001", RiskLevel::Danger, r"rm\s+-rf", "desc", None);
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
    fn highlighting_no_match_returns_unchanged() {
        let p = make_pattern("FS-001", RiskLevel::Danger, r"terraform", "desc", None);
        let patterns = vec![p];
        let cmd = "echo hello";
        let result = build_highlighted_command(cmd, &patterns);
        assert_eq!(result, cmd, "no match should return the command unchanged");
    }

    #[test]
    fn highlighting_merges_overlapping_ranges() {
        // Two patterns that overlap on "rm -rf"
        let p1 = make_pattern("A", RiskLevel::Danger, r"rm\s+-rf /", "desc", None);
        let p2 = make_pattern("B", RiskLevel::Danger, r"rm\s+-rf", "desc", None);
        let result = build_highlighted_command("rm -rf /home", &[p1, p2]);
        // Should not have double ANSI codes inside the overlap.
        let opens = result.matches("\x1b[1;31m").count();
        assert_eq!(
            opens, 1,
            "overlapping ranges must be merged into one highlight span"
        );
    }
}
