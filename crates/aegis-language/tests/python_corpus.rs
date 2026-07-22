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
//! The `ExpectedOp` manifest, the `assert_ops` / `assert_clean_no_ops` /
//! `assert_malformed` assertions, and the `bash_exec` payload builder are shared
//! with the JavaScript and TypeScript corpora via
//! `tests/common/corpus_harness.rs` so the three corpora stay in lockstep on
//! assertion semantics. Only `python_exec` (a Python payload builder) is local
//! to this corpus.
//!
//! Out of scope here (deferred, see plan): import / alias / simple-constant →
//! `Partial`-certainty cases (need bounded symbol resolution), `DatabaseDestructive`
//! coverage (the adapter does not emit it yet), stdin / heredoc-to-file /
//! named-file full-pipeline fixtures (need `ScriptFile` fs-read wiring in
//! `aegis::analysis::run`), and the adapter fuzz target.

#[path = "common/corpus_harness.rs"]
mod corpus_harness;

use aegis_language::SourceLanguage;
use aegis_language::languages::python::analyze;
use aegis_language::operation::{OperandCertainty, OperationKind, OperationModifiers};
use corpus_harness::{ExpectedOp, assert_clean_no_ops, assert_malformed, assert_ops, bash_exec};

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
    assert_ops(analyze, FS_DELETE, &expected);
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
    assert_ops(analyze, FS_OVERWRITE, &expected);
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
    assert_ops(analyze, PERMS, &expected);
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
    assert_ops(analyze, EXEC_SHELL, &expected);
}

#[test]
fn exec_python_emits_two_code_executions_with_python_payloads() {
    let expected = [
        python_exec("__import__('os').remove('x')"),
        python_exec("shutil.rmtree('/tmp/x')"),
    ];
    assert_ops(analyze, EXEC_PYTHON, &expected);
}

#[test]
fn negatives_emit_no_operations() {
    assert_clean_no_ops(analyze, NEGATIVES);
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
    assert_ops(analyze, DYNAMIC_OPERAND, &expected);
}

#[test]
fn modern_syntax_parses_cleanly_with_no_false_operations() {
    assert_clean_no_ops(analyze, MODERN_SYNTAX);
}

#[test]
fn malformed_source_records_parse_errors() {
    assert_malformed(analyze, MALFORMED);
}

/// Build a `CodeExecution` expectation carrying a Python literal payload.
/// Python-specific, so this builder stays local to this corpus rather than in
/// `tests/common/corpus_harness.rs`.
fn python_exec(payload: &'static str) -> ExpectedOp {
    ExpectedOp {
        kind: OperationKind::CodeExecution,
        modifiers: OperationModifiers::default(),
        certainty: OperandCertainty::Known,
        payload: Some((SourceLanguage::Python, payload)),
    }
}
