//! Integration tests for `load_starlark_policy`.
//!
//! Every test writes a temporary `.star` file, calls the loader, and asserts
//! the expected outcome. All tests must FAIL until the implementation is
//! written — `load_starlark_policy` currently panics with `todo!()`.

use std::io::Write as _;

use aegis_config::{PolicyPatternToken, PolicyRuleDecision};
use aegis_starlark::{StarlarkPolicyError, load_starlark_policy};
use tempfile::NamedTempFile;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write `content` to a temporary `.star` file and return the handle so the
/// file stays alive for the duration of the test.
fn star_file(content: &str) -> NamedTempFile {
    let mut f = NamedTempFile::with_suffix(".star").expect("tempfile creation must succeed");
    f.write_all(content.as_bytes())
        .expect("writing to tempfile must succeed");
    f
}

// ---------------------------------------------------------------------------
// Happy-path tests
// ---------------------------------------------------------------------------

/// An empty `.star` file must yield an empty `Vec<PolicyRule>` — no error.
#[test]
fn test_load_empty_file() {
    let f = star_file("");
    let rules = load_starlark_policy(f.path()).expect("empty .star file must parse without error");
    assert!(
        rules.is_empty(),
        "expected empty vec from empty file, got {rules:?}"
    );
}

/// A single `prefix_rule` with only `pattern` and `decision` must produce one
/// `PolicyRule` with the correct pattern tokens and decision.
#[test]
fn test_load_single_prefix_rule() {
    let src = r#"
prefix_rule(
    pattern  = ["git", "push"],
    decision = "prompt",
)
"#;
    let f = star_file(src);
    let rules =
        load_starlark_policy(f.path()).expect("single prefix_rule must parse without error");

    assert_eq!(rules.len(), 1, "expected exactly 1 rule, got {rules:?}");

    let rule = &rules[0];
    assert_eq!(
        rule.pattern,
        vec![
            PolicyPatternToken::Single("git".to_string()),
            PolicyPatternToken::Single("push".to_string()),
        ],
        "pattern tokens mismatch"
    );
    assert_eq!(
        rule.decision,
        PolicyRuleDecision::Prompt,
        "decision mismatch"
    );
    assert!(rule.justification.is_none(), "justification must be None");
    assert!(rule.when.is_none(), "when must be None");
}

/// A rule with `justification` must have that string preserved in the output.
#[test]
fn test_load_rule_with_justification() {
    let src = r#"
prefix_rule(
    pattern       = ["rm", "-rf"],
    decision      = "block",
    justification = "Recursive deletion is dangerous.",
)
"#;
    let f = star_file(src);
    let rules =
        load_starlark_policy(f.path()).expect("rule with justification must parse without error");

    assert_eq!(rules.len(), 1);
    let rule = &rules[0];
    assert_eq!(
        rule.justification.as_deref(),
        Some("Recursive deletion is dangerous."),
        "justification must be preserved exactly"
    );
    assert_eq!(rule.decision, PolicyRuleDecision::Block);
}

/// A rule with a `when` clause must produce a populated `WhenClause`.
#[test]
fn test_load_rule_with_when_clause() {
    let src = r#"
prefix_rule(
    pattern  = ["docker", "run"],
    decision = "prompt",
    when     = {"env": "CI", "value": "true", "then": "allow"},
)
"#;
    let f = star_file(src);
    let rules =
        load_starlark_policy(f.path()).expect("rule with when clause must parse without error");

    assert_eq!(rules.len(), 1);
    let rule = &rules[0];
    let when = rule.when.as_ref().expect("when clause must be populated");
    assert_eq!(when.env, "CI", "when.env mismatch");
    assert_eq!(when.value, "true", "when.value mismatch");
    assert_eq!(when.then, PolicyRuleDecision::Allow, "when.then mismatch");
}

/// `match_examples` and `not_match_examples` must be captured in the rule.
#[test]
fn test_load_rule_with_examples() {
    let src = r#"
prefix_rule(
    pattern            = ["git", "push", ["--force", "-f"]],
    decision           = "prompt",
    match_examples     = ["git push --force origin main", "git push -f origin main"],
    not_match_examples = ["git push origin main"],
)
"#;
    let f = star_file(src);
    let rules =
        load_starlark_policy(f.path()).expect("rule with examples must parse without error");

    assert_eq!(rules.len(), 1);
    let rule = &rules[0];

    assert_eq!(
        rule.match_examples,
        vec![
            "git push --force origin main".to_string(),
            "git push -f origin main".to_string(),
        ],
        "match_examples mismatch"
    );
    assert_eq!(
        rule.not_match_examples,
        vec!["git push origin main".to_string()],
        "not_match_examples mismatch"
    );
}

/// Multiple `prefix_rule` calls must all appear in the returned `Vec`, in
/// source order.
#[test]
fn test_load_multiple_rules() {
    let src = r#"
prefix_rule(
    pattern  = ["git", "push", ["--force", "-f"]],
    decision = "prompt",
    justification = "Force-push rewrites remote history.",
    match_examples     = ["git push --force origin main"],
    not_match_examples = ["git push origin main"],
)

prefix_rule(
    pattern  = ["rm", "-rf", "/"],
    decision = "block",
)

prefix_rule(
    pattern  = ["docker", "run"],
    decision = "prompt",
    when     = {"env": "CI", "value": "true", "then": "allow"},
)
"#;
    let f = star_file(src);
    let rules = load_starlark_policy(f.path())
        .expect("multiple prefix_rule calls must parse without error");

    assert_eq!(rules.len(), 3, "expected 3 rules, got {rules:?}");

    assert_eq!(rules[0].decision, PolicyRuleDecision::Prompt);
    assert_eq!(
        rules[0].justification.as_deref(),
        Some("Force-push rewrites remote history.")
    );

    assert_eq!(rules[1].decision, PolicyRuleDecision::Block);
    assert!(rules[1].justification.is_none());

    assert_eq!(rules[2].decision, PolicyRuleDecision::Prompt);
    assert!(rules[2].when.is_some());
}

/// An alternatives token `["--force", "-f"]` in the pattern must deserialise
/// into a `PolicyPatternToken::Alts` variant.
#[test]
fn test_load_rule_with_alts_token() {
    let src = r#"
prefix_rule(
    pattern  = ["git", "push", ["--force", "-f"]],
    decision = "prompt",
)
"#;
    let f = star_file(src);
    let rules =
        load_starlark_policy(f.path()).expect("rule with alts token must parse without error");

    assert_eq!(rules.len(), 1);
    let rule = &rules[0];
    assert_eq!(rule.pattern.len(), 3, "pattern must have 3 tokens");
    assert_eq!(
        rule.pattern[2],
        PolicyPatternToken::Alts(vec!["--force".to_string(), "-f".to_string()]),
        "third token must be Alts variant"
    );
}

// ---------------------------------------------------------------------------
// Error-path tests
// ---------------------------------------------------------------------------

/// Starlark with a syntax error must return `StarlarkPolicyError::ParseError`.
#[test]
fn test_load_invalid_syntax() {
    let src = "this is not valid starlark @@@ !!!";
    let f = star_file(src);
    let result = load_starlark_policy(f.path());

    assert!(
        result.is_err(),
        "invalid Starlark syntax must produce an error"
    );
    assert!(
        matches!(result, Err(StarlarkPolicyError::ParseError(_))),
        "error must be ParseError, got {result:?}"
    );
}

/// A `decision` string that is not one of the valid variants must return
/// `StarlarkPolicyError::InvalidDecision`.
#[test]
fn test_load_invalid_decision() {
    let src = r#"
prefix_rule(
    pattern  = ["git", "push"],
    decision = "yolo",
)
"#;
    let f = star_file(src);
    let result = load_starlark_policy(f.path());

    assert!(
        result.is_err(),
        "unknown decision value must produce an error"
    );
    assert!(
        matches!(result, Err(StarlarkPolicyError::InvalidDecision(_))),
        "error must be InvalidDecision, got {result:?}"
    );
}

/// A `prefix_rule` call with no `pattern` argument must return
/// `StarlarkPolicyError::MissingField`.
#[test]
fn test_load_missing_pattern() {
    let src = r#"
prefix_rule(
    decision = "block",
)
"#;
    let f = star_file(src);
    let result = load_starlark_policy(f.path());

    assert!(
        result.is_err(),
        "missing pattern field must produce an error"
    );
    assert!(
        matches!(result, Err(StarlarkPolicyError::MissingField(_))),
        "error must be MissingField, got {result:?}"
    );
}

/// Passing a path that does not exist must return `StarlarkPolicyError::Io`.
#[test]
fn test_load_nonexistent_file() {
    let path = std::path::Path::new("/tmp/aegis_starlark_does_not_exist_xyzzy.star");
    let result = load_starlark_policy(path);

    assert!(result.is_err(), "nonexistent file must produce an error");
    assert!(
        matches!(result, Err(StarlarkPolicyError::Io(_))),
        "error must be Io, got {result:?}"
    );
}
