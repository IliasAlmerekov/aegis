use std::sync::Arc;

use crate::nested::RecursiveScanLimit;
use crate::patterns::{Category, Pattern, PatternSource};
use aegis_parser::ParsedCommand;
use aegis_types::RiskLevel;

pub use aegis_types::{Assessment, DecisionSource, MatchResult};

use super::{Scanner, highlighting, pipeline_semantics, recursive};

impl Scanner {
    /// Assess a raw shell command and return a complete [`Assessment`].
    ///
    /// Pipeline:
    /// 1. Parse the command via [`aegis_parser::Parser::parse`] to preserve the original command contract.
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

        let command = aegis_parser::Parser::parse(cmd);

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

        // Pipeline detection needs the raw string (quoting-aware).
        let maybe_pipelines = cmd
            .contains('|')
            .then(|| aegis_parser::top_level_pipelines(cmd));
        let has_pipeline_chain = maybe_pipelines
            .as_ref()
            .map(|pipelines| pipelines.iter().any(|chain| chain.segments.len() > 1))
            .unwrap_or(false);

        // Use the normalized form as the primary scan target — free of quoting noise.
        if !self.quick_scan(&command.normalized) && !has_pipeline_chain {
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

        for target in &target_report.targets {
            // Derive the program from the target's first token (lowercase) so
            // full_scan can use the by-program index on the fast path.
            let prog = target.split_whitespace().next().map(str::to_lowercase);
            for pattern in self.full_scan(target, prog.as_deref()) {
                if !matched
                    .iter()
                    .any(|existing: &MatchResult| existing.pattern.id == pattern.pattern.id)
                {
                    matched.push(pattern);
                }
            }

            // Token-prefix scan: parsed tokens, not raw string.
            let tokens = aegis_parser::split_tokens(target);
            let token_refs: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();
            for result in self.prefix_scan(&token_refs) {
                if !matched
                    .iter()
                    .any(|existing: &MatchResult| existing.pattern.id == result.pattern.id)
                {
                    matched.push(result);
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
            program: None,
            argv: Vec::new(),
            normalized: cmd.to_string(),
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
            justification: None,
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

#[cfg(test)]
mod tests {
    #[test]
    fn uncertain_match_produces_pattern_with_no_justification() {
        let result = super::uncertain_match("SCAN-001", "desc".to_string(), None);
        assert!(result.pattern.justification.is_none());
    }
}
