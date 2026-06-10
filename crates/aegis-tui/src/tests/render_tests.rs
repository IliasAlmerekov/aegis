use super::*;

// ── Highlighting ──────────────────────────────────────────────────────────

#[test]
fn highlighting_wraps_match_in_ansi() {
    let p = make_match_with_text("FS-001", RiskLevel::Danger, r"rm\s+-rf", "desc", "rm -rf");
    let patterns = vec![p];
    let result = build_highlighted_command("rm -rf /home", &patterns);
    assert!(
        result.contains("\x1b[1;31m"),
        "highlighted output must contain bold-red ANSI code"
    );
    assert!(
        result.contains("rm -rf"),
        "the matched fragment must appear in the output"
    );
}

#[test]
fn highlighting_uses_scanner_matched_text_without_recompiling_regex() {
    let p = make_match_with_text("FS-001", RiskLevel::Danger, "(", "desc", "rm -rf");
    let result = build_highlighted_command("rm -rf /home", &[p]);

    assert!(
        result.contains("\x1b[1;31mrm -rf\x1b[0m"),
        "highlighting must use scanner-provided match metadata even when the pattern regex is unusable in the UI"
    );
}

#[test]
fn highlighting_does_not_duplicate_single_match_across_repeated_fragments() {
    let cmd = "rm -rf /tmp/one && echo safe && rm -rf /tmp/two";
    let start = cmd.rfind("rm -rf").unwrap();
    let p = make_match_with_text_and_range(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+-rf",
        "desc",
        "rm -rf",
        start,
    );

    let result = build_highlighted_command(cmd, &[p]);

    assert_eq!(
        result.matches("\x1b[1;31m").count(),
        1,
        "a single scanner match must highlight one concrete command span, not every identical fragment in the command"
    );
}

#[test]
fn highlighting_large_heredoc_like_input_marks_single_match_once() {
    let repeated_line = "rm -rf /tmp/cache\n";
    let mut cmd = String::from("cat <<'EOF'\n");
    for _ in 0..256 {
        cmd.push_str("echo keep\n");
    }
    for _ in 0..128 {
        cmd.push_str(repeated_line);
    }
    cmd.push_str("EOF");
    let start = cmd.rfind("rm -rf /tmp/cache").unwrap();
    let p = make_match_with_text_and_range(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+-rf",
        "desc",
        "rm -rf /tmp/cache",
        start,
    );

    let result = build_highlighted_command(&cmd, &[p]);

    assert_eq!(
        result.matches("\x1b[1;31m").count(),
        1,
        "large heredoc-like inputs must still honor the scanner's concrete match span instead of highlighting every repeated copy"
    );
}

#[test]
fn highlighting_no_match_returns_unchanged() {
    let p = make_match_with_text(
        "FS-001",
        RiskLevel::Danger,
        r"terraform",
        "desc",
        "terraform",
    );
    let patterns = vec![p];
    let cmd = "echo hello";
    let result = build_highlighted_command(cmd, &patterns);
    assert_eq!(result, cmd, "no match should return the command unchanged");
}

#[test]
fn highlighting_merges_overlapping_ranges() {
    // Two patterns that overlap on "rm -rf"
    let p1 = make_match_with_text("A", RiskLevel::Danger, r"rm\s+-rf /", "desc", "rm -rf /");
    let p2 = make_match_with_text("B", RiskLevel::Danger, r"rm\s+-rf", "desc", "rm -rf");
    let result = build_highlighted_command("rm -rf /home", &[p1, p2]);
    // Should not have double ANSI codes inside the overlap.
    let opens = result.matches("\x1b[1;31m").count();
    assert_eq!(
        opens, 1,
        "overlapping ranges must be merged into one highlight span"
    );
}

// ── /dev/tty helpers ──────────────────────────────────────────────────────

#[test]
fn tty_unavailable_safe_is_approved() {
    let assessment = make_assessment("ls -la", RiskLevel::Safe, vec![]);
    assert!(
        tty_unavailable_decision(&assessment),
        "Safe must be approved when /dev/tty is unavailable"
    );
}

#[test]
fn tty_unavailable_warn_is_denied() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);
    assert!(
        !tty_unavailable_decision(&assessment),
        "Warn must be denied when /dev/tty is unavailable"
    );
}

#[test]
fn tty_unavailable_danger_is_denied() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);
    assert!(
        !tty_unavailable_decision(&assessment),
        "Danger must be denied when /dev/tty is unavailable"
    );
}

#[test]
fn tty_unavailable_block_is_denied() {
    let p = make_match("PS-006", RiskLevel::Block, "rm", "Root delete", None);
    let assessment = make_assessment("rm -rf /", RiskLevel::Block, vec![p]);
    assert!(
        !tty_unavailable_decision(&assessment),
        "Block must be denied when /dev/tty is unavailable"
    );
}

// ── Justification rendering ───────────────────────────────────────────────

#[test]
fn render_dialog_shows_justification_when_present() {
    let p = make_match_with_justification(
        "GIT-003",
        RiskLevel::Warn,
        "git push --force",
        "git push --force — rewrites remote history",
        Some("--force-with-lease"),
        Some(
            "This command rewrites remote history. Collaborators with local copies will have diverged refs and will need to force-pull or re-clone.",
        ),
    );
    let assessment = make_assessment("git push --force origin main", RiskLevel::Warn, vec![p]);

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
        text.contains("rewrites remote history"),
        "dialog must show the justification text; got:\n{text}"
    );
}

#[test]
fn render_block_shows_justification_when_present() {
    let p = make_match_with_justification(
        "PS-006",
        RiskLevel::Block,
        "rm -rf /",
        "Deletes the entire root filesystem",
        None,
        Some(
            "This command recursively and forcefully deletes everything on the root filesystem. It will brick the machine immediately and permanently.",
        ),
    );
    let assessment = make_assessment("rm -rf /", RiskLevel::Block, vec![p]);

    let mut output = Vec::new();
    show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"".as_ref(),
        &mut output,
    );

    let text = strip_ansi(&String::from_utf8_lossy(&output));
    assert!(
        text.contains("brick the machine"),
        "block screen must show the justification text; got:\n{text}"
    );
}

#[test]
fn render_dialog_does_not_show_justification_when_absent() {
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
        !text.contains("Justification:"),
        "dialog must not synthesize a justification label when absent; got:\n{text}"
    );
}

#[test]
fn danger_deny_always_returns_deny_always() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let decision = crate::stdout_renderer::show_confirmation_with_decision(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"d\n".as_ref(),
        &mut Vec::new(),
    );
    assert_eq!(
        decision,
        PromptDecision::DenyAlways,
        "'d' must return DenyAlways"
    );
}

#[test]
fn danger_deny_always_alias_returns_deny_always() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let decision = crate::stdout_renderer::show_confirmation_with_decision(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"denyalways\n".as_ref(),
        &mut Vec::new(),
    );
    assert_eq!(
        decision,
        PromptDecision::DenyAlways,
        "'denyalways' must return DenyAlways"
    );
}

#[test]
fn warn_deny_always_returns_deny_always() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

    let decision = crate::stdout_renderer::show_confirmation_with_decision(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"d\n".as_ref(),
        &mut Vec::new(),
    );
    assert_eq!(
        decision,
        PromptDecision::DenyAlways,
        "'d' must return DenyAlways for Warn"
    );
}

#[test]
fn deny_always_is_not_approved_by_show_confirmation_with_input() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);

    let approved = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"d\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(
        !approved,
        "DenyAlways must not be treated as approved by show_confirmation_with_input"
    );
}
