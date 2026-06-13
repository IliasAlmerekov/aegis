//! Authoritative implementation of policy pattern matching.
//!
//! Used by both the config validator and the runtime rule evaluator to ensure
//! consistent case-sensitive prefix matching semantics.

use crate::model::PolicyPatternToken;

/// Returns `true` if `cmd_tokens` starts with the given `pattern` (prefix
/// match, exact case).
///
/// - [`PolicyPatternToken::Single`] requires an exact (`==`) match.
/// - [`PolicyPatternToken::Alts`] requires that at least one alternative
///   exactly matches the corresponding command token.
///
/// An empty pattern never matches. Extra trailing command tokens beyond the
/// pattern length are ignored.
pub fn policy_pattern_matches(pattern: &[PolicyPatternToken], cmd_tokens: &[&str]) -> bool {
    if pattern.is_empty() {
        return false;
    }
    if pattern.len() > cmd_tokens.len() {
        return false;
    }
    pattern
        .iter()
        .zip(cmd_tokens.iter())
        .all(|(pat, tok)| match pat {
            PolicyPatternToken::Single(s) => s == tok,
            PolicyPatternToken::Alts(alts) => alts.iter().any(|a| a == tok),
        })
}

#[cfg(test)]
mod tests {
    use super::policy_pattern_matches;
    use crate::model::PolicyPatternToken;

    fn single(s: &str) -> PolicyPatternToken {
        PolicyPatternToken::Single(s.to_string())
    }

    fn alts(v: &[&str]) -> PolicyPatternToken {
        PolicyPatternToken::Alts(v.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn empty_pattern_never_matches() {
        assert!(!policy_pattern_matches(&[], &["git", "push"]));
    }

    #[test]
    fn pattern_longer_than_tokens_does_not_match() {
        assert!(!policy_pattern_matches(
            &[single("git"), single("push")],
            &["git"]
        ));
    }

    #[test]
    fn exact_prefix_matches() {
        assert!(policy_pattern_matches(
            &[single("git"), single("push")],
            &["git", "push", "origin", "main"]
        ));
    }

    #[test]
    fn exact_case_required_for_single_tokens() {
        assert!(!policy_pattern_matches(
            &[single("Git"), single("Push")],
            &["git", "push"]
        ));
        assert!(!policy_pattern_matches(
            &[single("git"), single("push")],
            &["Git", "Push"]
        ));
    }

    #[test]
    fn alts_matches_any_exact_alternative() {
        assert!(policy_pattern_matches(
            &[single("git"), single("push"), alts(&["--force", "-f"])],
            &["git", "push", "--force"]
        ));
        assert!(policy_pattern_matches(
            &[single("git"), single("push"), alts(&["--force", "-f"])],
            &["git", "push", "-f"]
        ));
    }

    #[test]
    fn alts_case_sensitive() {
        assert!(!policy_pattern_matches(
            &[single("git"), single("push"), alts(&["--force", "-f"])],
            &["git", "push", "--Force"]
        ));
    }
}
