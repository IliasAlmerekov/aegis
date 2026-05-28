use super::*;

#[test]
fn render_policy_block_mentions_reason() {
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![]);
    let explanation = make_explanation(
        &assessment,
        PolicyRationale::StrictPolicy,
        Some(BlockReason::StrictPolicy),
        None,
    );
    let mut output = Vec::new();

    render_policy_block(&assessment, &explanation, &mut output);

    let text = strip_ansi(&String::from_utf8_lossy(&output));
    assert!(
        text.contains("AEGIS POLICY BLOCKED THIS COMMAND"),
        "policy block output must contain the headline; got:\n{text}"
    );
    assert!(
        text.contains(
            "Reason: blocked by strict mode (non-safe commands require an allowlist override)"
        ),
        "policy block output must contain the reason; got:\n{text}"
    );
}

#[test]
fn confirmation_renders_policy_reason_from_explanation() {
    let matched = make_match(
        "FS-001",
        RiskLevel::Danger,
        r"rm\s+",
        "Recursive delete",
        None,
    );
    let assessment = make_assessment("rm -rf /tmp/demo", RiskLevel::Danger, vec![matched]);
    let explanation = make_explanation(
        &assessment,
        PolicyRationale::RequiresConfirmation,
        None,
        Some(AllowlistExplanation {
            pattern: "rm -rf /tmp/*".to_string(),
            reason: "temporary workspace cleanup".to_string(),
            source_layer: ConfigSourceLayer::Project,
        }),
    );

    let mut output = Vec::new();
    show_confirmation_with_input(
        &assessment,
        &explanation,
        &[],
        true,
        &mut b"no\n".as_ref(),
        &mut output,
    );

    let text = strip_ansi(&String::from_utf8_lossy(&output));
    assert!(
        text.contains("Reason: requires confirmation despite matching allowlist rule: temporary workspace cleanup"),
        "confirmation output must use the canonical explanation reason; got:\n{text}"
    );
}

#[test]
fn policy_block_renders_from_canonical_block_reason() {
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![]);
    let explanation = make_explanation(
        &assessment,
        PolicyRationale::StrictPolicy,
        Some(BlockReason::StrictPolicy),
        None,
    );
    let mut output = Vec::new();

    render_policy_block(&assessment, &explanation, &mut output);

    let text = strip_ansi(&String::from_utf8_lossy(&output));
    assert!(
        text.contains(
            "Reason: blocked by strict mode (non-safe commands require an allowlist override)"
        ),
        "policy block output must use the canonical block reason; got:\n{text}"
    );
}

#[test]
fn policy_block_renders_ci_policy_reason_from_explanation() {
    let assessment = make_assessment(
        "terraform destroy -target=module.prod.api",
        RiskLevel::Danger,
        vec![],
    );
    let explanation = make_explanation(
        &assessment,
        PolicyRationale::ProtectCiPolicy,
        Some(BlockReason::ProtectCiPolicy),
        None,
    );
    let mut output = Vec::new();

    render_policy_block(&assessment, &explanation, &mut output);

    let text = strip_ansi(&String::from_utf8_lossy(&output));
    assert!(
        text.contains("Reason: blocked by CI policy (Protect mode + ci_policy=Block)"),
        "policy block output must use the CI policy reason from explanation; got:\n{text}"
    );
}

#[test]
fn ui_rendering_does_not_need_to_synthesize_missing_optional_sections() {
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![]);
    let explanation = make_explanation(
        &assessment,
        PolicyRationale::RequiresConfirmation,
        None,
        None,
    );
    let mut output = Vec::new();

    let denied = show_confirmation_with_input(
        &assessment,
        &explanation,
        &[],
        true,
        &mut b"no\n".as_ref(),
        &mut output,
    );

    assert!(!denied);

    let rendered = strip_ansi(&String::from_utf8(output).expect("ui output should be utf8"));
    assert!(
        rendered.contains("Execute suspicious command? [y/N/a/d]:"),
        "test must exercise the interactive confirmation dialog path; got:\n{rendered}"
    );
    assert!(
        rendered.contains("Reason: requires confirmation"),
        "ui should render the canonical concise policy reason; got:\n{rendered}"
    );
    assert!(
        !rendered.contains("requires explicit confirmation"),
        "ui should not synthesize an alternative reason label when optional sections are absent; got:\n{rendered}"
    );
    assert!(
        !rendered.contains("Snapshots created:"),
        "ui should not synthesize missing runtime outcome sections; got:\n{rendered}"
    );
    assert!(
        !rendered.contains("allowlist rule"),
        "ui should not synthesize a missing allowlist section; got:\n{rendered}"
    );
    assert!(
        !rendered.contains("outcome"),
        "ui should not synthesize a missing runtime outcome section; got:\n{rendered}"
    );
}
