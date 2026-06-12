//! Runtime evaluation of typed `[[rules]]` entries against a raw command string.

use aegis_config::{PolicyRule, policy_pattern_matches};
use aegis_parser::{logical_segments, split_tokens};
use aegis_policy::PolicyRulesResult;
use aegis_types::PolicyRuleDecision;

/// Evaluate `[[rules]]` entries against a raw command string.
///
/// - Tokenizes the first logical segment with the quote-aware parser.
/// - If the command is compound (more than one logical segment) and the
///   matching rule's effective decision is `Allow`, downgrades to `Prompt` so
///   the user reviews the full command chain.
/// - Returns the default (not matched) when no rule matches.
pub fn evaluate_policy_rules(rules: &[PolicyRule], cmd: &str) -> PolicyRulesResult {
    let segments = logical_segments(cmd);
    let first_segment: &str = segments.first().map(String::as_str).unwrap_or(cmd);
    let is_compound = segments.len() > 1;

    let token_strings: Vec<String> = split_tokens(first_segment);
    let tokens: Vec<&str> = token_strings.iter().map(String::as_str).collect();

    for rule in rules {
        if policy_pattern_matches(&rule.pattern, &tokens) {
            let decision = effective_decision(rule, is_compound);
            return PolicyRulesResult {
                matched: true,
                decision: Some(decision),
                justification: rule
                    .justification
                    .as_deref()
                    .map(|s| std::borrow::Cow::Owned(s.to_owned())),
            };
        }
    }
    PolicyRulesResult::default()
}

/// Resolve the final decision for a matched rule, applying `when`-clause and
/// compound-command downgrade.
///
/// Compound commands are never silently approved — a rule that would `allow`
/// a simple command can at most `prompt` when additional shell segments follow.
fn effective_decision(rule: &PolicyRule, is_compound: bool) -> PolicyRuleDecision {
    let base = if let Some(ref when) = rule.when {
        // Missing or non-Unicode env var → condition not met.
        let env_matches = std::env::var(&when.env)
            .map(|val| val == when.value)
            .unwrap_or(false);
        if env_matches { when.then } else { rule.decision }
    } else {
        rule.decision
    };

    if is_compound && base == PolicyRuleDecision::Allow {
        PolicyRuleDecision::Prompt
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use aegis_config::{PolicyPatternToken, PolicyRule, PolicyRuleDecision, WhenClause};

    use super::evaluate_policy_rules;

    fn single(s: &str) -> PolicyPatternToken {
        PolicyPatternToken::Single(s.to_string())
    }

    fn alts(v: &[&str]) -> PolicyPatternToken {
        PolicyPatternToken::Alts(v.iter().map(|s| s.to_string()).collect())
    }

    fn rule_with_pattern(
        pattern: Vec<PolicyPatternToken>,
        decision: PolicyRuleDecision,
    ) -> PolicyRule {
        PolicyRule {
            pattern,
            decision,
            justification: None,
            match_examples: vec![],
            not_match_examples: vec![],
            when: None,
        }
    }

    #[test]
    fn evaluate_policy_rules_matches_single_token_pattern() {
        let rules = vec![rule_with_pattern(
            vec![single("git"), single("push")],
            PolicyRuleDecision::Prompt,
        )];
        let result = evaluate_policy_rules(&rules, "git push origin main");
        assert!(result.matched);
        assert_eq!(result.decision, Some(PolicyRuleDecision::Prompt));
    }

    #[test]
    fn evaluate_policy_rules_alts_match() {
        let rules = vec![rule_with_pattern(
            vec![single("git"), single("push"), alts(&["--force", "-f"])],
            PolicyRuleDecision::Block,
        )];
        let result = evaluate_policy_rules(&rules, "git push -f origin");
        assert!(result.matched);
        assert_eq!(result.decision, Some(PolicyRuleDecision::Block));
    }

    #[test]
    fn evaluate_policy_rules_when_clause_overrides_on_env_match() {
        // SAFETY: test-only, single-threaded in this test context.
        unsafe { std::env::set_var("AEGIS_TEST_ENV_VAR_UNIQUE", "true") };
        let rules = vec![PolicyRule {
            pattern: vec![single("rm")],
            decision: PolicyRuleDecision::Prompt,
            justification: None,
            match_examples: vec![],
            not_match_examples: vec![],
            when: Some(WhenClause {
                env: "AEGIS_TEST_ENV_VAR_UNIQUE".to_string(),
                value: "true".to_string(),
                then: PolicyRuleDecision::Allow,
            }),
        }];
        let result = evaluate_policy_rules(&rules, "rm -rf build");
        assert!(result.matched);
        // env matches → use `then` (Allow)
        assert_eq!(result.decision, Some(PolicyRuleDecision::Allow));
        // SAFETY: test-only cleanup.
        unsafe { std::env::remove_var("AEGIS_TEST_ENV_VAR_UNIQUE") };
    }

    #[test]
    fn evaluate_policy_rules_when_clause_uses_base_on_env_mismatch() {
        // SAFETY: test-only, single-threaded in this test context.
        unsafe { std::env::remove_var("AEGIS_TEST_ENV_VAR_ABSENT") };
        let rules = vec![PolicyRule {
            pattern: vec![single("rm")],
            decision: PolicyRuleDecision::Prompt,
            justification: None,
            match_examples: vec![],
            not_match_examples: vec![],
            when: Some(WhenClause {
                env: "AEGIS_TEST_ENV_VAR_ABSENT".to_string(),
                value: "true".to_string(),
                then: PolicyRuleDecision::Allow,
            }),
        }];
        let result = evaluate_policy_rules(&rules, "rm -rf build");
        assert!(result.matched);
        // env does not match → use base decision (Prompt)
        assert_eq!(result.decision, Some(PolicyRuleDecision::Prompt));
    }

    #[test]
    fn evaluate_policy_rules_when_missing_env_empty_value_does_not_match() {
        // Ensure absent env var does NOT match value = "" via unwrap_or_default.
        unsafe { std::env::remove_var("AEGIS_TEST_ABSENT_EMPTY") };
        let rules = vec![PolicyRule {
            pattern: vec![single("ls")],
            decision: PolicyRuleDecision::Block,
            justification: None,
            match_examples: vec![],
            not_match_examples: vec![],
            when: Some(WhenClause {
                env: "AEGIS_TEST_ABSENT_EMPTY".to_string(),
                value: String::new(),
                then: PolicyRuleDecision::Allow,
            }),
        }];
        let result = evaluate_policy_rules(&rules, "ls");
        assert!(result.matched);
        // absent env → condition not met → base decision (Block)
        assert_eq!(result.decision, Some(PolicyRuleDecision::Block));
    }

    #[test]
    fn evaluate_policy_rules_no_match_returns_default() {
        let rules = vec![rule_with_pattern(
            vec![single("git"), single("push")],
            PolicyRuleDecision::Prompt,
        )];
        let result = evaluate_policy_rules(&rules, "echo hello");
        assert!(!result.matched);
        assert_eq!(result.decision, None);
    }

    #[test]
    fn compound_command_allow_rule_downgrades_to_prompt() {
        let rules = vec![rule_with_pattern(
            vec![single("git"), single("status")],
            PolicyRuleDecision::Allow,
        )];
        // Compound command — the git status part matches but tail is dangerous.
        let result = evaluate_policy_rules(&rules, "git status && terraform destroy");
        assert!(result.matched);
        // Allow must be downgraded to Prompt for compound commands.
        assert_eq!(result.decision, Some(PolicyRuleDecision::Prompt));
    }

    #[test]
    fn compound_command_block_rule_stays_block() {
        let rules = vec![rule_with_pattern(
            vec![single("git")],
            PolicyRuleDecision::Block,
        )];
        let result = evaluate_policy_rules(&rules, "git status && terraform destroy");
        assert!(result.matched);
        assert_eq!(result.decision, Some(PolicyRuleDecision::Block));
    }

    #[test]
    fn quoted_args_tokenized_correctly() {
        let rules = vec![rule_with_pattern(
            vec![single("git"), single("commit")],
            PolicyRuleDecision::Prompt,
        )];
        // The quoted message should be handled as one token by the quote-aware parser.
        let result = evaluate_policy_rules(&rules, r#"git commit -m "feat: add feature""#);
        assert!(result.matched);
    }
}
