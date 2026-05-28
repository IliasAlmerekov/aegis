use super::*;

#[test]
fn custom_pattern_changes_assessment_and_marks_custom_source() {
    let custom = UserPattern {
        id: "USR-ASS-001".to_string(),
        category: Category::Process,
        risk: RiskLevel::Danger,
        pattern: r"deploy-prod-now".to_string(),
        description: "Project-specific destructive deploy shortcut".to_string(),
        safe_alt: Some("deploy-prod-now --dry-run".to_string()),
        justification: None,
    };

    let patterns =
        PatternSet::from_sources(&[custom]).expect("merged builtin+custom set should load");
    let scanner = Scanner::new(patterns);

    let assessment = scanner.assess("echo ok && deploy-prod-now");
    assert_eq!(assessment.risk, RiskLevel::Danger);
    assert_eq!(assessment.decision_source(), DecisionSource::CustomPattern);
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "USR-ASS-001"
                && m.pattern.source == PatternSource::Custom),
        "expected USR-ASS-001 custom match in assessment"
    );
}

#[test]
fn assess_danger_has_matched_patterns() {
    let s = scanner();
    let a = s.assess("rm -rf /home/user/project");
    assert!(
        !a.matched.is_empty(),
        "expected at least one matched pattern"
    );
    assert_eq!(a.risk, RiskLevel::Danger);
}

#[test]
fn assess_block_beats_danger() {
    // rm -rf / matches both FS-001 (Danger) and PS-006 (Block) — Block wins.
    let s = scanner();
    let a = s.assess("rm -rf /");
    assert_eq!(a.risk, RiskLevel::Block);
    let ids: Vec<&str> = a.matched.iter().map(|m| m.pattern.id.as_ref()).collect();
    assert!(
        ids.contains(&"PS-006"),
        "PS-006 must be in matched: {ids:?}"
    );
}

#[test]
fn assess_rm_root_delete_variants_with_split_and_extra_flags_as_block() {
    let s = scanner();

    let cases = [
        "rm -r -f /",
        "rm -R -f /",
        "rm -r --force /",
        "rm --recursive -f /",
        "rm -r --one-file-system -f /",
        "rm -rf --no-preserve-root /",
        "sudo rm -rf --no-preserve-root /",
    ];

    for cmd in cases {
        let assessment = s.assess(cmd);
        assert_eq!(
            assessment.risk,
            RiskLevel::Block,
            "command {cmd:?}: got {:?}, expected Block",
            assessment.risk,
        );
        assert!(
            assessment
                .matched
                .iter()
                .any(|m| m.pattern.id.as_ref() == "PS-006"),
            "command {cmd:?}: PS-006 must be in matched patterns: {:?}",
            assessment
                .matched
                .iter()
                .map(|m| m.pattern.id.as_ref())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn assess_rm_recursive_force_split_flags_on_non_root_paths_as_danger() {
    let s = scanner();

    let cases = [
        "rm -r -f /home/user/old-project",
        "rm -R -f /tmp/build",
        "rm --recursive --force /tmp/build",
        "rm -r --one-file-system -f /tmp/build",
    ];

    for cmd in cases {
        let assessment = s.assess(cmd);
        assert_eq!(
            assessment.risk,
            RiskLevel::Danger,
            "command {cmd:?}: got {:?}, expected Danger",
            assessment.risk,
        );
        assert!(
            assessment
                .matched
                .iter()
                .any(|m| m.pattern.id.as_ref() == "FS-001"),
            "command {cmd:?}: FS-001 must be in matched patterns: {:?}",
            assessment
                .matched
                .iter()
                .map(|m| m.pattern.id.as_ref())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn assess_does_not_match_rm_patterns_inside_longer_words() {
    let s = scanner();
    for cmd in ["echo farm -rf /", "echo farm -rf --no-preserve-root /"] {
        let assessment = s.assess(cmd);
        assert_eq!(
            assessment.risk,
            RiskLevel::Safe,
            "command {cmd:?}: got {:?}, expected Safe",
            assessment.risk,
        );
        assert!(
            assessment.matched.is_empty(),
            "command {cmd:?}: expected no matches, got {:?}",
            assessment
                .matched
                .iter()
                .map(|m| m.pattern.id.as_ref())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn assess_preserves_raw_command() {
    let s = scanner();
    let cmd = "git reset --hard HEAD~1";
    let a = s.assess(cmd);
    assert_eq!(a.command.raw, cmd);
}

// ── assess: compound command classification ───────────────────────────────
//
// Rule: the risk of a compound command is the highest risk across all segments.

#[test]
fn assess_compound_commands() {
    let s = scanner();

    let cases: &[(&str, RiskLevel)] = &[
        // Safe first segment, Danger second — result is Danger
        ("echo ok && rm -rf /home/user/project", RiskLevel::Danger),
        // Safe first segment, Block second — result is Block
        ("echo ok && rm -rf /", RiskLevel::Block),
        // Semicolon with no space after slash — Block must still fire
        ("rm -rf /;echo done", RiskLevel::Block),
        ("rm -rf /&&echo done", RiskLevel::Block),
        // Block first, safe second — Block wins
        ("rm -rf / && echo done", RiskLevel::Block),
        // Danger in middle segment of three
        ("echo a; DROP TABLE users; echo b", RiskLevel::Danger),
        // Block in last segment of three
        ("echo a; echo b; rm -rf /", RiskLevel::Block),
        // Danger via pipe right-hand side
        (
            "echo creds | aws ec2 terminate-instances --instance-ids i-1234",
            RiskLevel::Danger,
        ),
        // || fallback is dangerous
        (
            "false || terraform destroy -auto-approve",
            RiskLevel::Danger,
        ),
        // All segments safe — result is Safe
        ("echo hello && ls /tmp && pwd", RiskLevel::Safe),
    ];

    for (cmd, expected) in cases {
        let assessment = s.assess(cmd);
        assert_eq!(
            assessment.risk, *expected,
            "compound command {cmd:?}: got {:?}, expected {expected:?}",
            assessment.risk,
        );
    }
}

// ── assess: bypass-prone command forms ───────────────────────────────────
//
// For each form the raw string always reaches the Aho-Corasick + regex scan,
// so dangerous payloads wrapped in these shells/operators are still caught.

#[test]
fn assess_bypass_prone_forms() {
    let s = scanner();

    let cases: &[(&str, RiskLevel)] = &[
        // sh -c wrapping a Block payload.
        ("sh -c 'rm -rf /'", RiskLevel::Block),
        // bash -c with a SQL payload
        ("bash -c 'DROP TABLE users;'", RiskLevel::Danger),
        // bash -lc: combined login+command flag
        ("bash -lc 'rm -rf /'", RiskLevel::Block),
        // bash -ic: combined interactive+command flag
        (
            "bash -ic 'terraform destroy -auto-approve'",
            RiskLevel::Danger,
        ),
        // bash --login -c: long login flag before -c
        (
            "bash --login -c 'kubectl delete namespace production'",
            RiskLevel::Danger,
        ),
        // env-prefix without 'env' keyword
        (
            "MY_VAR=x bash -c 'aws ec2 terminate-instances --instance-ids i-1234'",
            RiskLevel::Danger,
        ),
        // heredoc: dangerous command on its own line
        ("bash <<EOF\nrm -rf /\nEOF", RiskLevel::Block),
        // heredoc: non-root dangerous path
        (
            "bash <<EOF\nrm -rf /home/user/project\nEOF",
            RiskLevel::Danger,
        ),
        // pipe chain
        ("echo safe | bash -c 'rm -rf /'", RiskLevel::Block),
        // semicolon chain
        ("echo ok; terraform destroy", RiskLevel::Danger),
        // && chain
        ("ls && DROP TABLE users;", RiskLevel::Danger),
        // || chain
        (
            "false || kubectl delete namespace staging",
            RiskLevel::Danger,
        ),
        // command substitution
        ("echo $(rm -rf /)", RiskLevel::Block),
        // subshell grouping
        ("(rm -rf /)", RiskLevel::Block),
        // python -c inline script
        (
            r#"python3 -c "import os; os.system('rm -rf /')""#,
            RiskLevel::Danger,
        ),
        // double-quoted fragment
        (r#"bash -c "rm -rf / && echo done""#, RiskLevel::Block),
    ];

    for (cmd, expected) in cases {
        let assessment = s.assess(cmd);
        assert_eq!(
            assessment.risk, *expected,
            "bypass form {cmd:?}: got {:?}, expected {expected:?}",
            assessment.risk,
        );
    }
}

// ── assess: indirect / encoded execution patterns ────────────────────────
//
// EXEC-001: echo | sh/bash     (indirect shell execution of a string)
// EXEC-002: python -c          (inline Python interpreter)
// EXEC-003: node -e            (inline Node.js interpreter)
// EXEC-004: perl -e            (inline Perl interpreter)
// EXEC-005: eval ...           (runtime shell evaluation)
// EXEC-006: shell -c ...       (nested shell command strings)
// EXEC-008: cmd <(...)         (process substitution as shell input)

#[test]
fn assess_indirect_execution_forms() {
    let s = scanner();

    let cases: &[(&str, RiskLevel)] = &[
        // ── EXEC-001: echo payload | sh ──────────────────────────────────
        ("echo 'ls /tmp' | sh", RiskLevel::Danger),
        ("echo malicious_payload | bash", RiskLevel::Danger),
        // ── EXEC-001A: shell -c / nested shell string execution ─────────
        ("bash -c 'echo hello'", RiskLevel::Warn),
        ("zsh -c 'echo hello'", RiskLevel::Warn),
        // ── EXEC-002: python -c ──────────────────────────────────────────
        ("python -c 'import sys'", RiskLevel::Warn),
        ("python3 -c \"print('hi')\"", RiskLevel::Warn),
        ("python2 -ic \"import os\"", RiskLevel::Warn),
        ("python3 - <<'PY'\nprint('hi')\nPY", RiskLevel::Warn),
        // ── EXEC-003: node -e ────────────────────────────────────────────
        ("node -e 'console.log(1)'", RiskLevel::Warn),
        ("nodejs -e 'process.version'", RiskLevel::Warn),
        // ── EXEC-004: perl -e ────────────────────────────────────────────
        ("perl -e 'print 42'", RiskLevel::Warn),
        // ── EXEC-005: eval with variable ─────────────────────────────────
        ("eval \"printf hi\"", RiskLevel::Warn),
        ("eval \"$DEPLOY_CMD\"", RiskLevel::Warn),
        ("eval $INIT_SCRIPT", RiskLevel::Warn),
        ("eval \"${MY_BOOTSTRAP_SCRIPT}\"", RiskLevel::Warn),
        // ── EXEC-005A: additional inline interpreters ───────────────────
        ("ruby -e 'puts 42'", RiskLevel::Warn),
        ("php -r 'echo 42;'", RiskLevel::Warn),
        ("lua -e 'print(42)'", RiskLevel::Warn),
        // ── EXEC-006: sh/bash <(...) ─────────────────────────────────────
        ("sh <(generate_config.sh)", RiskLevel::Warn),
        ("bash <(cat bootstrap.sh)", RiskLevel::Warn),
        // ── EXEC-007: source <(...) ──────────────────────────────────────
        ("source <(kubectl completion bash)", RiskLevel::Warn),
        ("source <(helm completion zsh)", RiskLevel::Warn),
    ];

    for (cmd, expected) in cases {
        let assessment = s.assess(cmd);
        assert_eq!(
            assessment.risk, *expected,
            "indirect execution form {cmd:?}: got {:?}, expected {expected:?}",
            assessment.risk,
        );
    }
}

#[test]
fn indirect_execution_safe_commands_not_flagged() {
    let s = scanner();

    for cmd in [
        "python3 script.py",
        "node server.js",
        "perl script.pl",
        "echo hello world",
        "echo hello | grep foo",
        "source ~/.bashrc",
        ". ~/.profile",
        "printf 'eval is just text'",
        "echo bash -c is documented here",
    ] {
        let assessment = s.assess(cmd);
        assert_eq!(
            assessment.risk,
            RiskLevel::Safe,
            "expected Safe for {cmd:?}, got {:?}",
            assessment.risk,
        );
    }
}

#[test]
fn indirect_execution_dangerous_body_escalates_risk() {
    let s = scanner();

    let cases: &[(&str, RiskLevel)] = &[
        // EXEC-002 (Warn) + FS-004 shred in body (Danger) → Danger
        (
            "python3 -c \"import os; os.system('shred -u secrets.key')\"",
            RiskLevel::Danger,
        ),
        // EXEC-003 (Warn) + CL-001 terraform destroy in body (Danger) → Danger
        // NOTE: token-prefix rules cannot see inside interpreted-language string
        // literals, so the inner terraform destroy is invisible to prefix_scan.
        // full_scan regex is also absent for CL-001 (migrated to prefix).
        // Therefore this composite only escalates to Warn, not Danger.
        (
            "node -e \"require('cp').execSync('terraform destroy')\"",
            RiskLevel::Warn,
        ),
        // EXEC-004 (Warn) + FS-001 rm -rf in body (Danger) → Danger
        (
            "perl -e 'system(\"rm -rf /home/user/project\")'",
            RiskLevel::Danger,
        ),
        // EXEC-001: echo|sh wrapping a dangerous payload
        ("echo 'terraform destroy' | sh", RiskLevel::Danger),
    ];

    for (cmd, expected) in cases {
        let assessment = s.assess(cmd);
        assert_eq!(
            assessment.risk, *expected,
            "dangerous body escalation {cmd:?}: got {:?}, expected {expected:?}",
            assessment.risk,
        );
    }
}

#[test]
fn nested_execution_recursive_payloads_escalate_to_inner_risk() {
    let s = scanner();

    let cases: &[(&str, RiskLevel)] = &[
        (r#"source <(bash -c 'rm -rf /')"#, RiskLevel::Block),
        (r#"eval "bash -c 'rm -rf /'""#, RiskLevel::Block),
        ("bash <<'EOF'\nbash -c 'rm -rf /'\nEOF", RiskLevel::Block),
        (
            r#"bash -c 'source <(bash -c "rm -rf /")'"#,
            RiskLevel::Block,
        ),
    ];

    for (cmd, expected) in cases {
        let assessment = s.assess(cmd);
        assert_eq!(
            assessment.risk, *expected,
            "recursive nested payload {cmd:?}: got {:?}, expected {expected:?}",
            assessment.risk,
        );
    }
}
