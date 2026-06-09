// Pattern struct, Category, loading

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use serde::Deserialize;

pub use aegis_types::{Category, Pattern, PatternSource, PatternToken, PrefixPattern};

use crate::config::UserPattern;
use crate::error::AegisError;
use crate::interceptor::RiskLevel;

// ── Token-prefix rule types (live alongside regex-based Pattern) ──────────

/// A token-level prefix rule that matches the beginning of a tokenized command.
///
/// Replaces free-form regex for commands whose dangerous semantics are fully
/// captured by a fixed prefix of tokens (e.g. `git push --force`).
#[derive(Debug, Clone)]
pub struct PrefixRule {
    /// Unique pattern identifier.
    pub id: Cow<'static, str>,
    /// Semantic category.
    pub category: Category,
    /// Token prefix to match against.
    pub pattern: PrefixPattern,
    /// Assigned risk level when this rule matches.
    pub risk: RiskLevel,
    /// Human-readable description of what this rule detects.
    pub description: Cow<'static, str>,
    /// Safer alternative to suggest, if any.
    pub safe_alt: Option<Cow<'static, str>>,
    /// Optional rationale for this rule.
    pub justification: Option<Cow<'static, str>>,
    /// Whether the rule is built-in or user-defined.
    pub source: PatternSource,
    /// Example commands that MUST match this prefix rule.
    pub match_examples: &'static [&'static str],
    /// Example commands that MUST NOT match this prefix rule.
    pub not_match_examples: &'static [&'static str],
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
    justification: Option<String>,
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
            justification: raw.justification.map(Cow::Owned),
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
            justification: user.justification.map(Cow::Owned),
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
    patterns: Vec<Arc<Pattern>>,
    prefix_rules: Vec<Arc<PrefixRule>>,
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
        // 4) validate unified set (required fields + duplicate IDs forbidden for regex patterns).
        let mut pattern_ids: HashSet<String> =
            HashSet::with_capacity(builtin_patterns.len() + custom_patterns.len());
        let mut patterns: Vec<Arc<Pattern>> =
            Vec::with_capacity(builtin_patterns.len() + custom_patterns.len());

        for pattern in builtin_patterns
            .into_iter()
            .chain(custom_patterns.into_iter())
        {
            Self::validate_pattern(&pattern, &mut pattern_ids)?;
            patterns.push(Arc::new(pattern));
        }

        // 5) compile built-in prefix rules.
        let prefix_rules = builtin_prefix_rules();

        // 5a) validate built-in prefix rules against their examples in debug builds and tests.
        //    A rule that fails its own examples is a bug and must be fixed before the binary ships.
        #[cfg(debug_assertions)]
        for rule in &prefix_rules {
            if let Err(e) = rule.validate_examples() {
                panic!(
                    "built-in prefix rule {} failed example validation: {e}",
                    rule.id
                );
            }
        }

        // 6) validate prefix rules: required fields + no conflict with regex pattern IDs.
        //    Duplicate IDs within prefix rules are intentional: the same logical rule can
        //    have multiple syntactic forms (e.g. "docker-compose" vs "docker compose").
        for rule in &prefix_rules {
            Self::validate_prefix_rule(rule, &pattern_ids)?;
        }

        let prefix_rules: Vec<Arc<PrefixRule>> = prefix_rules.into_iter().map(Arc::new).collect();

        // 7) compiled into runtime PatternSet.
        Ok(PatternSet {
            patterns,
            prefix_rules,
        })
    }

    /// Return the effective merged regex pattern set consumed by scanner construction.
    pub fn patterns(&self) -> &[Arc<Pattern>] {
        self.patterns.as_slice()
    }

    /// Return the effective merged prefix-rule set consumed by scanner construction.
    pub fn prefix_rules(&self) -> &[Arc<PrefixRule>] {
        self.prefix_rules.as_slice()
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

    fn validate_prefix_rule(
        rule: &PrefixRule,
        pattern_ids: &HashSet<String>,
    ) -> Result<(), AegisError> {
        if rule.id.trim().is_empty() {
            return Err(AegisError::Config(format!(
                "invalid prefix rule id: empty id (source={:?})",
                rule.source
            )));
        }

        if rule.pattern.is_empty() {
            return Err(AegisError::Config(format!(
                "invalid prefix rule {}: empty pattern",
                rule.id
            )));
        }

        if rule.description.trim().is_empty() {
            return Err(AegisError::Config(format!(
                "invalid prefix rule {}: empty description",
                rule.id
            )));
        }

        // Prevent a prefix rule from shadowing a regex pattern with the same ID.
        let id = rule.id.as_ref();
        if pattern_ids.contains(id) {
            return Err(AegisError::Config(format!(
                "prefix rule id '{id}' conflicts with an existing regex pattern id"
            )));
        }

        Ok(())
    }
}

mod builtins_a;
mod builtins_b;

// ── Built-in prefix rules (replaces regex for token-prefixable commands) ───

pub(super) fn s(s: &'static str) -> PatternToken {
    PatternToken::Single(Cow::Borrowed(s))
}

pub(super) fn a(alts: &'static [&'static str]) -> PatternToken {
    PatternToken::Alts(alts.iter().map(|&s| Cow::Borrowed(s)).collect())
}

pub(super) fn any_star() -> PatternToken {
    PatternToken::AnyStar
}

fn builtin_prefix_rules() -> Vec<PrefixRule> {
    let mut rules = builtins_a::rules();
    rules.extend(builtins_b::rules());
    rules
}
