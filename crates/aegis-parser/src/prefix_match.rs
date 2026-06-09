//! Token-level prefix-pattern matching.
//!
//! The algorithm operates purely on a [`PrefixPattern`] (a sequence of
//! [`PatternToken`]s) and a slice of command tokens. It is decoupled from any
//! rule or scanner type so it can live at the parser layer; rule types graft
//! their own metadata on top by delegating here.

use aegis_types::{PatternToken, PrefixPattern};

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

/// Check whether `tokens` matches `pattern` as a token prefix.
///
/// Supports [`PatternToken::Single`], [`PatternToken::Alts`],
/// [`PatternToken::Any`] and [`PatternToken::AnyStar`]. The pattern must be a
/// prefix of `tokens` — extra trailing tokens are allowed. Empty patterns never
/// match.
pub fn matches_prefix(pattern: &PrefixPattern, tokens: &[&str]) -> bool {
    if pattern.is_empty() {
        return false;
    }
    matches_from(pattern, tokens, 0)
}

fn matches_from(pattern: &PrefixPattern, tokens: &[&str], pat_idx: usize) -> bool {
    if pat_idx == pattern.len() {
        return true;
    }
    match &pattern[pat_idx] {
        PatternToken::Single(s) => {
            if tokens.is_empty() || !str_eq_maybe_case(tokens[0], s.as_ref()) {
                return false;
            }
            matches_from(pattern, &tokens[1..], pat_idx + 1)
        }
        PatternToken::Alts(alts) => {
            if tokens.is_empty()
                || !alts
                    .iter()
                    .any(|a| str_eq_maybe_case(tokens[0], a.as_ref()))
            {
                return false;
            }
            matches_from(pattern, &tokens[1..], pat_idx + 1)
        }
        PatternToken::Any => {
            if tokens.is_empty() {
                return false;
            }
            matches_from(pattern, &tokens[1..], pat_idx + 1)
        }
        PatternToken::AnyStar => {
            for skip in 0..=tokens.len() {
                if matches_from(pattern, &tokens[skip..], pat_idx + 1) {
                    return true;
                }
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::matches_prefix;
    use aegis_types::PatternToken;
    use std::borrow::Cow;

    fn single(s: &'static str) -> PatternToken {
        PatternToken::Single(Cow::Borrowed(s))
    }

    fn alts(alts: &[&'static str]) -> PatternToken {
        PatternToken::Alts(alts.iter().map(|&s| Cow::Borrowed(s)).collect())
    }

    #[test]
    fn empty_pattern_never_matches() {
        assert!(!matches_prefix(&vec![], &["anything"]));
        assert!(!matches_prefix(&vec![], &[]));
    }

    #[test]
    fn single_token_matches_as_prefix_with_trailing_tokens() {
        assert!(matches_prefix(&vec![single("rm")], &["rm", "-rf", "/"]));
    }

    #[test]
    fn fails_on_insufficient_tokens() {
        let pattern = vec![single("git"), single("push"), single("origin")];
        assert!(!matches_prefix(&pattern, &["git", "push"]));
    }

    #[test]
    fn alts_matches_any_alternative() {
        let pattern = vec![single("git"), single("push"), alts(&["--force", "-f"])];
        assert!(matches_prefix(&pattern, &["git", "push", "--force"]));
        assert!(matches_prefix(&pattern, &["git", "push", "-f"]));
        assert!(!matches_prefix(&pattern, &["git", "push", "--dry-run"]));
    }

    #[test]
    fn commands_are_case_insensitive_but_flags_are_case_sensitive() {
        assert!(matches_prefix(&vec![single("Git")], &["git"]));
        let flag = vec![single("git"), single("branch"), single("-D")];
        assert!(matches_prefix(&flag, &["git", "branch", "-D"]));
        assert!(!matches_prefix(&flag, &["git", "branch", "-d"]));
    }

    #[test]
    fn any_matches_exactly_one_token() {
        let pattern = vec![single("git"), PatternToken::Any, single("status")];
        assert!(matches_prefix(&pattern, &["git", "log", "status"]));
        assert!(!matches_prefix(&pattern, &["git", "status"]));
        assert!(!matches_prefix(&pattern, &["git", "a", "b", "status"]));
    }

    #[test]
    fn any_star_matches_zero_or_more_tokens() {
        let pattern = vec![single("git"), PatternToken::AnyStar, single("status")];
        assert!(matches_prefix(&pattern, &["git", "status"]));
        assert!(matches_prefix(&pattern, &["git", "log", "status"]));
        assert!(matches_prefix(&pattern, &["git", "a", "b", "c", "status"]));
        assert!(!matches_prefix(&pattern, &["git", "log"]));
    }
}
