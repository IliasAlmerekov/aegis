use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

pub use aegis_types::PolicyRuleDecision;
use aegis_types::{Category, Pattern, PatternSource, RiskLevel};

use super::AuditIntegrityMode;
use crate::error::ConfigError;

type Result<T> = std::result::Result<T, ConfigError>;

/// A single token in a typed policy rule pattern.
///
/// - `Single(s)` matches exactly the literal string `s`.
/// - `Alts(v)` matches any one of the strings in `v`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum PolicyPatternToken {
    /// A single literal token.
    Single(String),
    /// A set of alternative tokens (any one must match).
    Alts(Vec<String>),
}

/// Conditional override: when environment variable `env` equals `value`, use
/// `then` as the decision instead of the rule's default `decision`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WhenClause {
    /// Environment variable name to check.
    pub env: String,
    /// Expected value of the environment variable.
    pub value: String,
    /// Decision to use when the condition is met.
    pub then: PolicyRuleDecision,
}

/// A typed `[[rules]]` entry in `aegis.toml`.
///
/// Each rule defines a token-sequence pattern and the decision to apply when a
/// command matches.  Optional `match_examples` / `not_match_examples` let you
/// embed self-documenting test cases that are verified at config-load time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyRule {
    /// Ordered list of tokens the command must start with.
    pub pattern: Vec<PolicyPatternToken>,
    /// Decision when this rule fires and no `when` clause overrides it.
    pub decision: PolicyRuleDecision,
    /// Human-readable rationale stored in audit logs and shown in the UI.
    pub justification: Option<String>,
    /// Example commands that MUST match this rule (validated at load time).
    #[serde(default)]
    pub match_examples: Vec<String>,
    /// Example commands that must NOT match this rule (validated at load time).
    #[serde(default)]
    pub not_match_examples: Vec<String>,
    /// Optional conditional override.
    pub when: Option<WhenClause>,
}

/// A user-defined pattern loaded from `aegis.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UserPattern {
    /// Unique identifier for this pattern.
    pub id: String,
    /// Semantic category (e.g. Filesystem, Database).
    pub category: Category,
    /// Risk level assigned when this pattern matches.
    pub risk: RiskLevel,
    /// Regex or literal pattern string.
    pub pattern: String,
    /// Human-readable explanation of what this pattern detects.
    pub description: String,
    /// Safer alternative command to suggest, if any.
    pub safe_alt: Option<String>,
    /// Optional rationale for adding this pattern.
    pub justification: Option<String>,
}

/// Convert a config-layer [`UserPattern`] into the neutral [`Pattern`] consumed
/// by the scanner. This conversion lives at the config/orchestration boundary so
/// the scanner crate never depends on config-specific types.
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

mod offset_datetime_option {
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    pub fn serialize<S>(value: &Option<OffsetDateTime>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(value) => serializer
                .serialize_some(&value.format(&Rfc3339).map_err(serde::ser::Error::custom)?),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<OffsetDateTime>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<String>::deserialize(deserializer)?;
        value
            .map(|value| {
                OffsetDateTime::parse(&value, &Rfc3339).map_err(|error| {
                    D::Error::custom(format!("invalid RFC 3339 timestamp: {error}"))
                })
            })
            .transpose()
    }
}

/// A structured allowlist rule with optional scope, expiry, and rationale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AllowlistRule {
    /// Command pattern to allow.
    pub pattern: String,
    /// Optional working-directory scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Optional user scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Optional expiry timestamp (RFC 3339).
    #[serde(
        default,
        with = "offset_datetime_option",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub expires_at: Option<OffsetDateTime>,
    /// Human-readable reason for allowing this pattern.
    pub reason: String,
}

/// A structured block rule with optional scope, expiry, and rationale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlockRule {
    /// Command pattern to block.
    pub pattern: String,
    /// Optional working-directory scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Optional user scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Optional expiry timestamp (RFC 3339).
    #[serde(
        default,
        with = "offset_datetime_option",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub expires_at: Option<OffsetDateTime>,
    /// Human-readable reason for blocking this pattern.
    pub reason: String,
}

/// Audit log rotation and integrity configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AuditConfig {
    /// Enable automatic audit log rotation.
    pub rotation_enabled: bool,
    /// Max audit file size in bytes before rotation.
    pub max_file_size_bytes: u64,
    /// Number of rotated audit files to retain.
    pub retention_files: usize,
    /// Compress rotated audit files with gzip.
    pub compress_rotated: bool,
    /// Integrity chaining mode for corruption and inconsistent-edit checks.
    pub integrity_mode: AuditIntegrityMode,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            rotation_enabled: false,
            max_file_size_bytes: 10 * 1024 * 1024,
            retention_files: 5,
            compress_rotated: true,
            integrity_mode: AuditIntegrityMode::ChainSha256,
        }
    }
}

/// A trusted global alias for the Language-aware analysis interpreter
/// registry (ADR-022 §6) — e.g. a wrapper script name that should be treated
/// as a stand-in for a canonical registry interpreter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrustedAlias {
    /// The alias program name as it appears in a command (e.g. `"py"`).
    pub alias: String,
    /// The canonical registry program name it stands in for (e.g. `"python3"`).
    pub canonical: String,
}

/// Non-configurable hard ceiling for
/// `language_analysis.script_file_limit_bytes` (ADR-022 §6) — 1 MiB, enforced
/// at every config layer.
pub const LANGUAGE_ANALYSIS_SCRIPT_FILE_HARD_CEILING_BYTES: u64 = 1024 * 1024;

/// Default `language_analysis.script_file_limit_bytes` (ADR-022 §6) — 256 KiB.
const LANGUAGE_ANALYSIS_SCRIPT_FILE_DEFAULT_BYTES: u64 = 256 * 1024;
/// Hard ceiling and default for one inline source body.
pub const LANGUAGE_ANALYSIS_INLINE_SOURCE_MAX_BYTES: u64 = 16 * 1024;
/// Hard ceiling for top-level script files inspected per command.
pub const LANGUAGE_ANALYSIS_MAX_SCRIPT_FILES: u64 = 8;
/// Hard ceiling for recursive analysis depth.
pub const LANGUAGE_ANALYSIS_MAX_DEPTH: u64 = 8;
/// Hard ceiling for all top-level and recursive targets.
pub const LANGUAGE_ANALYSIS_MAX_TARGETS: u64 = 16;
/// Hard ceiling for aggregate source bytes in one analysis session.
pub const LANGUAGE_ANALYSIS_MAX_AGGREGATE_BYTES: u64 = 1024 * 1024;
/// Hard ceiling for one command's complete language-analysis session.
pub const LANGUAGE_ANALYSIS_TIMEOUT_MS: u64 = 100;

/// Language-aware analysis script-file and trusted-alias budgets (ADR-022 §6).
///
/// `script_file_limit_bytes` is bounded by
/// [`LANGUAGE_ANALYSIS_SCRIPT_FILE_HARD_CEILING_BYTES`] at every layer; project
/// config may additionally only lower it, never raise it. `trusted_aliases` is
/// a Global-layer-only concept ("trusted global aliases only", ADR-022 §6) —
/// project-layer entries are dropped entirely rather than merged, since a
/// project must never be able to introduce a new trusted interpreter alias.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct LanguageAnalysisConfig {
    /// Maximum bytes accepted from one inline source body.
    pub inline_source_limit_bytes: u64,
    /// Maximum bytes read from a routed script file.
    pub script_file_limit_bytes: u64,
    /// Maximum top-level script files inspected for one command.
    pub max_script_files: u64,
    /// Maximum recursive analysis depth.
    pub max_depth: u64,
    /// Maximum distinct top-level and recursive analysis targets.
    pub max_targets: u64,
    /// Maximum aggregate source bytes across all accepted targets.
    pub max_aggregate_bytes: u64,
    /// Total wall-clock budget for language analysis of one command.
    pub timeout_ms: u64,
    /// Trusted global aliases mapping a wrapper program name to the canonical
    /// registry interpreter it stands in for.
    pub trusted_aliases: Vec<TrustedAlias>,
}

impl Default for LanguageAnalysisConfig {
    fn default() -> Self {
        Self {
            inline_source_limit_bytes: LANGUAGE_ANALYSIS_INLINE_SOURCE_MAX_BYTES,
            script_file_limit_bytes: LANGUAGE_ANALYSIS_SCRIPT_FILE_DEFAULT_BYTES,
            max_script_files: LANGUAGE_ANALYSIS_MAX_SCRIPT_FILES,
            max_depth: LANGUAGE_ANALYSIS_MAX_DEPTH,
            max_targets: LANGUAGE_ANALYSIS_MAX_TARGETS,
            max_aggregate_bytes: LANGUAGE_ANALYSIS_MAX_AGGREGATE_BYTES,
            timeout_ms: LANGUAGE_ANALYSIS_TIMEOUT_MS,
            trusted_aliases: Vec::new(),
        }
    }
}

/// Validate `language_analysis.trusted_aliases` entries: neither field may be
/// empty or whitespace-only, an alias must not map a program to itself, and
/// no two entries may share the same `alias` (ADR-022 §6).
pub(super) fn validate_trusted_aliases(aliases: &[TrustedAlias]) -> Result<()> {
    let mut seen_aliases = std::collections::HashSet::with_capacity(aliases.len());
    for entry in aliases {
        if entry.alias.trim().is_empty() || entry.canonical.trim().is_empty() {
            return Err(ConfigError::Config(
                "language_analysis.trusted_aliases entries must have non-empty alias and \
                 canonical fields"
                    .to_string(),
            ));
        }
        if entry.alias == entry.canonical {
            return Err(ConfigError::Config(format!(
                "language_analysis.trusted_aliases alias '{}' must not map a program to itself",
                entry.alias
            )));
        }
        if !seen_aliases.insert(entry.alias.as_str()) {
            return Err(ConfigError::Config(format!(
                "language_analysis.trusted_aliases contains a duplicate alias '{}'",
                entry.alias
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{PolicyPatternToken, PolicyRule, PolicyRuleDecision, WhenClause};

    /// A single-token pattern like `["git", "push"]` must deserialize into
    /// `Single` variants and the overall `PolicyRule` must round-trip.
    #[test]
    fn test_policy_rule_deserializes_with_single_tokens() {
        let toml = r#"
pattern  = ["git", "push"]
decision = "prompt"
"#;
        let rule: PolicyRule =
            toml::from_str(toml).expect("should deserialize policy rule with single tokens");

        assert_eq!(rule.pattern.len(), 2);
        assert_eq!(
            rule.pattern[0],
            PolicyPatternToken::Single("git".to_string())
        );
        assert_eq!(
            rule.pattern[1],
            PolicyPatternToken::Single("push".to_string())
        );
        assert_eq!(rule.decision, PolicyRuleDecision::Prompt);
    }

    /// A pattern that includes an alternatives array like `["--force", "-f"]`
    /// must deserialize into the `Alts` variant.
    #[test]
    fn test_policy_rule_deserializes_with_alts() {
        let toml = r#"
pattern  = ["git", "push", ["--force", "-f"]]
decision = "block"
"#;
        let rule: PolicyRule =
            toml::from_str(toml).expect("should deserialize policy rule with alts token");

        assert_eq!(rule.pattern.len(), 3);
        assert_eq!(
            rule.pattern[0],
            PolicyPatternToken::Single("git".to_string())
        );
        assert_eq!(
            rule.pattern[1],
            PolicyPatternToken::Single("push".to_string())
        );
        assert_eq!(
            rule.pattern[2],
            PolicyPatternToken::Alts(vec!["--force".to_string(), "-f".to_string()])
        );
        assert_eq!(rule.decision, PolicyRuleDecision::Block);
    }

    /// All three `decision` variants must deserialize from their snake_case names.
    #[test]
    fn test_policy_rule_decision_variants() {
        let allow: PolicyRuleDecision =
            toml::from_str("decision = \"allow\"").expect("allow should deserialize");
        let prompt: PolicyRuleDecision =
            toml::from_str("decision = \"prompt\"").expect("prompt should deserialize");
        let block: PolicyRuleDecision =
            toml::from_str("decision = \"block\"").expect("block should deserialize");

        assert_eq!(allow, PolicyRuleDecision::Allow);
        assert_eq!(prompt, PolicyRuleDecision::Prompt);
        assert_eq!(block, PolicyRuleDecision::Block);
    }

    /// An inline `when` table with env/value/then must deserialize correctly.
    #[test]
    fn test_when_clause_deserializes() {
        let toml = r#"
env   = "CI"
value = "true"
then  = "allow"
"#;
        let when: WhenClause = toml::from_str(toml).expect("should deserialize WhenClause");

        assert_eq!(when.env, "CI");
        assert_eq!(when.value, "true");
        assert_eq!(when.then, PolicyRuleDecision::Allow);
    }

    /// A `PolicyRule` with an empty `pattern` array must either fail serde
    /// deserialization or be caught by validation (the type must enforce it).
    #[test]
    fn test_policy_rule_requires_nonempty_pattern() {
        let toml = r#"
pattern  = []
decision = "block"
"#;
        // Either parse fails or validation must catch it.
        // We use the validator from validate.rs.
        let result: Result<PolicyRule, _> = toml::from_str(toml);
        // If serde succeeds, validation must reject it.
        match result {
            Err(_) => { /* good — serde itself rejected empty pattern */ }
            Ok(rule) => {
                // The validator lives in crate::validate; once implemented it
                // must reject a rule with an empty pattern vector.
                use crate::validate::validate_policy_rules;
                let validation = validate_policy_rules(&[rule]);
                assert!(
                    validation.is_err(),
                    "empty pattern must be rejected by validate_policy_rules"
                );
            }
        }
    }
}
