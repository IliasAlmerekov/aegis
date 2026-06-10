use super::*;
// ── Block ─────────────────────────────────────────────────────────────────

#[test]
fn block_always_returns_false() {
    let p = make_match(
        "PS-006",
        RiskLevel::Block,
        "rm",
        "Deletes root filesystem",
        None,
    );
    let assessment = make_assessment("rm -rf /", RiskLevel::Block, vec![p]);

    // Even if the user somehow types "yes", Block must return false.
    let result = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"yes\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(!result, "Block must always return false");
}

#[test]
fn block_output_contains_reason() {
    let p = make_match(
        "PS-006",
        RiskLevel::Block,
        "rm",
        "Kills the root filesystem",
        None,
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
        text.contains("Kills the root filesystem"),
        "Block output must contain the pattern description; got:\n{text}"
    );
}

#[test]
fn block_output_contains_command() {
    let p = make_match("PS-006", RiskLevel::Block, "rm", "Root delete", None);
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
        text.contains("rm -rf /"),
        "Block output must contain the command; got:\n{text}"
    );
}
