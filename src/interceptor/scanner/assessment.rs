use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::interceptor::RiskLevel;
use crate::interceptor::nested::RecursiveScanLimit;
use crate::interceptor::parser::ParsedCommand;
use crate::interceptor::patterns::{Category, Pattern, PatternSource};

use super::{HighlightRange, Scanner, highlighting, pipeline_semantics, recursive};

/// A single pattern match with the actual text fragment that triggered it.
#[derive(Debug, Clone)]
pub struct MatchResult {
    pub pattern: Arc<Pattern>,
    /// The substring of the scanned text that the pattern's regex matched.
    pub matched_text: String,
    /// The concrete span in the original command suitable for confirmation UI highlighting.
    pub highlight_range: Option<HighlightRange>,
}

/// What ultimately caused the final interception decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionSource {
    /// Matched one or more built-in patterns compiled into the binary.
    BuiltinPattern,
    /// Matched one or more user-defined patterns from aegis.toml.
    CustomPattern,
    /// No patterns matched; the command was assessed Safe by default.
    Fallback,
}

/// The result of assessing a shell command through the full scanner pipeline.
pub struct Assessment {
    /// The highest `RiskLevel` among all matched patterns (`Safe` when none matched).
    pub risk: RiskLevel,
    /// Every pattern that matched the command (raw + inline scripts).
    pub matched: Vec<MatchResult>,
    /// Sorted, merged highlight spans for the original raw command.
    pub highlight_ranges: Vec<HighlightRange>,
    /// The parsed representation of the original command string.
    pub command: ParsedCommand,
}

impl Assessment {
    /// Determine what caused this assessment, ignoring allowlist (handled by the caller).
    pub fn decision_source(&self) -> DecisionSource {
        if self.matched.is_empty() {
            return DecisionSource::Fallback;
        }
        if self
            .matched
            .iter()
            .any(|m| m.pattern.source == PatternSource::Custom)
        {
            DecisionSource::CustomPattern
        } else {
            DecisionSource::BuiltinPattern
        }
    }
}

impl Scanner {
    /// Assess a raw shell command and return a complete [`Assessment`].
    ///
    /// Pipeline:
    /// 1. Parse the command via [`crate::interceptor::parser::Parser::parse`] to preserve the original command contract.
    /// 2. Run [`Scanner::quick_scan`] on the raw command — if no keyword hits, return `Safe` immediately.
    /// 3. Build the recursive scan path via nested parsing helpers.
    /// 4. Run [`Scanner::full_scan`] on each discovered target and merge unique pattern matches.
    /// 5. Compute the maximum [`RiskLevel`] across all matched patterns and return.
    pub fn assess(&self, cmd: &str) -> Assessment {
        if cmd.len() > super::MAX_SCAN_COMMAND_LEN {
            return uncertain_assessment_without_parse(
                cmd,
                "SCAN-001",
                format!(
                    "scan input exceeded command length limit ({})",
                    super::MAX_SCAN_COMMAND_LEN
                ),
                Some(
                    "Review the command out-of-band or move the payload into a smaller, reviewed script file",
                ),
            );
        }

        let command = crate::interceptor::parser::Parser::parse(cmd);

        if let Some(script) = command
            .inline_scripts
            .iter()
            .find(|script| script.body.len() > super::MAX_INLINE_SCRIPT_LEN)
        {
            let interpreter = script.interpreter.clone();
            return uncertain_assessment(
                command,
                "SCAN-002",
                format!(
                    "scan input exceeded inline script length limit ({}) for {}",
                    super::MAX_INLINE_SCRIPT_LEN,
                    interpreter
                ),
                Some(
                    "Review the generated script separately or store it in a checked file before execution",
                ),
            );
        }

        let maybe_pipelines = cmd
            .contains('|')
            .then(|| crate::interceptor::parser::top_level_pipelines(cmd));
        let has_pipeline_chain = maybe_pipelines
            .as_ref()
            .map(|pipelines| pipelines.iter().any(|chain| chain.segments.len() > 1))
            .unwrap_or(false);

        if !self.quick_scan(cmd) && !has_pipeline_chain {
            return Assessment {
                risk: RiskLevel::Safe,
                matched: vec![],
                highlight_ranges: vec![],
                command,
            };
        }

        let mut matched = Vec::new();

        let target_report = recursive::scan_targets(cmd, &command);
        if let Some(limit_hit) = target_report.limit_hit {
            return uncertain_assessment(
                command,
                "SCAN-003",
                recursive_limit_description(limit_hit),
                Some(
                    "Reduce shell nesting depth or rewrite the command into a reviewed intermediate script",
                ),
            );
        }

        for target in target_report.targets {
            for pattern in self.full_scan(&target) {
                if !matched
                    .iter()
                    .any(|existing: &MatchResult| existing.pattern.id == pattern.pattern.id)
                {
                    matched.push(pattern);
                }
            }
        }

        if let Some(pipelines) = maybe_pipelines {
            for evidence in pipeline_semantics::semantic_pipeline_matches(&pipelines) {
                if !matched
                    .iter()
                    .any(|existing: &MatchResult| existing.pattern.id == evidence.pattern.id)
                {
                    matched.push(evidence);
                }
            }
        }

        let risk = matched
            .iter()
            .map(|p| p.pattern.risk)
            .max()
            .unwrap_or(RiskLevel::Safe);
        let highlight_ranges = highlighting::sorted_highlight_ranges(cmd, &matched);

        Assessment {
            risk,
            matched,
            highlight_ranges,
            command,
        }
    }
}

fn uncertain_assessment_without_parse(
    cmd: &str,
    id: &'static str,
    description: String,
    safe_alt: Option<&'static str>,
) -> Assessment {
    uncertain_assessment(
        ParsedCommand {
            executable: None,
            args: Vec::new(),
            inline_scripts: Vec::new(),
            raw: cmd.to_string(),
        },
        id,
        description,
        safe_alt,
    )
}

fn uncertain_assessment(
    command: ParsedCommand,
    id: &'static str,
    description: String,
    safe_alt: Option<&'static str>,
) -> Assessment {
    let matched = vec![uncertain_match(id, description, safe_alt)];
    let highlight_ranges = highlighting::sorted_highlight_ranges(&command.raw, &matched);

    Assessment {
        risk: RiskLevel::Warn,
        matched,
        highlight_ranges,
        command,
    }
}

fn uncertain_match(
    id: &'static str,
    description: String,
    safe_alt: Option<&'static str>,
) -> MatchResult {
    MatchResult {
        pattern: Arc::new(Pattern {
            id: id.into(),
            category: Category::Process,
            risk: RiskLevel::Warn,
            pattern: id.into(),
            description: description.into(),
            safe_alt: safe_alt.map(Into::into),
            source: PatternSource::Builtin,
        }),
        matched_text: String::new(),
        highlight_range: None,
    }
}

fn recursive_limit_description(limit: RecursiveScanLimit) -> String {
    match limit {
        RecursiveScanLimit::DepthExceeded { limit } => {
            format!("scan input exceeded recursive parsing depth limit ({limit})")
        }
    }
}

#[cfg(test)]
pub(super) fn assess_for_tests(scanner: &Scanner, cmd: &str) -> Assessment {
    scanner.assess(cmd)
}
