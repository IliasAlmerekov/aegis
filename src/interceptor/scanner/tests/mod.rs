pub use std::sync::Arc;

pub use crate::config::UserPattern;
pub use crate::interceptor::RiskLevel;
pub use crate::interceptor::parser::{Parser, top_level_pipelines};
pub use crate::interceptor::patterns::{Category, Pattern, PatternSet, PatternSource};
pub use crate::interceptor::scanner::*;

#[cfg(test)]
fn scanner() -> Scanner {
    let patterns = PatternSet::load().expect("patterns.toml must load");
    Scanner::new(patterns)
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

mod advanced;
mod basic;
mod edge_cases;
