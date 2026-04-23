use aegis::interceptor::{RiskLevel, assess};

#[test]
fn regression_commands_with_heredoc_hermetically_block_or_warn() {
    let command = "bash <<'EOF'\necho 'prepare'\nrm -rf /tmp/aegis-fuzz-shell-regression\nEOF\n";

    let assessment = assess(command).expect("assessment should not fail");
    assert!(
        assessment.risk >= RiskLevel::Warn,
        "expected heredoc payload with rm to be risky: {command:?}"
    );
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "FS-001" || m.pattern.id.as_ref() == "SCAN-001"),
        "expected heredoc regression command to match a scanner pattern"
    );
}

#[test]
fn regression_commands_with_inline_python_script_are_flagged() {
    let command = r#"python3 -c "import os; os.system('rm -rf /tmp/aegis-inline-regression')""#;

    let assessment = assess(command).expect("assessment should not fail");
    assert!(
        assessment.risk >= RiskLevel::Warn,
        "expected inline script payload to be risky: {command:?}"
    );
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "PKG-001"
                || m.pattern.id.as_ref() == "FS-001"
                || m.pattern.id.as_ref() == "PS-010"),
        "expected inline script regression command to match inline-script or scanner pattern"
    );
}

#[test]
fn regression_commands_with_pipes_and_chains_are_covered() {
    let command = "cat /etc/passwd | bash -lc \"echo ok; rm -rf /tmp/aegis-pipe-regression\"";

    let assessment = assess(command).expect("assessment should not fail");
    assert!(
        assessment.risk >= RiskLevel::Warn,
        "expected piped chain regression command to be risky: {command:?}"
    );
    assert!(
        assessment.matched.iter().any(|m| !m.pattern.id.is_empty()),
        "expected piped chain command to produce at least one match"
    );
}

#[test]
fn regression_commands_with_quotes_and_escape_sequences_are_scanned() {
    let command = "echo \"safe token: rm -rf /tmp/aegis-quoted-regression\" && true";

    let assessment = assess(command).expect("assessment should not fail");
    assert!(
        assessment.risk >= RiskLevel::Warn,
        "expected quoted escape content to remain risky: {command:?}"
    );
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "FS-001"),
        "expected quoted regression command to match rm command pattern"
    );
}

#[test]
fn regression_commands_with_subshell_grouping_and_multiline_payloads() {
    let command =
        "(\n    echo start\n    rm -rf /tmp/aegis-subshell-regression\n) && { echo done; }\n";

    let assessment = assess(command).expect("assessment should not fail");
    assert!(
        assessment.risk >= RiskLevel::Warn,
        "expected subshell and multiline regression command to be risky: {command:?}"
    );
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "FS-001"),
        "expected subshell regression command to match scanner pattern"
    );
}

#[test]
fn regression_docker_compose_down_with_volume_flag_is_flagged() {
    let command = "docker compose down -v";

    let assessment = assess(command).expect("assessment should not fail");
    assert_eq!(
        assessment.risk,
        RiskLevel::Warn,
        "expected docker compose down -v to be warn: {command:?}"
    );
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "DK-003"),
        "expected docker compose down -v to match DK-003"
    );
}
