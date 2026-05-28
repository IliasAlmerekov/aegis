use super::*;
// ── Always allow option ───────────────────────────────────────────────────

#[test]
fn danger_always_allow_returns_approve_always() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let decision = super::stdout_renderer::show_confirmation_with_decision(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"a\n".as_ref(),
        &mut Vec::new(),
    );
    assert_eq!(
        decision,
        PromptDecision::ApproveAlways,
        "'a' must return ApproveAlways"
    );
}

#[test]
fn danger_y_still_returns_approve() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let decision = super::stdout_renderer::show_confirmation_with_decision(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"y\n".as_ref(),
        &mut Vec::new(),
    );
    assert_eq!(decision, PromptDecision::Approve, "'y' must return Approve");
}

#[test]
fn danger_deny_returns_deny() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let decision = super::stdout_renderer::show_confirmation_with_decision(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"n\n".as_ref(),
        &mut Vec::new(),
    );
    assert_eq!(decision, PromptDecision::Deny, "'n' must return Deny");
}

#[test]
fn warn_always_allow_returns_approve_always() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

    let decision = super::stdout_renderer::show_confirmation_with_decision(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"always\n".as_ref(),
        &mut Vec::new(),
    );
    assert_eq!(
        decision,
        PromptDecision::ApproveAlways,
        "'always' must return ApproveAlways"
    );
}

#[test]
fn danger_prompt_shows_always_allow_hint() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let mut output = Vec::new();
    super::stdout_renderer::show_confirmation_with_decision(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"n\n".as_ref(),
        &mut output,
    );

    let text = strip_ansi(&String::from_utf8_lossy(&output));
    assert!(
        text.contains("[y/N/a/d]:"),
        "Danger dialog must show [y/N/a/d] prompt; got:\n{text}"
    );
}

#[test]
fn warn_prompt_shows_always_allow_hint() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

    let mut output = Vec::new();
    super::stdout_renderer::show_confirmation_with_decision(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"n\n".as_ref(),
        &mut output,
    );

    let text = strip_ansi(&String::from_utf8_lossy(&output));
    assert!(
        text.contains("[y/N/a/d]:"),
        "Warn dialog must show [y/N/a/d] prompt; got:\n{text}"
    );
}

#[test]
fn noninteractive_danger_returns_deny_via_decision() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let decision = super::stdout_renderer::show_confirmation_with_decision(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        false,
        &mut b"a\n".as_ref(),
        &mut Vec::new(),
    );
    assert_eq!(
        decision,
        PromptDecision::Deny,
        "non-interactive mode must deny even with 'a' input"
    );
}

#[test]
fn block_returns_deny_via_decision() {
    let p = make_match("PS-006", RiskLevel::Block, "rm", "Root delete", None);
    let assessment = make_assessment("rm -rf /", RiskLevel::Block, vec![p]);

    let decision = super::stdout_renderer::show_confirmation_with_decision(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"a\n".as_ref(),
        &mut Vec::new(),
    );
    assert_eq!(
        decision,
        PromptDecision::Deny,
        "Block must always return Deny"
    );
}

// ── Warn ──────────────────────────────────────────────────────────────────

#[test]
fn warn_enter_denies() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

    let denied = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(!denied, "Enter must deny a Warn command");
}

#[test]
fn warn_y_approves() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

    let approved = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"y\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(approved, "'y' must approve a Warn command");
}

#[test]
fn warn_uppercase_y_approves() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

    let approved = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"Y\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(approved, "'Y' must approve a Warn command");
}

#[test]
fn warn_yes_approves_after_trim() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

    let approved = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b" yes \n".as_ref(),
        &mut Vec::new(),
    );
    assert!(approved, "' yes ' must approve a Warn command");
}

#[test]
fn warn_n_denies() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

    let denied = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"n\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(!denied, "'n' must deny a Warn command");
}

#[test]
fn warn_no_denies() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

    let denied = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"no\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(!denied, "'no' must deny a Warn command");
}

#[test]
fn warn_any_other_input_denies() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

    for input in [b"maybe\n".as_ref(), b"1\n".as_ref(), b"ok\n".as_ref()] {
        let denied = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut { input },
            &mut Vec::new(),
        );
        assert!(!denied, "unexpected input must deny a Warn command");
    }
}

#[test]
fn warn_output_contains_explicit_yes_no_prompt() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

    let mut output = Vec::new();
    show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"no\n".as_ref(),
        &mut output,
    );

    let text = strip_ansi(&String::from_utf8_lossy(&output));
    assert!(
        text.contains("Execute suspicious command? [y/N/a/d]:"),
        "Warn dialog must use the explicit yes/no/always prompt; got:\n{text}"
    );
}

// ── Dialog content ────────────────────────────────────────────────────────

#[test]
fn danger_output_contains_command() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let mut output = Vec::new();
    show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"no\n".as_ref(),
        &mut output,
    );

    let text = strip_ansi(&String::from_utf8_lossy(&output));
    assert!(
        text.contains("rm -rf /home/user"),
        "dialog must show the full command; got:\n{text}"
    );
}

#[test]
fn danger_output_contains_pattern_description() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive force delete",
        Some("git clean -fd"),
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let mut output = Vec::new();
    show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"no\n".as_ref(),
        &mut output,
    );

    let text = strip_ansi(&String::from_utf8_lossy(&output));
    assert!(
        text.contains("Recursive force delete"),
        "dialog must show pattern description; got:\n{text}"
    );
    assert!(
        text.contains("git clean -fd"),
        "dialog must show safe_alt suggestion; got:\n{text}"
    );
}

#[test]
fn danger_output_contains_dangerous_action_section() {
    let p = make_match_with_text(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+-rf\s+/var/tmp/cache",
        "Recursive force delete",
        "rm -rf /var/tmp/cache",
    );
    let assessment = make_assessment("sudo rm -rf /var/tmp/cache", RiskLevel::Danger, vec![p]);

    let mut output = Vec::new();
    show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"no\n".as_ref(),
        &mut output,
    );

    let text = strip_ansi(&String::from_utf8_lossy(&output));
    assert!(
        text.contains("Dangerous action:"),
        "dialog must show a dedicated dangerous action section; got:\n{text}"
    );
    assert!(
        text.contains("rm -rf /var/tmp/cache"),
        "dialog must show the dangerous action fragment; got:\n{text}"
    );
}

#[test]
fn danger_output_contains_explicit_yes_no_prompt() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let mut output = Vec::new();
    show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"no\n".as_ref(),
        &mut output,
    );

    let text = strip_ansi(&String::from_utf8_lossy(&output));
    assert!(
        text.contains("Execute dangerous command? [y/N/a/d]:"),
        "dialog must use the explicit yes/no/always prompt; got:\n{text}"
    );
}

#[test]
fn dialog_shows_snapshot_records() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);
    let snap = SnapshotRecord {
        plugin: "git",
        snapshot_id: "stash@{0}".to_string(),
    };

    let mut output = Vec::new();
    show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[snap],
        true,
        &mut b"no\n".as_ref(),
        &mut output,
    );

    let text = strip_ansi(&String::from_utf8_lossy(&output));
    assert!(
        text.contains("git") && text.contains("stash@{0}"),
        "dialog must list snapshot records; got:\n{text}"
    );
}

// ── Non-interactive mode ──────────────────────────────────────────────────
//
// When stdin is not a TTY (CI, pipes, agent runners) Aegis must fail-closed:
// any command that would trigger a prompt is denied without asking.
//
// Rule table:
//   Safe   → auto-approved  (same as interactive)
//   Warn   → denied         (no human present to confirm)
//   Danger → denied         (no human present to confirm)
//   Block  → denied         (same as interactive — always hard-stopped)

#[test]
fn noninteractive_warn_is_denied() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

    // Even with an "approving" response on stdin, is_interactive=false must deny.
    let result = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        false,
        &mut b"\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(!result, "Warn must be denied in non-interactive mode");
}

#[test]
fn noninteractive_danger_is_denied() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let result = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        false,
        &mut b"yes\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(!result, "Danger must be denied in non-interactive mode");
}

#[test]
fn noninteractive_block_is_denied() {
    let p = make_match("PS-006", RiskLevel::Block, "rm", "Root delete", None);
    let assessment = make_assessment("rm -rf /", RiskLevel::Block, vec![p]);

    let result = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        false,
        &mut b"yes\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(!result, "Block must be denied in non-interactive mode");
}

#[test]
fn noninteractive_output_explains_denial() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let mut output = Vec::new();
    show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        false,
        &mut b"yes\n".as_ref(),
        &mut output,
    );

    let text = strip_ansi(&String::from_utf8_lossy(&output));
    assert!(
        text.contains("non-interactive"),
        "non-interactive denial must mention 'non-interactive'; got:\n{text}"
    );
    assert!(
        text.contains("allowlist"),
        "non-interactive denial must mention 'allowlist' as the escape hatch; got:\n{text}"
    );
}

#[test]
fn noninteractive_safe_is_still_approved() {
    // Safe commands must never be blocked regardless of TTY state.
    let assessment = make_assessment("ls -la", RiskLevel::Safe, vec![]);
    let result = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        false,
        &mut b"".as_ref(),
        &mut Vec::new(),
    );
    assert!(
        result,
        "Safe commands must be approved even in non-interactive mode"
    );
}
