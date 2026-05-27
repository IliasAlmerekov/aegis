use std::borrow::Cow;
use std::sync::Arc;

use crate::interceptor::patterns::PrefixRule;
use crate::interceptor::scanner::MatchResult;

/// Compare two tokens for prefix-rule equality.
///
/// Shell flags (tokens beginning with `-`) are compared case-sensitively;
/// everything else is compared case-insensitively so that SQL keywords and
/// command names match regardless of casing.
fn str_eq_maybe_case(a: &str, b: &str) -> bool {
    if a.starts_with('-') || b.starts_with('-') {
        a == b
    } else {
        a.eq_ignore_ascii_case(b)
    }
}

impl PrefixRule {
    /// Check whether `tokens` matches this rule's prefix pattern.
    ///
    /// Supports [`crate::interceptor::patterns::PatternToken::Single`],
    /// [`crate::interceptor::patterns::PatternToken::Alts`],
    /// [`crate::interceptor::patterns::PatternToken::Any`] and
    /// [`crate::interceptor::patterns::PatternToken::AnyStar`].
    /// The pattern must be a prefix of `tokens` — extra trailing tokens are allowed.
    /// Empty patterns never match.
    pub fn matches_tokens(&self, tokens: &[&str]) -> bool {
        if self.pattern.is_empty() {
            return false;
        }
        self.matches_from(tokens, 0)
    }

    fn matches_from(&self, tokens: &[&str], pat_idx: usize) -> bool {
        if pat_idx == self.pattern.len() {
            return true;
        }
        match &self.pattern[pat_idx] {
            crate::interceptor::patterns::PatternToken::Single(s) => {
                if tokens.is_empty() || !str_eq_maybe_case(tokens[0], s.as_ref()) {
                    return false;
                }
                self.matches_from(&tokens[1..], pat_idx + 1)
            }
            crate::interceptor::patterns::PatternToken::Alts(alts) => {
                if tokens.is_empty()
                    || !alts
                        .iter()
                        .any(|a| str_eq_maybe_case(tokens[0], a.as_ref()))
                {
                    return false;
                }
                self.matches_from(&tokens[1..], pat_idx + 1)
            }
            crate::interceptor::patterns::PatternToken::Any => {
                if tokens.is_empty() {
                    return false;
                }
                self.matches_from(&tokens[1..], pat_idx + 1)
            }
            crate::interceptor::patterns::PatternToken::AnyStar => {
                for skip in 0..=tokens.len() {
                    if self.matches_from(&tokens[skip..], pat_idx + 1) {
                        return true;
                    }
                }
                false
            }
        }
    }

    /// Produce a [`MatchResult`] for this rule when it matched `tokens`.
    ///
    /// `matched_text` joins the consumed tokens (up to the pattern length, ignoring
    /// wildcards for span purposes).
    pub fn to_match_result(&self, tokens: &[&str]) -> MatchResult {
        let consumed = tokens.join(" ");
        MatchResult {
            pattern: Arc::new(crate::interceptor::patterns::Pattern {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interceptor::RiskLevel;
    use crate::interceptor::patterns::{Category, PatternSource, PatternToken, PrefixRule};

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
        };
        let result = rule.to_match_result(&["git", "push", "--force"]);
        assert_eq!(
            result.pattern.justification.as_deref(),
            Some("rewrites remote history")
        );
    }
}
