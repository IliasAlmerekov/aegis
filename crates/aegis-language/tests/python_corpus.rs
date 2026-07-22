//! Python adapter corpus (plan Iteration 6, ADR-022 §11).
//!
//! Checked-in `.py` corpus files under `tests/corpora/python/`, embedded at
//! compile time via `include_str!`. Each file is paired with a hand-derived
//! expectation — derived from ADR-022 §3/§7 and Python API semantics, NOT
//! recomputed by the adapter — declaring the operations the adapter must
//! surface, their modifiers and [`OperandCertainty`], the parse-error count,
//! and any nested execution-sink payload. The harness runs the public
//! [`aegis_language::languages::python::analyze`] seam over every corpus file
//! and asserts the adapter output matches the expectation.
//!
//! This is a characterization + regression corpus: the expected values come
//! from an independent source of truth, so a real adapter or grammar regression
//! fails the test. `modern_syntax` additionally proves the pinned
//! tree-sitter-python 0.25.0 grammar parses current Python without errors and
//! without false operations; `malformed` proves a nonzero parse-error count is
//! reported (the root mapping turns that into `IncompleteSyntax` degradation).
//!
//! Out of scope here (deferred, see plan): import / alias / simple-constant →
//! `Partial`-certainty cases (need bounded symbol resolution), `DatabaseDestructive`
//! coverage (the adapter does not emit it yet), stdin / heredoc-to-file /
//! named-file full-pipeline fixtures (need `ScriptFile` fs-read wiring in
//! `aegis::analysis::run`), and the adapter fuzz target.

use aegis_language::SourceLanguage;
use aegis_language::languages::python::analyze;
use aegis_language::operation::{OperandCertainty, OperationKind, OperationModifiers};

/// One expected [`aegis_language::operation::DetectedOperation`]. Spans are
/// deliberately not pinned — they are an implementation detail that changes on
/// refactor; the unit tests pin span coverage. The spec-level invariants are
/// kind, modifiers, certainty, and (for execution sinks) the nested payload.
#[derive(Debug, Clone, Copy)]
struct ExpectedOp {
    kind: OperationKind,
    modifiers: OperationModifiers,
    certainty: OperandCertainty,
    /// For a `CodeExecution` sink: the nested payload's language and recovered
    /// literal source. `None` for non-execution ops and for dynamic payloads.
    payload: Option<(SourceLanguage, &'static str)>,
}

fn assert_ops(source: &str, expected: &[ExpectedOp]) {
    let result = analyze(source);
    assert_eq!(
        result.parse_errors, 0,
        "corpus source must parse cleanly:\n{source}",
    );
    assert_eq!(
        result.operations.len(),
        expected.len(),
        "operation count mismatch for source:\n{source}\ngot {:?}",
        result.operations,
    );
    for (i, (op, exp)) in result.operations.iter().zip(expected.iter()).enumerate() {
        assert_eq!(op.kind, exp.kind, "op[{i}] kind for source:\n{source}");
        assert_eq!(
            op.modifiers, exp.modifiers,
            "op[{i}] modifiers for source:\n{source}"
        );
        assert_eq!(
            op.certainty, exp.certainty,
            "op[{i}] certainty for source:\n{source}"
        );
        match (op.payload.as_ref(), exp.payload) {
            (None, None) => {}
            (Some(p), Some((lang, src))) => {
                assert_eq!(
                    p.language, lang,
                    "op[{i}] payload language for source:\n{source}"
                );
                assert_eq!(
                    p.source, src,
                    "op[{i}] payload source for source:\n{source}"
                );
            }
            (Some(p), None) => panic!("op[{i}] unexpected payload {p:?} for source:\n{source}"),
            (None, Some(_)) => panic!("op[{i}] missing expected payload for source:\n{source}"),
        }
    }
}

/// Assert a corpus source parses cleanly and yields no operations.
fn assert_clean_no_ops(source: &str) {
    let result = analyze(source);
    assert_eq!(result.parse_errors, 0, "expected clean parse:\n{source}");
    assert!(
        result.operations.is_empty(),
        "expected no operations for source:\n{source}\ngot {:?}",
        result.operations,
    );
}

/// Assert a corpus source is reported as malformed (nonzero parse errors).
fn assert_malformed(source: &str) {
    let result = analyze(source);
    assert!(
        result.parse_errors > 0,
        "expected nonzero parse errors for source:\n{source}",
    );
}

const FS_DELETE: &str = include_str!("corpora/python/fs_delete.py");
const FS_OVERWRITE: &str = include_str!("corpora/python/fs_overwrite.py");
const PERMS: &str = include_str!("corpora/python/perms.py");
const EXEC_SHELL: &str = include_str!("corpora/python/exec_shell.py");
const EXEC_PYTHON: &str = include_str!("corpora/python/exec_python.py");
const NEGATIVES: &str = include_str!("corpora/python/negatives.py");
const DYNAMIC_OPERAND: &str = include_str!("corpora/python/dynamic_operand.py");
const MODERN_SYNTAX: &str = include_str!("corpora/python/modern_syntax.py");
const MALFORMED: &str = include_str!("corpora/python/malformed.py");

#[test]
fn fs_delete_emits_four_deletes_with_rmtree_recursive() {
    let expected = [
        ExpectedOp {
            kind: OperationKind::FilesystemDelete,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Known,
            payload: None,
        },
        ExpectedOp {
            kind: OperationKind::FilesystemDelete,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Known,
            payload: None,
        },
        ExpectedOp {
            kind: OperationKind::FilesystemDelete,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Known,
            payload: None,
        },
        ExpectedOp {
            kind: OperationKind::FilesystemDelete,
            modifiers: OperationModifiers {
                recursive: true,
                ..OperationModifiers::default()
            },
            certainty: OperandCertainty::Known,
            payload: None,
        },
    ];
    assert_ops(FS_DELETE, &expected);
}

#[test]
fn fs_overwrite_emits_w_and_a_only() {
    let expected = [
        ExpectedOp {
            kind: OperationKind::FilesystemOverwrite,
            modifiers: OperationModifiers {
                destructive_mode: true,
                ..OperationModifiers::default()
            },
            certainty: OperandCertainty::Known,
            payload: None,
        },
        ExpectedOp {
            kind: OperationKind::FilesystemOverwrite,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Known,
            payload: None,
        },
    ];
    assert_ops(FS_OVERWRITE, &expected);
}

#[test]
fn perms_emits_three_permission_or_ownership_changes() {
    let expected = [
        ExpectedOp {
            kind: OperationKind::PermissionOrOwnershipChange,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Known,
            payload: None,
        },
        ExpectedOp {
            kind: OperationKind::PermissionOrOwnershipChange,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Known,
            payload: None,
        },
        ExpectedOp {
            kind: OperationKind::PermissionOrOwnershipChange,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Known,
            payload: None,
        },
    ];
    assert_ops(PERMS, &expected);
}

#[test]
fn exec_shell_emits_six_code_executions_with_bash_payloads() {
    let expected = [
        bash_exec("rm -rf /tmp/x"),
        bash_exec("rm /tmp/y"),
        bash_exec("rm /tmp/z"),
        bash_exec("rm /tmp/w"),
        bash_exec("rm /tmp/v"),
        bash_exec("rm /tmp/u"),
    ];
    assert_ops(EXEC_SHELL, &expected);
}

#[test]
fn exec_python_emits_two_code_executions_with_python_payloads() {
    let expected = [
        python_exec("__import__('os').remove('x')"),
        python_exec("shutil.rmtree('/tmp/x')"),
    ];
    assert_ops(EXEC_PYTHON, &expected);
}

#[test]
fn negatives_emit_no_operations() {
    assert_clean_no_ops(NEGATIVES);
}

#[test]
fn dynamic_operand_emits_ops_with_dynamic_certainty_and_no_payload() {
    // ADR-022 §3/§7: a dynamic operand never lowers risk and never hides the
    // operation, but a dynamic payload is never enqueued or evaluated. Bounded
    // symbol resolution is deferred, so a variable holding a literal is still
    // Dynamic at this seam.
    let expected = [
        ExpectedOp {
            kind: OperationKind::FilesystemDelete,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Dynamic,
            payload: None,
        },
        ExpectedOp {
            kind: OperationKind::CodeExecution,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Dynamic,
            payload: None,
        },
        ExpectedOp {
            kind: OperationKind::CodeExecution,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Dynamic,
            payload: None,
        },
    ];
    assert_ops(DYNAMIC_OPERAND, &expected);
}

#[test]
fn modern_syntax_parses_cleanly_with_no_false_operations() {
    assert_clean_no_ops(MODERN_SYNTAX);
}

#[test]
fn malformed_source_records_parse_errors() {
    assert_malformed(MALFORMED);
}

/// Build a `CodeExecution` expectation carrying a Bash (shell) literal payload.
fn bash_exec(payload: &'static str) -> ExpectedOp {
    ExpectedOp {
        kind: OperationKind::CodeExecution,
        modifiers: OperationModifiers::default(),
        certainty: OperandCertainty::Known,
        payload: Some((SourceLanguage::Bash, payload)),
    }
}

/// Build a `CodeExecution` expectation carrying a Python literal payload.
fn python_exec(payload: &'static str) -> ExpectedOp {
    ExpectedOp {
        kind: OperationKind::CodeExecution,
        modifiers: OperationModifiers::default(),
        certainty: OperandCertainty::Known,
        payload: Some((SourceLanguage::Python, payload)),
    }
}
