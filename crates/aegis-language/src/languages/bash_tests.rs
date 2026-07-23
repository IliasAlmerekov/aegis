//! Unit tests for the Bash adapter (`bash.rs`).
//!
//! Lives in a sibling file via `#[path = "bash_tests.rs"]` to keep the adapter
//! source under the workspace 800-line file-size budget
//! (`tests/file_size_budget.rs`); the same `#[path]` split is used by
//! `typescript.rs` → `typescript_tests.rs`. `use super::*` resolves to the
//! `bash` module.
//!
//! These tests pin the Bash adapter's L1 operation scope (plan Iteration 8
//! RED): destructive commands (`rm` / `rmdir` / `unlink` / `chmod` / `chown` /
//! `chgrp`), truncating and append file redirects (`>` / `>>` / `&>` / `&>>` /
//! `>|`), the `tee` write command, and execution sinks (`eval`, `source` / `.`,
//! `bash` / `sh` / … `-c`, `python* -c`, `node -e`) with literal or dynamic
//! operand certainty, plus cross-language nested payloads (ADR-022 §7). The
//! genuine RED-risk is whether the pinned tree-sitter-bash 0.25.1 grammar
//! parses the modern/quirky shapes (arrays, `[[ ]]`, process substitution,
//! heredocs, arithmetic) cleanly and whether `calls.scm` matches the bash AST
//! (a `command` field `name: (command_name)` and a `file_redirect` node, not
//! the call-expression shapes the Python/JS/TS adapters use).

use super::*;
use crate::language::SourceLanguage;
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

/// Assert `source` records a nonzero parse-error count (incomplete syntax).
fn malformed(source: &str) {
    let result = analyze(source);
    assert!(
        result.parse_errors > 0,
        "incomplete syntax must record a nonzero parse error count for {source:?}"
    );
}

// --- Filesystem deletion ------------------------------------------------

#[test]
fn rm_recursive_forced_literal_path_yields_filesystem_delete() {
    let op = one_op("rm -rf /tmp/x");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert!(op.modifiers.recursive, "-r sets recursive");
    assert!(op.modifiers.forced, "-f sets forced");
    assert_eq!(op.certainty, OperandCertainty::Known, "literal path");
    assert!(op.payload.is_none());
}

#[test]
fn rm_plain_literal_path_yields_filesystem_delete_without_modifiers() {
    let op = one_op("rm file");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert_eq!(op.modifiers, OperationModifiers::default());
    assert_eq!(op.certainty, OperandCertainty::Known);
}

#[test]
fn rm_recursive_only_flag() {
    let op = one_op("rm -r d");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert!(op.modifiers.recursive);
    assert!(!op.modifiers.forced);
}

#[test]
fn rm_forced_only_flag() {
    let op = one_op("rm -f d");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert!(!op.modifiers.recursive);
    assert!(op.modifiers.forced);
}

#[test]
fn rm_long_flags_set_modifiers() {
    let op = one_op("rm --recursive --force d");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert!(op.modifiers.recursive);
    assert!(op.modifiers.forced);
}

#[test]
fn rmdir_and_unlink_are_filesystem_delete() {
    for src in ["rmdir d", "unlink f"] {
        let op = one_op(src);
        assert_eq!(op.kind, OperationKind::FilesystemDelete, "{src}");
        assert_eq!(op.modifiers, OperationModifiers::default(), "{src}");
    }
}

#[test]
fn rm_with_variable_path_is_dynamic() {
    let op = one_op("rm $x");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none());
}

#[test]
fn rm_with_command_substitution_path_is_dynamic() {
    let op = one_op("rm $(cat f)");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
}

// --- Filesystem overwrite: redirects ------------------------------------

#[test]
fn truncate_redirect_is_destructive_overwrite() {
    let op = one_op("> f");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(
        op.modifiers.destructive_mode,
        "`>` truncates and must set destructive_mode"
    );
    assert_eq!(op.certainty, OperandCertainty::Known, "literal destination");
}

#[test]
fn redirect_attached_to_a_command_is_overwrite() {
    // `echo` is not tracked; the only operation is the `> f` redirect.
    let op = one_op("echo hi > f");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(op.modifiers.destructive_mode);
    let op = one_op("cat > f");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(op.modifiers.destructive_mode);
}

#[test]
fn append_redirect_is_overwrite_without_destructive_mode() {
    let op = one_op(">> f");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(
        !op.modifiers.destructive_mode,
        "`>>` appends without truncating"
    );
    let op = one_op("echo hi >> f");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(!op.modifiers.destructive_mode);
}

#[test]
fn combined_stdout_stderr_truncate_redirect_is_destructive_overwrite() {
    // `&>` redirects both stdout and stderr and truncates.
    let op = one_op("echo hi &> f");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(op.modifiers.destructive_mode, "`&>` truncates");
}

#[test]
fn combined_stdout_stderr_append_redirect_is_overwrite_without_destructive_mode() {
    // `&>>` redirects both stdout and stderr and appends.
    let op = one_op("echo hi &>> f");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(!op.modifiers.destructive_mode, "`&>>` appends");
}

#[test]
fn clobber_redirect_is_destructive_overwrite() {
    // `>|` truncates, overriding `set -o noclobber`.
    let op = one_op("echo hi >| f");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(op.modifiers.destructive_mode, "`>|` truncates");
}

#[test]
fn input_redirect_is_not_an_overwrite() {
    no_ops("cat < f");
}

#[test]
fn fd_dup_redirect_is_not_an_overwrite() {
    // `>&2` duplicates a file descriptor; it is not a file write.
    no_ops("echo hi >&2");
}

#[test]
fn stderr_truncate_redirect_is_destructive_overwrite() {
    let op = one_op("echo hi 2> f");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(op.modifiers.destructive_mode);
}

#[test]
fn redirect_with_variable_destination_is_dynamic() {
    let op = one_op("> $x");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
}

// --- tee write command ---------------------------------------------------

#[test]
fn tee_literal_file_is_destructive_overwrite() {
    let op = one_op("tee f");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(
        op.modifiers.destructive_mode,
        "tee truncates by default and must set destructive_mode"
    );
    assert_eq!(op.certainty, OperandCertainty::Known);
}

#[test]
fn tee_append_flag_is_overwrite_without_destructive_mode() {
    let op = one_op("tee -a f");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(
        !op.modifiers.destructive_mode,
        "-a appends without truncating"
    );
}

#[test]
fn tee_append_long_flag_is_overwrite_without_destructive_mode() {
    let op = one_op("tee --append f");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(
        !op.modifiers.destructive_mode,
        "--append appends without truncating"
    );
}

#[test]
fn tee_with_variable_file_is_dynamic() {
    let op = one_op("tee $f");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
}

// --- Permission / ownership ---------------------------------------------

#[test]
fn chmod_chown_chgrp_are_permission_or_ownership() {
    for src in ["chmod 777 f", "chown u f", "chgrp g f"] {
        let op = one_op(src);
        assert_eq!(op.kind, OperationKind::PermissionOrOwnershipChange, "{src}");
    }
}

#[test]
fn chmod_with_variable_mode_is_dynamic() {
    let op = one_op("chmod $mode f");
    assert_eq!(op.kind, OperationKind::PermissionOrOwnershipChange);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
}

// --- Execution sinks: eval / bash -c / sh -c (Bash payload) ---------------

#[test]
fn bash_c_literal_emits_code_execution_with_bash_payload() {
    let op = one_op("bash -c \"rm x\"");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Known);
    let payload = op.payload.expect("literal shell payload recovered");
    assert_eq!(
        payload.language,
        SourceLanguage::Bash,
        "cross-language nesting"
    );
    assert_eq!(payload.source, "rm x");
}

#[test]
fn sh_c_single_quoted_literal_emits_bash_payload() {
    let op = one_op("sh -c 'rm x'");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    let payload = op.payload.expect("literal shell payload recovered");
    assert_eq!(payload.language, SourceLanguage::Bash);
    assert_eq!(payload.source, "rm x");
}

#[test]
fn bash_c_payload_is_the_argument_after_the_flag() {
    // Positional parameters after the payload (`arg0`) are not the payload.
    let op = one_op("bash -c \"rm x\" arg0");
    let payload = op.payload.as_ref().expect("payload recovered");
    assert_eq!(payload.source, "rm x");
}

#[test]
fn eval_literal_emits_code_execution_with_bash_payload() {
    let op = one_op("eval \"rm x\"");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Known);
    let payload = op.payload.expect("literal shell payload recovered");
    assert_eq!(payload.language, SourceLanguage::Bash);
    assert_eq!(payload.source, "rm x");
}

#[test]
fn bash_c_dynamic_payload_is_code_execution_without_nested_target() {
    let op = one_op("bash -c \"$x\"");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none(), "a dynamic payload is never evaluated");
}

#[test]
fn eval_dynamic_payload_is_code_execution_without_nested_target() {
    let op = one_op("eval $x");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none());
}

#[test]
fn eval_with_literal_and_dynamic_args_is_dynamic_without_payload() {
    // `eval` joins all arguments with spaces, so `eval "rm" "$x"` evaluates a
    // dynamic string. Recovering only the literal `"rm"` would hide the
    // dynamic tail — the unsafe direction. The multi-arg shape degrades to
    // Dynamic with no nested target until joining lands.
    let op = one_op("eval \"rm\" \"$x\"");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(
        op.certainty,
        OperandCertainty::Dynamic,
        "a multi-argument eval is dynamic even if the first arg is literal"
    );
    assert!(op.payload.is_none());
}

#[test]
fn eval_with_extra_literal_args_is_dynamic_without_payload() {
    // Even with no expansion, more than one argument means the evaluated
    // string is the join, not the single literal — degrade to Dynamic.
    let op = one_op("eval \"rm x\" extra");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none());
}

// --- Execution sinks: python -c / node -e (cross-language payload) -------

#[test]
fn python_c_literal_emits_code_execution_with_python_payload() {
    let op = one_op("python3 -c \"os.remove(x)\"");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Known);
    let payload = op.payload.expect("literal python payload recovered");
    assert_eq!(
        payload.language,
        SourceLanguage::Python,
        "cross-language nesting into Python"
    );
    assert_eq!(payload.source, "os.remove(x)");
}

#[test]
fn python_c_single_quoted_literal_emits_python_payload() {
    let op = one_op("python -c 'print(1)'");
    let payload = op.payload.as_ref().expect("payload recovered");
    assert_eq!(payload.language, SourceLanguage::Python);
    assert_eq!(payload.source, "print(1)");
}

#[test]
fn versioned_python_interpreter_is_recognized() {
    // `python3.11` is a versioned Python interpreter basename.
    let op = one_op("python3.11 -c \"os.remove(x)\"");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    let payload = op.payload.as_ref().expect("payload recovered");
    assert_eq!(payload.language, SourceLanguage::Python);
    assert_eq!(payload.source, "os.remove(x)");
}

#[test]
fn python_prefixed_name_is_not_an_interpreter() {
    // `python3foo` is not a Python interpreter; it must not be classified as
    // a cross-language execution sink.
    no_ops("python3foo -c \"os.remove(x)\"");
}

#[test]
fn node_e_literal_emits_code_execution_with_javascript_payload() {
    let op = one_op("node -e \"fs.unlinkSync(x)\"");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Known);
    let payload = op.payload.expect("literal javascript payload recovered");
    assert_eq!(
        payload.language,
        SourceLanguage::JavaScript,
        "cross-language nesting into JavaScript"
    );
    assert_eq!(payload.source, "fs.unlinkSync(x)");
}

#[test]
fn node_eval_long_flag_emits_javascript_payload() {
    let op = one_op("node --eval \"fs.unlinkSync(x)\"");
    let payload = op.payload.as_ref().expect("payload recovered");
    assert_eq!(payload.language, SourceLanguage::JavaScript);
    assert_eq!(payload.source, "fs.unlinkSync(x)");
}

#[test]
fn python_c_dynamic_payload_is_code_execution_without_nested_target() {
    let op = one_op("python3 -c \"$x\"");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none());
}

// --- Execution sinks: source / . (path operand, no inline payload) -------

#[test]
fn source_literal_path_emits_code_execution_without_inline_payload() {
    let op = one_op("source s.sh");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    // The file's contents are not in the source, so no inline nested target is
    // recovered (a ScriptFile recursive target is the orchestration layer's
    // job, like Python `subprocess` on a script).
    assert!(
        op.payload.is_none(),
        "source does not carry an inline payload"
    );
}

#[test]
fn dot_builtin_emits_code_execution_without_inline_payload() {
    let op = one_op(". s.sh");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert!(op.payload.is_none());
}

// --- Negatives ----------------------------------------------------------

#[test]
fn comment_mentioning_rm_is_not_an_operation() {
    no_ops("# rm -rf /");
}

#[test]
fn rm_inside_an_echo_string_is_not_an_operation() {
    no_ops("echo \"rm -rf /\"");
}

#[test]
fn rm_prefixed_command_name_is_not_rm() {
    no_ops("rm_func x");
}

#[test]
fn unrelated_commands_are_not_operations() {
    no_ops("ls -la");
    no_ops("echo hi");
}

#[test]
fn variable_assignment_is_not_a_command() {
    no_ops("x=rm");
}

#[test]
fn declaration_and_unset_commands_are_not_operations() {
    no_ops("export FOO=bar");
    no_ops("unset FOO");
}

#[test]
fn test_command_is_not_an_operation() {
    no_ops("[[ -f x ]]");
}

#[test]
fn function_definition_named_rm_is_not_the_rm_command() {
    no_ops("function rm { echo hi; }");
}

// --- Composition / ordering / spans -------------------------------------

#[test]
fn multiple_rm_operations_are_emitted_in_source_order() {
    let result = analyze("rm a; rm b");
    assert_eq!(result.operations.len(), 2);
    assert_eq!(result.operations[0].kind, OperationKind::FilesystemDelete);
    assert_eq!(result.operations[1].kind, OperationKind::FilesystemDelete);
    assert!(
        result.operations[0].span.byte_start < result.operations[1].span.byte_start,
        "operations must be in document order"
    );
}

#[test]
fn rm_with_redirect_emits_delete_and_overwrite_in_source_order() {
    let result = analyze("rm a > out");
    assert_eq!(result.operations.len(), 2);
    assert_eq!(result.operations[0].kind, OperationKind::FilesystemDelete);
    assert_eq!(
        result.operations[1].kind,
        OperationKind::FilesystemOverwrite
    );
    assert!(
        result.operations[0].span.byte_start < result.operations[1].span.byte_start,
        "operations must be in document order"
    );
}

#[test]
fn destructive_inside_command_substitution_surfaces_both_operations() {
    // The query matches commands recursively, so a destructive command nested
    // inside `$(…)` surfaces as its own operation alongside the outer one.
    // `rm $(rm x)` → the inner `rm x` (a delete) and the outer `rm` (a delete
    // whose operand is the dynamic `$(rm x)`).
    let result = analyze("rm $(rm x)");
    assert_eq!(result.parse_errors, 0);
    assert_eq!(
        result.operations.len(),
        2,
        "both the outer and inner rm must surface"
    );
    assert_eq!(result.operations[0].kind, OperationKind::FilesystemDelete);
    assert_eq!(result.operations[1].kind, OperationKind::FilesystemDelete);
    assert_eq!(
        result.operations[0].certainty,
        OperandCertainty::Dynamic,
        "outer rm operand is the dynamic $(rm x)"
    );
    assert_eq!(
        result.operations[1].certainty,
        OperandCertainty::Known,
        "inner rm x has a literal operand"
    );
    assert!(
        result.operations[0].span.byte_start < result.operations[1].span.byte_start,
        "operations must be in document order"
    );
}

#[test]
fn command_span_covers_the_command() {
    let src = "rm -rf /tmp/x";
    let op = one_op(src);
    assert_eq!(op.span.byte_start, 0);
    assert_eq!(op.span.byte_end, src.len());
    assert_eq!(op.span.line, 1);
    assert_eq!(op.span.column, 1);
}

// --- Modern / quirky bash parses clean ----------------------------------

#[test]
fn array_assignment_parses_clean_with_no_ops() {
    no_ops("arr=(1 2 3); echo ${arr[0]}");
}

#[test]
fn test_command_chained_with_rm_keeps_rm_and_drops_test() {
    // `[[ -f x ]]` is a test_command (not matched); `rm x` is a command. The
    // modern `[[ ]]` shape must parse clean and the chained `rm` must still
    // surface.
    let result = analyze("[[ -f x ]] && rm x");
    assert_eq!(result.parse_errors, 0);
    assert_eq!(result.operations.len(), 1);
    assert_eq!(result.operations[0].kind, OperationKind::FilesystemDelete);
}

#[test]
fn process_substitution_parses_clean_with_no_ops() {
    no_ops("cat <(echo hi)");
}

#[test]
fn for_loop_parses_clean_with_no_ops() {
    no_ops("for i in 1 2; do echo $i; done");
}

#[test]
fn arithmetic_expansion_parses_clean_with_no_ops() {
    no_ops("x=$((1 + 2))");
}

#[test]
fn ansi_c_string_parses_clean_with_no_ops() {
    no_ops("echo $'hi\\n'");
}

#[test]
fn heredoc_body_is_not_reparsed_into_commands() {
    // tree-sitter-bash parses a heredoc body as one `heredoc_body` text node,
    // not as commands, so the adapter does not surface the `rm` inside it. The
    // orchestration layer re-feeds a quoted heredoc body as its own Bash
    // target (plan Iteration 4), where `rm -rf /tmp/x` is a top-level command
    // — it is not missed, just not re-parsed inside the parent's heredoc node.
    no_ops("cat <<EOF\nrm -rf /tmp/x\nEOF");
}

// --- Malformed source ---------------------------------------------------

#[test]
fn unterminated_string_records_parse_errors() {
    malformed("\"rm x");
}

#[test]
fn unterminated_bash_c_payload_records_parse_errors() {
    malformed("bash -c \"rm x");
}

// --- Empty source -------------------------------------------------------

#[test]
fn empty_source_records_no_operations_and_no_errors() {
    let result = analyze("");
    assert_eq!(result.parse_errors, 0);
    assert!(result.operations.is_empty());
}
