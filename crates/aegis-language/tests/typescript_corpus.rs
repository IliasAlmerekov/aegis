//! TypeScript adapter corpus (plan Iteration 7, ADR-022 §11).
//!
//! Checked-in `.ts` corpus files under `tests/corpora/typescript/`, embedded at
//! compile time via `include_str!`. Each file is paired with a hand-derived
//! expectation — derived from ADR-022 §3/§7 and the shared JavaScript-family
//! API semantics, NOT recomputed by the adapter — declaring the operations the
//! adapter must surface, their modifiers and [`OperandCertainty`], the
//! parse-error count, and any nested execution-sink payload. The harness runs
//! the public [`aegis_language::languages::typescript::analyze`] seam over every
//! corpus file and asserts the adapter output matches the expectation.
//!
//! This is a characterization + regression corpus: the expected values come
//! from an independent source of truth, so a real adapter or grammar regression
//! fails the test. `modern_syntax` additionally proves the pinned
//! tree-sitter-typescript 0.23.2 grammar parses current TypeScript (generics,
//! arrow generics, `satisfies`, decorators, `import type`, mapped / conditional
//! / `infer` types) without errors and without false operations; `malformed`
//! proves a nonzero parse-error count is reported (the root mapping turns that
//! into `IncompleteSyntax` degradation). The operation-category files enrich
//! the JavaScript corpus with TypeScript-only call syntax — explicit type
//! arguments on tracked calls (`fs.unlinkSync<void>(...)`,
//! `child_process.exec<void>(...)`, `new Function<string>(...)`) and an `as`-
//! cast dynamic operand — proving those calls still capture and classify at
//! corpus scale.
//!
//! The `ExpectedOp` manifest, the `assert_ops` / `assert_clean_no_ops` /
//! `assert_malformed` assertions, and the `bash_exec` payload builder are shared
//! with the JavaScript and Python corpora via `tests/common/corpus_harness.rs`
//! so the three corpora stay in lockstep on assertion semantics. Only
//! `js_exec` (a JavaScript-family payload builder) is local to this corpus.
//!
//! Out of scope here (deferred, see plan): `fs.promises.*` and callback-form
//! variants, import / alias / simple-constant → `OperandCertainty::Partial`
//! cases (need bounded symbol resolution), `DatabaseDestructive` coverage (the
//! adapter does not emit it yet), chained member calls (`a.b.c()` — the
//! `calls.scm` query matches `object: (identifier)` only), a TypeScript inline
//! runner in the router registry (no `node -e` analog routes to TypeScript
//! today, so there is no real-subprocess orchestration corpus here — deferred
//! with the TypeScript runner-routing slice), and the adapter fuzz target.

#[path = "common/corpus_harness.rs"]
mod corpus_harness;

use aegis_language::SourceLanguage;
use aegis_language::languages::typescript::analyze;
use aegis_language::operation::{OperandCertainty, OperationKind, OperationModifiers};
use corpus_harness::{ExpectedOp, assert_clean_no_ops, assert_malformed, assert_ops, bash_exec};

const FS_DELETE: &str = include_str!("corpora/typescript/fs_delete.ts");
const FS_OVERWRITE: &str = include_str!("corpora/typescript/fs_overwrite.ts");
const PERMS: &str = include_str!("corpora/typescript/perms.ts");
const EXEC_SHELL: &str = include_str!("corpora/typescript/exec_shell.ts");
const EXEC_JS: &str = include_str!("corpora/typescript/exec_js.ts");
const NEGATIVES: &str = include_str!("corpora/typescript/negatives.ts");
const DYNAMIC_OPERAND: &str = include_str!("corpora/typescript/dynamic_operand.ts");
const MODERN_SYNTAX: &str = include_str!("corpora/typescript/modern_syntax.ts");
const MALFORMED: &str = include_str!("corpora/typescript/malformed.ts");

#[test]
fn fs_delete_emits_four_deletes_with_rmsync_recursive() {
    // The first call carries an explicit type argument (`fs.unlinkSync<void>`);
    // it still surfaces as a `FilesystemDelete` with `Known` certainty.
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
            modifiers: OperationModifiers {
                recursive: true,
                ..OperationModifiers::default()
            },
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
fn fs_overwrite_emits_write_destructive_and_append() {
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
fn perms_emits_two_permission_or_ownership_changes() {
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
    ];
    assert_ops(analyze, PERMS, &expected);
}

#[test]
fn exec_shell_emits_three_code_executions_with_bash_payloads() {
    // The third call carries an explicit type argument
    // (`child_process.exec<void>`); its Bash payload is still recovered.
    let expected = [
        bash_exec("rm -rf /tmp/x"),
        bash_exec("rm /tmp/y"),
        bash_exec("rm -rf /tmp/z"),
    ];
    assert_ops(analyze, EXEC_SHELL, &expected);
}

#[test]
fn exec_js_emits_two_code_executions_with_javascript_payloads() {
    // The shared `family` module tags `eval`/`Function` payloads as
    // `JavaScript` (they evaluate JS regardless of the enclosing file's
    // language). The `new Function<string>(...)` type argument does not change
    // the recovered JavaScript body.
    let expected = [
        js_exec("fs.unlinkSync('x')"),
        js_exec("return fs.unlinkSync('y')"),
    ];
    assert_ops(analyze, EXEC_JS, &expected);
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
    // Dynamic at this seam. A template literal with interpolation is computed,
    // so it is Dynamic too. The final delete's `as`-cast operand is not a
    // string literal, so it is Dynamic as well.
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
        ExpectedOp {
            kind: OperationKind::FilesystemDelete,
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

/// Build a `CodeExecution` expectation carrying a JavaScript literal payload.
/// JavaScript-family specific (the shared `family` module tags `eval`/`Function`
/// payloads as `JavaScript`), so this builder stays local to the JS-family
/// corpora rather than in `tests/common/corpus_harness.rs`.
fn js_exec(payload: &'static str) -> ExpectedOp {
    ExpectedOp {
        kind: OperationKind::CodeExecution,
        modifiers: OperationModifiers::default(),
        certainty: OperandCertainty::Known,
        payload: Some((SourceLanguage::JavaScript, payload)),
    }
}
