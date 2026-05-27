use aegis::interceptor::patterns::PatternSet;

#[test]
fn builtin_prefix_rules_git_push_has_justification() {
    let set = PatternSet::load().unwrap();
    let rule = set
        .prefix_rules()
        .iter()
        .find(|r| r.id.as_ref() == "GIT-003")
        .expect("GIT-003 should exist");
    let text = rule
        .justification
        .as_deref()
        .expect("GIT-003 justification should be Some");
    assert!(!text.is_empty(), "GIT-003 justification must not be empty");
}

#[test]
fn builtin_prefix_rules_git_reset_hard_has_justification() {
    let set = PatternSet::load().unwrap();
    let rule = set
        .prefix_rules()
        .iter()
        .find(|r| r.id.as_ref() == "GIT-001")
        .expect("GIT-001 should exist");
    assert!(
        rule.justification.is_some(),
        "GIT-001 (git reset --hard) should have a non-empty justification"
    );
}

#[test]
fn builtin_prefix_rules_kill_sigkill_pid1_has_justification() {
    let set = PatternSet::load().unwrap();
    let rule = set
        .prefix_rules()
        .iter()
        .find(|r| r.id.as_ref() == "PS-001")
        .expect("PS-001 should exist");
    assert!(
        rule.justification.is_some(),
        "PS-001 (kill -9 1) should have a non-empty justification"
    );
}

#[test]
fn builtin_prefix_rules_danger_rules_have_justification() {
    let set = PatternSet::load().unwrap();
    let danger_rules: Vec<_> = set
        .prefix_rules()
        .iter()
        .filter(|r| {
            matches!(
                r.risk,
                aegis::interceptor::RiskLevel::Danger | aegis::interceptor::RiskLevel::Block
            )
        })
        .collect();
    assert!(
        !danger_rules.is_empty(),
        "there should be at least one Danger/Block prefix rule"
    );
    for rule in danger_rules {
        assert!(
            rule.justification.is_some(),
            "{} ({:?}) should have a justification because it is {:?}",
            rule.id,
            rule.description,
            rule.risk
        );
    }
}

#[test]
fn builtin_prefix_rules_warn_and_danger_have_match_examples() {
    let set = PatternSet::load().unwrap();
    for rule in set.prefix_rules() {
        if matches!(
            rule.risk,
            aegis::interceptor::RiskLevel::Warn
                | aegis::interceptor::RiskLevel::Danger
                | aegis::interceptor::RiskLevel::Block
        ) {
            assert!(
                !rule.match_examples.is_empty(),
                "{} ({:?}) is {:?} and must have match_examples",
                rule.id,
                rule.description,
                rule.risk
            );
        }
    }
}

#[test]
fn builtin_prefix_rules_all_have_not_match_examples() {
    let set = PatternSet::load().unwrap();
    for rule in set.prefix_rules() {
        assert!(
            !rule.not_match_examples.is_empty(),
            "{} ({:?}) must have not_match_examples",
            rule.id,
            rule.description
        );
    }
}
