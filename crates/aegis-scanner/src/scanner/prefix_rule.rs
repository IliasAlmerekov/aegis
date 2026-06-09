use std::borrow::Cow;
use std::sync::Arc;

use crate::patterns::PrefixRule;
use crate::scanner::MatchResult;

impl PrefixRule {
    /// Check whether `tokens` matches this rule's prefix pattern.
    ///
    /// Delegates to [`aegis_parser::matches_prefix`], which supports
    /// `Single`/`Alts`/`Any`/`AnyStar` tokens. The pattern must be a prefix of
    /// `tokens` — extra trailing tokens are allowed. Empty patterns never match.
    pub fn matches_tokens(&self, tokens: &[&str]) -> bool {
        aegis_parser::matches_prefix(&self.pattern, tokens)
    }

    /// Produce a [`MatchResult`] for this rule when it matched `tokens`.
    ///
    /// `matched_text` joins the consumed tokens (up to the pattern length, ignoring
    /// wildcards for span purposes).
    pub fn to_match_result(&self, tokens: &[&str]) -> MatchResult {
        let consumed = tokens.join(" ");
        MatchResult {
            pattern: Arc::new(crate::patterns::Pattern {
                id: self.id.clone(),
                category: self.category,
                risk: self.risk,
                pattern: Cow::Owned(
                    self.pattern
                        .iter()
                        .map(|t| format!("{t:?}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                description: self.description.clone(),
                safe_alt: self.safe_alt.clone(),
                justification: self.justification.clone(),
                source: self.source,
            }),
            matched_text: consumed,
            highlight_range: None,
        }
    }

    /// Validate that [`match_examples`] all match and [`not_match_examples`] all do not.
    ///
    /// Called at startup in debug builds and tests. A rule that fails its own
    /// examples is treated as a bug and must be fixed before the binary ships.
    pub(crate) fn validate_examples(&self) -> Result<(), String> {
        for example in self.match_examples {
            let tokens = aegis_parser::split_tokens(example);
            let token_refs: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();
            if !self.matches_tokens(&token_refs) {
                return Err(format!(
                    "match_example {:?} does not match pattern {:?}",
                    example, self.pattern
                ));
            }
        }
        for example in self.not_match_examples {
            let tokens = aegis_parser::split_tokens(example);
            let token_refs: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();
            if self.matches_tokens(&token_refs) {
                return Err(format!(
                    "not_match_example {:?} unexpectedly matches pattern {:?}",
                    example, self.pattern
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patterns::{Category, PatternSource, PatternToken, PrefixRule};
    use aegis_types::RiskLevel;

    fn single(s: &'static str) -> PatternToken {
        PatternToken::Single(Cow::Borrowed(s))
    }

    fn alts(alts: &[&'static str]) -> PatternToken {
        PatternToken::Alts(alts.iter().map(|&s| Cow::Borrowed(s)).collect())
    }

    #[test]
    fn prefix_rule_matches_single_token() {
        let rule = PrefixRule {
            id: Cow::Borrowed("T-001"),
            category: Category::Process,
            pattern: vec![single("rm")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &[],
        };
        assert!(rule.matches_tokens(&["rm", "-rf", "/"]));
    }

    #[test]
    fn prefix_rule_matches_multiple_tokens() {
        let rule = PrefixRule {
            id: Cow::Borrowed("T-002"),
            category: Category::Git,
            pattern: vec![single("git"), single("push")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &[],
        };
        assert!(rule.matches_tokens(&["git", "push", "origin", "main"]));
    }

    #[test]
    fn prefix_rule_matches_with_alts() {
        let rule = PrefixRule {
            id: Cow::Borrowed("T-003"),
            category: Category::Git,
            pattern: vec![single("git"), single("push"), alts(&["--force", "-f"])],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &[],
        };
        assert!(rule.matches_tokens(&["git", "push", "--force"]));
        assert!(rule.matches_tokens(&["git", "push", "-f"]));
    }

    #[test]
    fn prefix_rule_fails_on_token_mismatch() {
        let rule = PrefixRule {
            id: Cow::Borrowed("T-004"),
            category: Category::Git,
            pattern: vec![single("git"), single("push")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &[],
        };
        assert!(!rule.matches_tokens(&["git", "status"]));
    }

    #[test]
    fn prefix_rule_fails_on_insufficient_tokens() {
        let rule = PrefixRule {
            id: Cow::Borrowed("T-005"),
            category: Category::Git,
            pattern: vec![single("git"), single("push"), single("origin")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &[],
        };
        assert!(!rule.matches_tokens(&["git", "push"]));
    }

    #[test]
    fn prefix_rule_alts_fails_when_none_match() {
        let rule = PrefixRule {
            id: Cow::Borrowed("T-006"),
            category: Category::Git,
            pattern: vec![single("git"), single("push"), alts(&["--force", "-f"])],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &[],
        };
        assert!(!rule.matches_tokens(&["git", "push", "--dry-run"]));
    }

    #[test]
    fn prefix_rule_exact_length_match() {
        let rule = PrefixRule {
            id: Cow::Borrowed("T-007"),
            category: Category::Filesystem,
            pattern: vec![single("rm"), single("-rf")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &[],
        };
        assert!(rule.matches_tokens(&["rm", "-rf"]));
    }

    #[test]
    fn prefix_rule_empty_pattern_never_matches() {
        let rule = PrefixRule {
            id: Cow::Borrowed("T-008"),
            category: Category::Process,
            pattern: vec![],
            risk: RiskLevel::Safe,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &[],
        };
        assert!(!rule.matches_tokens(&["anything", "here"]));
        assert!(!rule.matches_tokens(&[]));
    }

    #[test]
    fn prefix_rule_case_insensitive_for_commands() {
        let rule = PrefixRule {
            id: Cow::Borrowed("T-009"),
            category: Category::Git,
            pattern: vec![single("Git")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &[],
        };
        assert!(rule.matches_tokens(&["git"])); // case-insensitive for non-flag tokens
        assert!(rule.matches_tokens(&["Git"]));
    }

    #[test]
    fn prefix_rule_case_sensitive_for_flags() {
        let rule = PrefixRule {
            id: Cow::Borrowed("T-013"),
            category: Category::Git,
            pattern: vec![single("git"), single("branch"), single("-D")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &[],
        };
        assert!(rule.matches_tokens(&["git", "branch", "-D"]));
        assert!(!rule.matches_tokens(&["git", "branch", "-d"])); // flags are case-sensitive
    }

    #[test]
    fn prefix_rule_multiple_alts_positions() {
        let rule = PrefixRule {
            id: Cow::Borrowed("T-010"),
            category: Category::Cloud,
            pattern: vec![
                single("aws"),
                alts(&["ec2", "s3"]),
                alts(&["delete", "terminate"]),
            ],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &[],
        };
        assert!(rule.matches_tokens(&["aws", "ec2", "delete"]));
        assert!(rule.matches_tokens(&["aws", "s3", "terminate"]));
        assert!(!rule.matches_tokens(&["aws", "ec2", "create"]));
    }

    #[test]
    fn prefix_rule_any_star_matches_zero_tokens() {
        let rule = PrefixRule {
            id: Cow::Borrowed("T-011"),
            category: Category::Git,
            pattern: vec![single("git"), PatternToken::AnyStar, single("status")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &[],
        };
        assert!(rule.matches_tokens(&["git", "status"]));
        assert!(rule.matches_tokens(&["git", "log", "status"]));
        assert!(rule.matches_tokens(&["git", "a", "b", "c", "status"]));
        assert!(!rule.matches_tokens(&["git", "log"]));
    }

    #[test]
    fn prefix_rule_any_matches_one_token() {
        let rule = PrefixRule {
            id: Cow::Borrowed("T-012"),
            category: Category::Git,
            pattern: vec![single("git"), PatternToken::Any, single("status")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: None,
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &[],
        };
        assert!(rule.matches_tokens(&["git", "log", "status"]));
        assert!(!rule.matches_tokens(&["git", "status"])); // Any needs one token
        assert!(!rule.matches_tokens(&["git", "a", "b", "status"])); // Any matches exactly one
    }

    #[test]
    fn prefix_rule_to_match_result_copies_justification() {
        let rule = PrefixRule {
            id: Cow::Borrowed("T-013"),
            category: Category::Git,
            pattern: vec![single("git"), single("push"), single("--force")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: Some(Cow::Borrowed("rewrites remote history")),
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &[],
        };
        let result = rule.to_match_result(&["git", "push", "--force"]);
        assert_eq!(
            result.pattern.justification.as_deref(),
            Some("rewrites remote history")
        );
    }

    #[test]
    fn prefix_rule_validate_examples_detects_bad_match_example() {
        let rule = PrefixRule {
            id: Cow::Borrowed("BAD-001"),
            category: Category::Process,
            pattern: vec![single("rm")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: Some(Cow::Borrowed("test")),
            source: PatternSource::Builtin,
            match_examples: &["echo hello"],
            not_match_examples: &[],
        };
        assert!(
            rule.validate_examples().is_err(),
            "validate_examples must reject a rule whose match_examples do not actually match"
        );
    }

    #[test]
    fn prefix_rule_validate_examples_detects_bad_not_match_example() {
        let rule = PrefixRule {
            id: Cow::Borrowed("BAD-002"),
            category: Category::Process,
            pattern: vec![single("rm")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed("test"),
            safe_alt: None,
            justification: Some(Cow::Borrowed("test")),
            source: PatternSource::Builtin,
            match_examples: &[],
            not_match_examples: &["rm -rf /"],
        };
        assert!(
            rule.validate_examples().is_err(),
            "validate_examples must reject a rule whose not_match_examples actually match"
        );
    }
}
