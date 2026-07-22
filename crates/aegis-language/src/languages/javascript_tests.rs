//! Unit tests for the JavaScript adapter (`javascript.rs`).
//!
//! Lives in a sibling file via `#[path = "javascript_tests.rs"]` to keep the
//! adapter source under the workspace 800-line file-size budget
//! (`tests/file_size_budget.rs`); the same `#[path]` split is used by
//! `operation.rs` → `operation_tests.rs`. `use super::*` resolves to the
//! `javascript` module, so the tests reach the adapter's items directly.

use super::*;
use crate::operation::{DetectedOperation, OperandCertainty, OperationKind, OperationModifiers};

/// Assert `source` yields exactly one operation and return it.
fn one_op(source: &str) -> DetectedOperation {
    let result = analyze(source);
    assert_eq!(
        result.parse_errors, 0,
        "source must parse cleanly: {source:?}"
    );
    assert_eq!(
        result.operations.len(),
        1,
        "expected exactly one operation for {source:?}, got {:?}",
        result.operations
    );
    result.operations.into_iter().next().unwrap()
}

/// Assert `source` yields no operations (and parses cleanly).
fn no_ops(source: &str) {
    let result = analyze(source);
    assert_eq!(
        result.parse_errors, 0,
        "source must parse cleanly: {source:?}"
    );
    assert!(
        result.operations.is_empty(),
        "expected no operations for {source:?}, got {:?}",
        result.operations
    );
}

// --- Filesystem deletion ------------------------------------------------

#[test]
fn fs_unlinksync_literal_path_yields_filesystem_delete_known() {
    let op = one_op("fs.unlinkSync(\"data.txt\")");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert_eq!(op.certainty, OperandCertainty::Known);
    assert_eq!(op.modifiers, OperationModifiers::default());
    assert!(op.payload.is_none());
}

#[test]
fn fs_rmdirsync_is_filesystem_delete() {
    let op = one_op("fs.rmdirSync(\"d\")");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert_eq!(op.modifiers, OperationModifiers::default());
}

#[test]
fn fs_rmsync_recursive_option_sets_recursive_modifier() {
    let op = one_op("fs.rmSync(\"d\", {recursive: true})");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert!(op.modifiers.recursive, "recursive: true must set recursive");
    assert!(!op.modifiers.forced);
}

#[test]
fn fs_rmsync_string_key_recursive_option_sets_recursive_modifier() {
    // A string-keyed `{"recursive": true}` pair is the same literal shape as the
    // identifier-keyed form and must also set the recursive modifier.
    let op = one_op("fs.rmSync(\"d\", {\"recursive\": true})");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert!(
        op.modifiers.recursive,
        "string-keyed recursive: true must set recursive"
    );
}

#[test]
fn fs_rmsync_without_recursive_option_does_not_set_recursive() {
    let op = one_op("fs.rmSync(\"d\")");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert!(!op.modifiers.recursive);
}

#[test]
fn fs_unlinksync_with_variable_path_is_dynamic() {
    let op = one_op("fs.unlinkSync(path)");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none());
}

#[test]
fn fs_unlinksync_with_template_literal_is_known() {
    let op = one_op("fs.unlinkSync(`data.txt`)");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert_eq!(op.certainty, OperandCertainty::Known);
}

#[test]
fn fs_unlinksync_with_template_interpolation_is_dynamic() {
    let op = one_op("fs.unlinkSync(`${name}`)");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert_eq!(
        op.certainty,
        OperandCertainty::Dynamic,
        "a template with interpolation is not a known literal"
    );
}

// --- Filesystem overwrite ------------------------------------------------

#[test]
fn fs_writesync_is_destructive_overwrite() {
    let op = one_op("fs.writeFileSync(\"f\", \"x\")");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(
        op.modifiers.destructive_mode,
        "writeFileSync truncates and must set destructive_mode"
    );
    assert_eq!(op.certainty, OperandCertainty::Known, "literal path");
}

#[test]
fn fs_appendsync_is_overwrite_without_destructive_mode() {
    let op = one_op("fs.appendFileSync(\"f\", \"x\")");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(
        !op.modifiers.destructive_mode,
        "appendFileSync appends without truncating"
    );
}

// --- Permission / ownership ---------------------------------------------

#[test]
fn fs_chmodsync_and_chownsync_are_permission_or_ownership() {
    for src in ["fs.chmodSync(\"f\", 0o000)", "fs.chownSync(\"f\", 0, 0)"] {
        let op = one_op(src);
        assert_eq!(op.kind, OperationKind::PermissionOrOwnershipChange, "{src}");
    }
}

// --- Execution sinks: eval / Function (JavaScript payload) --------------

#[test]
fn eval_literal_emits_code_execution_with_javascript_payload() {
    let op = one_op("eval(\"fs.unlinkSync('x')\")");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Known);
    let payload = op.payload.expect("literal payload must be recovered");
    assert_eq!(payload.language, SourceLanguage::JavaScript);
    assert_eq!(payload.source, "fs.unlinkSync('x')");
}

#[test]
fn new_function_literal_emits_code_execution_with_javascript_payload() {
    // The Function constructor's final string argument is the body.
    let op = one_op("new Function(\"return fs.unlinkSync('x')\")");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Known);
    let payload = op.payload.expect("literal payload must be recovered");
    assert_eq!(payload.language, SourceLanguage::JavaScript);
    assert_eq!(payload.source, "return fs.unlinkSync('x')");
}

#[test]
fn new_function_multiarg_uses_last_string_as_body() {
    let op = one_op("new Function(\"a\", \"return 1\")");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Known);
    assert_eq!(op.payload.as_ref().unwrap().source, "return 1");
}

#[test]
fn eval_dynamic_payload_is_code_execution_without_nested_target() {
    let op = one_op("eval(userInput)");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none(), "a dynamic payload is never evaluated");
}

#[test]
fn new_function_dynamic_body_is_dynamic_without_payload() {
    let op = one_op("new Function(\"a\", body)");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none());
}

// --- Execution sinks: child_process (Bash payload / argv) ---------------

#[test]
fn child_process_exec_literal_emits_code_execution_with_bash_payload() {
    let op = one_op("child_process.exec(\"rm -rf /tmp/x\")");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Known);
    let payload = op.payload.expect("literal shell payload recovered");
    assert_eq!(
        payload.language,
        SourceLanguage::Bash,
        "cross-language nesting"
    );
    assert_eq!(payload.source, "rm -rf /tmp/x");
}

#[test]
fn child_process_execsync_literal_emits_bash_payload() {
    let op = one_op("child_process.execSync(\"rm x\")");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.payload.as_ref().unwrap().language, SourceLanguage::Bash);
    assert_eq!(op.payload.as_ref().unwrap().source, "rm x");
}

#[test]
fn child_process_exec_dynamic_payload_is_dynamic_without_nested_target() {
    let op = one_op("child_process.exec(cmd)");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none());
}

#[test]
fn child_process_spawn_argv_form_is_dynamic_without_payload() {
    // spawn takes a program name + argv, not shell source; the visible sink
    // still fires as CodeExecution but no nested target is recovered.
    let op = one_op("child_process.spawn(\"rm\", [\"-rf\", \"x\"])");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none());
}

#[test]
fn child_process_spawn_execfile_fork_are_code_execution_argv() {
    for src in [
        "child_process.spawnSync(\"rm\", [\"x\"])",
        "child_process.execFile(\"rm\", [\"x\"])",
        "child_process.execFileSync(\"rm\", [\"x\"])",
        "child_process.fork(\"./worker.js\")",
    ] {
        let op = one_op(src);
        assert_eq!(op.kind, OperationKind::CodeExecution, "{src}");
        assert!(op.payload.is_none(), "{src}: argv form has no payload");
        assert_eq!(op.certainty, OperandCertainty::Dynamic, "{src}");
    }
}

// --- Execution-sink payloads: template literals & escape sequences -------
// `string_literal_content`'s `template_string`/`escape_sequence` arms are only
// reached from the Exec path, so exercised here, not via the FS tests above.

#[test]
fn eval_template_literal_without_interpolation_emits_javascript_payload() {
    // Template with no interpolation → known payload, raw text between
    // backticks (exercises `string_literal_content`'s `template_string` arm).
    let op = one_op("eval(`fs.unlinkSync(\"x\")`)");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Known);
    let payload = op.payload.expect("literal payload must be recovered");
    assert_eq!(payload.language, SourceLanguage::JavaScript);
    assert_eq!(payload.source, "fs.unlinkSync(\"x\")");
}

#[test]
fn child_process_exec_template_with_interpolation_is_dynamic() {
    // `${}` template → Dynamic, no payload (`template_substitution => None`).
    let op = one_op("child_process.exec(`rm -rf ${dir}`)");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none());
}

#[test]
fn eval_payload_with_escape_sequence_recovers_raw_text() {
    // Escapes as-written; decoding deferred (Python Slice 1). Exercises `escape_sequence`.
    let op = one_op("eval(\"a\\tb\")");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Known);
    let payload = op.payload.expect("literal payload must be recovered");
    assert_eq!(payload.language, SourceLanguage::JavaScript);
    assert_eq!(payload.source, "a\\tb");
}

#[test]
fn eval_template_literal_with_escape_sequence_recovers_raw_text() {
    // A template literal containing an escape sequence exercises the
    // `escape_sequence` sub-arm of the `template_string` arm; the raw text
    // between the backticks is recovered with escapes as-written.
    let op = one_op("eval(`a\\tb`)");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Known);
    let payload = op.payload.expect("literal payload must be recovered");
    assert_eq!(payload.language, SourceLanguage::JavaScript);
    assert_eq!(payload.source, "a\\tb");
}

// --- Negatives ----------------------------------------------------------

#[test]
fn comment_mentioning_fs_unlinksync_is_not_an_operation() {
    no_ops("// fs.unlinkSync(\"x\")");
}

#[test]
fn string_literal_mentioning_fs_unlinksync_is_not_an_operation() {
    no_ops("\"fs.unlinkSync('x')\"");
}

#[test]
fn member_reference_without_call_is_not_an_operation() {
    no_ops("f = fs.unlinkSync");
}

#[test]
fn unrelated_call_is_not_an_operation() {
    no_ops("console.log(\"hello\")");
}

// --- Composition / ordering / spans -------------------------------------

#[test]
fn multiple_operations_are_emitted_in_source_order() {
    let result = analyze("fs.unlinkSync(\"a\")\nchild_process.exec(\"rm b\")\n");
    assert_eq!(result.operations.len(), 2);
    assert_eq!(result.operations[0].kind, OperationKind::FilesystemDelete);
    assert_eq!(result.operations[1].kind, OperationKind::CodeExecution);
    assert!(
        result.operations[0].span.byte_start < result.operations[1].span.byte_start,
        "operations must be in document order"
    );
}

#[test]
fn nested_calls_detect_both_inner_and_outer() {
    // child_process.exec(eval("x")): the outer sink takes a non-literal
    // (the eval call), so it is Dynamic; the inner eval has a literal
    // payload.
    let result = analyze("child_process.exec(eval(\"x\"))");
    assert_eq!(result.operations.len(), 2);
    let kinds: Vec<_> = result.operations.iter().map(|o| o.kind).collect();
    assert_eq!(
        kinds,
        vec![OperationKind::CodeExecution, OperationKind::CodeExecution]
    );
    assert!(
        result.operations.iter().any(|o| o
            .payload
            .as_ref()
            .is_some_and(|p| p.language == SourceLanguage::JavaScript && p.source == "x")),
        "the inner eval payload must be recovered"
    );
    assert!(
        result
            .operations
            .iter()
            .any(|o| o.certainty == OperandCertainty::Dynamic),
        "the outer child_process.exec must be dynamic"
    );
}

#[test]
fn operation_span_covers_the_call() {
    let src = "fs.unlinkSync(\"data.txt\")";
    let op = one_op(src);
    assert_eq!(op.span.byte_start, 0);
    assert_eq!(op.span.byte_end, src.len());
    assert_eq!(op.span.line, 1);
    assert_eq!(op.span.column, 1);
}

// --- Malformed source ---------------------------------------------------

#[test]
fn malformed_source_records_parse_errors() {
    let result = analyze("fs.unlinkSync(");
    assert!(
        result.parse_errors > 0,
        "incomplete syntax must record a nonzero parse error count"
    );
}

#[test]
fn empty_source_records_no_operations_and_no_errors() {
    let result = analyze("");
    assert_eq!(result.parse_errors, 0);
    assert!(result.operations.is_empty());
}
