// Pattern struct, Category, loading

use std::borrow::Cow;
use std::sync::Arc;

use serde::Deserialize;

use crate::error::AegisError;
use crate::interceptor::RiskLevel;

/// Which class of operation the pattern guards against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Category {
    Filesystem,
    Git,
    Database,
    Cloud,
    Docker,
    Process,
    Package,
}

/// Unified runtime pattern.
///
/// Built-in patterns use `Cow::Borrowed(&'static str)` — zero-copy.
/// User-defined patterns loaded from TOML use `Cow::Owned(String)`.
#[derive(Debug, Clone)]
pub struct Pattern {
    pub id:          Cow<'static, str>,
    pub category:    Category,
    pub risk:        RiskLevel,
    pub pattern:     Cow<'static, str>,
    pub description: Cow<'static, str>,
    pub safe_alt:    Option<Cow<'static, str>>,
}

/// Internal helper: TOML-deserializable representation before conversion to [`Pattern`].
#[derive(Debug, Deserialize)]
struct RawPattern {
    id:          String,
    category:    Category,
    risk:        RiskLevel,
    pattern:     String,
    description: String,
    safe_alt:    Option<String>,
}

impl From<RawPattern> for Pattern {
    fn from(raw: RawPattern) -> Self {
        Pattern {
            id:          Cow::Owned(raw.id),
            category:    raw.category,
            risk:        raw.risk,
            pattern:     Cow::Owned(raw.pattern),
            description: Cow::Owned(raw.description),
            safe_alt:    raw.safe_alt.map(Cow::Owned),
        }
    }
}

/// Wrapper for TOML top-level table: `[[patterns]]`.
#[derive(Debug, Deserialize)]
struct PatternsFile {
    patterns: Vec<RawPattern>,
}

/// Compiled set of patterns used by the scanner.
pub struct PatternSet {
    pub patterns: Vec<Arc<Pattern>>,
}

/// Built-in patterns embedded at compile time — binary stays self-contained.
const BUILTIN_PATTERNS_TOML: &str = include_str!("../../config/patterns.toml");

impl PatternSet {
    /// Parse and return the built-in pattern set from the embedded `config/patterns.toml`.
    pub fn load() -> Result<PatternSet, AegisError> {
        let file: PatternsFile = toml::from_str(BUILTIN_PATTERNS_TOML)
            .map_err(|e| AegisError::Config(format!("failed to parse patterns.toml: {e}")))?;

        let patterns = file
            .patterns
            .into_iter()
            .map(|raw| Arc::new(Pattern::from(raw)))
            .collect();

        Ok(PatternSet { patterns })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_builtin_patterns_parses_without_error() {
        let set = PatternSet::load().expect("patterns.toml should parse cleanly");
        assert!(
            set.patterns.len() >= 50,
            "expected at least 50 patterns, got {}",
            set.patterns.len()
        );
    }

    #[test]
    fn all_categories_represented() {
        let set = PatternSet::load().unwrap();
        let categories: std::collections::HashSet<_> =
            set.patterns.iter().map(|p| p.category).collect();
        assert!(categories.contains(&Category::Filesystem));
        assert!(categories.contains(&Category::Git));
        assert!(categories.contains(&Category::Database));
        assert!(categories.contains(&Category::Cloud));
        assert!(categories.contains(&Category::Docker));
        assert!(categories.contains(&Category::Process));
        assert!(categories.contains(&Category::Package));
    }

    #[test]
    fn all_patterns_have_non_empty_fields() {
        let set = PatternSet::load().unwrap();
        for p in &set.patterns {
            assert!(!p.id.is_empty(), "empty id");
            assert!(!p.pattern.is_empty(), "empty pattern for {}", p.id);
            assert!(!p.description.is_empty(), "empty description for {}", p.id);
        }
    }
}
