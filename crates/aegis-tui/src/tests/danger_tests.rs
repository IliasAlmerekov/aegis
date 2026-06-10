use super::*;
// ── Danger ────────────────────────────────────────────────────────────────

#[test]
fn danger_yes_approves() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive force delete",
        Some("git clean -fd"),
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let approved = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"yes\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(approved, "'yes' must approve a Danger command");
}

#[test]
fn danger_y_approves() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let denied = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"y\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(denied, "'y' must approve a Danger command");
}

#[test]
fn danger_uppercase_y_approves() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let approved = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"Y\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(approved, "'Y' must approve a Danger command");
}

#[test]
fn danger_empty_does_not_approve() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let denied = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(!denied, "empty Enter must NOT approve a Danger command");
}

#[test]
fn danger_anything_else_denies() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    for input in [b"nope\n".as_ref(), b"ok\n".as_ref(), b"cancel\n".as_ref()] {
        let denied = show_confirmation_with_input(
            &assessment,
            &default_explanation_for_assessment(&assessment),
            &[],
            true,
            &mut { input },
            &mut Vec::new(),
        );
        assert!(!denied, "only 'yes' approves; other inputs must deny");
    }
}

#[test]
fn danger_no_denies() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let denied = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"no\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(!denied, "'no' must deny a Danger command");
}

#[test]
fn danger_uppercase_no_denies() {
    let p = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);

    let denied = show_confirmation_with_input(
        &assessment,
        &default_explanation_for_assessment(&assessment),
        &[],
        true,
        &mut b"NO\n".as_ref(),
        &mut Vec::new(),
    );
    assert!(!denied, "'NO' must deny a Danger command");
}
