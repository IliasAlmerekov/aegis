use std::io::Write;

use crossterm::{queue, style::Print};

use crate::explanation::CommandExplanation;
use crate::interceptor::patterns::PatternSource;
#[cfg(test)]
use crate::interceptor::scanner::MatchResult;
use crate::interceptor::scanner::{Assessment, DecisionSource, HighlightRange};

pub(super) fn print_command_line<W: Write>(assessment: &Assessment, out: &mut W) {
    let _ = queue!(out, Print("  Command:\n    "));
    let highlighted = build_highlighted_command_from_ranges(
        &assessment.command.raw,
        &assessment.highlight_ranges,
    );
    let _ = queue!(out, Print(highlighted));
    let _ = queue!(out, Print("\n"));
}

pub(super) fn print_dangerous_action<W: Write>(assessment: &Assessment, out: &mut W) {
    let action = dangerous_action_text(assessment);
    let _ = queue!(out, Print("  Dangerous action:\n    "));
    let _ = queue!(out, Print(action));
    let _ = queue!(out, Print("\n"));
}

#[cfg(test)]
pub(super) fn build_highlighted_command(cmd: &str, matches: &[MatchResult]) -> String {
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
pub(super) fn sorted_highlight_ranges_for_tests(
    cmd: &str,
    matches: &[MatchResult],
) -> Vec<HighlightRange> {
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

pub(super) fn confirmation_reason_text(explanation: &CommandExplanation) -> String {
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

pub(super) fn block_reason_text(explanation: &CommandExplanation) -> &'static str {
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

pub(super) fn pattern_source_label(source: PatternSource) -> &'static str {
    match source {
        PatternSource::Builtin => "built-in",
        PatternSource::Custom => "custom",
    }
}

pub(super) fn decision_source_label(source: DecisionSource) -> &'static str {
    match source {
        DecisionSource::BuiltinPattern => "built-in pattern",
        DecisionSource::CustomPattern => "custom pattern",
        DecisionSource::Fallback => "fallback (no patterns matched)",
    }
}
