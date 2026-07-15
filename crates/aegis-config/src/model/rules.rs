use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

pub use aegis_types::PolicyRuleDecision;
use aegis_types::{Category, Pattern, PatternSource, RiskLevel};

use super::AuditIntegrityMode;

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
