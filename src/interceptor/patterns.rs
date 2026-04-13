// Pattern struct, Category, loading

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::UserPattern;
use crate::error::AegisError;
use crate::interceptor::RiskLevel;

/// Whether a pattern was compiled into the binary or loaded from user config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternSource {
    Builtin,
    Custom,
}

/// Which class of operation the pattern guards against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
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
    pub id: Cow<'static, str>,
    pub category: Category,
    pub risk: RiskLevel,
    pub pattern: Cow<'static, str>,
    pub description: Cow<'static, str>,
    pub safe_alt: Option<Cow<'static, str>>,
    pub source: PatternSource,
}

/// Internal helper: TOML-deserializable representation before conversion to [`Pattern`].
#[derive(Debug, Deserialize)]
struct RawPattern {
    id: String,
    category: Category,
    risk: RiskLevel,
    pattern: String,
    description: String,
    safe_alt: Option<String>,
}

impl From<RawPattern> for Pattern {
    fn from(raw: RawPattern) -> Self {
        Pattern {
            id: Cow::Owned(raw.id),
            category: raw.category,
            risk: raw.risk,
            pattern: Cow::Owned(raw.pattern),
            description: Cow::Owned(raw.description),
            safe_alt: raw.safe_alt.map(Cow::Owned),
            source: PatternSource::Builtin,
        }
    }
}

impl From<UserPattern> for Pattern {
    fn from(user: UserPattern) -> Self {
        Pattern {
            id: Cow::Owned(user.id),
            category: user.category,
            risk: user.risk,
            pattern: Cow::Owned(user.pattern),
            description: Cow::Owned(user.description),
            safe_alt: user.safe_alt.map(Cow::Owned),
            source: PatternSource::Custom,
        }
    }
}

/// Wrapper for TOML top-level table: `[[patterns]]`.
#[derive(Debug, Deserialize)]
struct PatternsFile {
    patterns: Vec<RawPattern>,
}

/// Effective merged pattern set consumed when constructing a scanner.
///
/// This is the authoritative runtime view after combining the built-in
/// patterns embedded in the binary with any custom patterns supplied by the
/// resolved config layers.
#[derive(Debug)]
pub struct PatternSet {
    pub patterns: Vec<Arc<Pattern>>,
}

/// Built-in patterns embedded at compile time — binary stays self-contained.
const BUILTIN_PATTERNS_TOML: &str = include_str!("../../config/patterns.toml");

impl PatternSet {
    /// Parse and return the canonical built-in-only pattern set.
    ///
    /// This loads the embedded `config/patterns.toml` without any config
    /// overlays, providing the built-in source of truth before custom patterns
    /// are merged for runtime scanner construction.
    pub fn load() -> Result<PatternSet, AegisError> {
        Self::from_sources(&[])
    }

    /// Build the authoritative merged pattern view for scanner construction.
    ///
    /// Merge order is fixed and explicit:
    /// 1) built-in patterns embedded in the binary
    /// 2) user-defined patterns loaded from config
    ///
    /// The returned set is the effective runtime input consumed by
    /// `Scanner::new`, after validation and normalization into one `Pattern`
    /// representation.
    pub fn from_sources(custom_patterns: &[UserPattern]) -> Result<PatternSet, AegisError> {
        let file: PatternsFile = toml::from_str(BUILTIN_PATTERNS_TOML)
            .map_err(|e| AegisError::Config(format!("failed to parse patterns.toml: {e}")))?;

        // 1) built-in
        let builtin_patterns: Vec<Pattern> = file.patterns.into_iter().map(Pattern::from).collect();

        // 2) custom (already merged global+project in config layer)
        let custom_patterns: Vec<Pattern> =
            custom_patterns.iter().cloned().map(Pattern::from).collect();

        // 3) normalize to one structure (`Pattern`) happened via `From` conversions above.
        // 4) validate unified set (required fields + duplicate IDs forbidden).
        let mut ids: HashSet<String> =
            HashSet::with_capacity(builtin_patterns.len() + custom_patterns.len());
        let mut patterns: Vec<Arc<Pattern>> =
            Vec::with_capacity(builtin_patterns.len() + custom_patterns.len());

        for pattern in builtin_patterns
            .into_iter()
            .chain(custom_patterns.into_iter())
        {
            Self::validate_pattern(&pattern, &mut ids)?;
            patterns.push(Arc::new(pattern));
        }

        // 5) compiled into runtime PatternSet (regex compilation happens in Scanner::new).
        Ok(PatternSet { patterns })
    }

    fn validate_pattern(pattern: &Pattern, ids: &mut HashSet<String>) -> Result<(), AegisError> {
        if pattern.id.trim().is_empty() {
            return Err(AegisError::Config(format!(
                "invalid pattern id: empty id (source={:?})",
                pattern.source
            )));
        }

        if pattern.pattern.trim().is_empty() {
            return Err(AegisError::Config(format!(
                "invalid pattern {}: empty regex pattern",
                pattern.id
            )));
        }

        if pattern.description.trim().is_empty() {
            return Err(AegisError::Config(format!(
                "invalid pattern {}: empty description",
                pattern.id
            )));
        }

        let id = pattern.id.as_ref();
        if !ids.insert(id.to_string()) {
            return Err(AegisError::Config(format!(
                "duplicate pattern id '{id}' is not allowed"
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UserPattern;

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

    #[test]
    fn from_sources_merges_builtin_and_custom_and_marks_custom_source() {
        let custom = UserPattern {
            id: "USR-999".to_string(),
            category: Category::Cloud,
            risk: RiskLevel::Warn,
            pattern: r"internal-teardown".to_string(),
            description: "Internal teardown guard".to_string(),
            safe_alt: Some("internal-teardown --dry-run".to_string()),
        };

        let set = PatternSet::from_sources(&[custom]).expect("custom pattern set should compile");

        let matched = set
            .patterns
            .iter()
            .find(|p| p.id.as_ref() == "USR-999")
            .expect("custom pattern id should be present");

        assert_eq!(matched.source, PatternSource::Custom);
    }

    #[test]
    fn from_sources_rejects_duplicate_ids_between_builtin_and_custom() {
        let duplicate = UserPattern {
            id: "FS-001".to_string(),
            category: Category::Filesystem,
            risk: RiskLevel::Warn,
            pattern: r"dummy-pattern".to_string(),
            description: "dummy".to_string(),
            safe_alt: None,
        };

        let err = PatternSet::from_sources(&[duplicate]).expect_err("duplicate id must fail");
        assert!(err.to_string().contains("duplicate pattern id 'FS-001'"));
    }

    #[test]
    fn from_sources_rejects_duplicate_ids_inside_custom_patterns() {
        let first = UserPattern {
            id: "USR-DUP".to_string(),
            category: Category::Cloud,
            risk: RiskLevel::Warn,
            pattern: r"first".to_string(),
            description: "first".to_string(),
            safe_alt: None,
        };
        let second = UserPattern {
            id: "USR-DUP".to_string(),
            category: Category::Cloud,
            risk: RiskLevel::Danger,
            pattern: r"second".to_string(),
            description: "second".to_string(),
            safe_alt: None,
        };

        let err = PatternSet::from_sources(&[first, second]).expect_err("duplicate id must fail");
        assert!(err.to_string().contains("duplicate pattern id 'USR-DUP'"));
    }
}
