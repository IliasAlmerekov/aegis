pub use std::sync::Arc;

pub use crate::patterns::{Category, Pattern, PatternSet, PatternSource};
pub use crate::scanner::*;
pub use aegis_parser::{Parser, top_level_pipelines};
pub use aegis_types::RiskLevel;

#[cfg(test)]
fn scanner() -> Scanner {
    let patterns = PatternSet::load().expect("patterns.toml must load");
    Scanner::try_new(patterns).expect("built-in patterns compile")
}

fn test_match_result(matched_text: &str, start: usize, end: usize) -> MatchResult {
    MatchResult {
        pattern: Arc::new(Pattern {
            id: "TEST-001".into(),
            category: Category::Process,
            risk: RiskLevel::Danger,
            pattern: "test".into(),
            description: "test helper".into(),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
        }),
        matched_text: matched_text.to_string(),
        highlight_range: Some(HighlightRange { start, end }),
    }
}

/// Assert that `assess(cmd)` reaches `expected_risk` and that `expected_id` is
/// among the matched patterns. Shared across the `basic` and `h3_gaps` test
/// modules (the `edge_cases` module keeps its own `assert_command_matches_pattern`).
#[cfg(test)]
fn assert_assessment_matches_pattern(cmd: &str, expected_risk: RiskLevel, expected_id: &str) {
    let s = scanner();
    let assessment = s.assess(cmd);

    assert_eq!(
        assessment.risk, expected_risk,
        "command {cmd:?}: got {:?}, expected {expected_risk:?}",
        assessment.risk,
    );
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == expected_id),
        "command {cmd:?}: expected pattern id {expected_id}, got {:?}",
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref())
            .collect::<Vec<_>>()
    );
}

mod advanced;
mod basic;
mod edge_cases;
mod h3_gaps;
