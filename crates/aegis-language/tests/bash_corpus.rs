//! Bash adapter corpus (plan Iteration 8, ADR-022 §3, §7).
//!
//! The checked-in Bash sources are embedded at compile time and exercised
//! through the public `aegis_language::languages::bash::analyze` seam. The
//! expectations are hand-derived from shell semantics and ADR-022, rather than
//! from the adapter implementation, so this corpus characterizes the qualified
//! Slice 1 behavior and catches grammar or interpretation regressions.
//!
//! This corpus intentionally stays at the adapter seam. Worker dispatch,
//! routing of heredoc bodies, and preservation/deduplication of outer Scanner
//! Matches are separate Iteration 8 slices; asserting them here would create a
//! synthetic pipeline test.

#[path = "common/corpus_harness.rs"]
mod corpus_harness;

use aegis_language::SourceLanguage;
use aegis_language::languages::bash::analyze;
use aegis_language::operation::{OperandCertainty, OperationKind, OperationModifiers};
use corpus_harness::{ExpectedOp, assert_clean_no_ops, assert_malformed, assert_ops, bash_exec};

const FS_DELETE: &str = include_str!("corpora/bash/fs_delete.sh");
const FS_OVERWRITE: &str = include_str!("corpora/bash/fs_overwrite.sh");
const PERMS: &str = include_str!("corpora/bash/perms.sh");
const EXEC_SHELL: &str = include_str!("corpora/bash/exec_shell.sh");
const EXEC_CROSS_LANGUAGE: &str = include_str!("corpora/bash/exec_cross_language.sh");
const NEGATIVES: &str = include_str!("corpora/bash/negatives.sh");
const DYNAMIC_OPERAND: &str = include_str!("corpora/bash/dynamic_operand.sh");
const MODERN_SYNTAX: &str = include_str!("corpora/bash/modern_syntax.sh");
const MALFORMED: &str = include_str!("corpora/bash/malformed.sh");

#[test]
fn fs_delete_emits_rm_rmdir_and_unlink_with_rm_modifiers() {
    let expected = [
        ExpectedOp {
            kind: OperationKind::FilesystemDelete,
            modifiers: OperationModifiers {
                recursive: true,
                forced: true,
                ..OperationModifiers::default()
            },
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
    ];
    assert_ops(analyze, FS_DELETE, &expected);
}

#[test]
fn fs_overwrite_distinguishes_truncation_from_append() {
    let expected = [
        overwrite(true, OperandCertainty::Known),
        overwrite(false, OperandCertainty::Known),
        overwrite(true, OperandCertainty::Known),
        overwrite(false, OperandCertainty::Known),
    ];
    assert_ops(analyze, FS_OVERWRITE, &expected);
}

#[test]
fn perms_emits_permission_or_ownership_operations() {
    let expected = [
        permission_change(),
        permission_change(),
        permission_change(),
    ];
    assert_ops(analyze, PERMS, &expected);
}

#[test]
fn shell_execution_emits_bash_payloads() {
    let expected = [
        bash_exec("rm -rf /tmp/from-bash"),
        bash_exec("rm /tmp/from-sh"),
        bash_exec("rm /tmp/from-eval"),
    ];
    assert_ops(analyze, EXEC_SHELL, &expected);
}

#[test]
fn cross_language_execution_emits_python_and_javascript_payloads() {
    let expected = [
        exec(SourceLanguage::Python, "os.remove('data.txt')"),
        exec(SourceLanguage::JavaScript, "fs.unlinkSync('data.txt')"),
    ];
    assert_ops(analyze, EXEC_CROSS_LANGUAGE, &expected);
}

#[test]
fn comments_strings_and_non_calls_emit_no_operations() {
    assert_clean_no_ops(analyze, NEGATIVES);
}

#[test]
fn dynamic_operands_keep_operations_without_nested_payloads() {
    let expected = [
        ExpectedOp {
            kind: OperationKind::FilesystemDelete,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Dynamic,
            payload: None,
        },
        overwrite(true, OperandCertainty::Dynamic),
        ExpectedOp {
            kind: OperationKind::FilesystemDelete,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Dynamic,
            payload: None,
        },
        ExpectedOp {
            kind: OperationKind::FilesystemDelete,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Known,
            payload: None,
        },
        dynamic_exec(),
        dynamic_exec(),
        dynamic_exec(),
    ];
    assert_ops(analyze, DYNAMIC_OPERAND, &expected);
}

#[test]
fn modern_shell_syntax_and_heredoc_parse_cleanly_without_false_operations() {
    assert_clean_no_ops(analyze, MODERN_SYNTAX);
}

#[test]
fn malformed_source_records_parse_errors() {
    assert_malformed(analyze, MALFORMED);
}

fn overwrite(destructive_mode: bool, certainty: OperandCertainty) -> ExpectedOp {
    ExpectedOp {
        kind: OperationKind::FilesystemOverwrite,
        modifiers: OperationModifiers {
            destructive_mode,
            ..OperationModifiers::default()
        },
        certainty,
        payload: None,
    }
}

fn permission_change() -> ExpectedOp {
    ExpectedOp {
        kind: OperationKind::PermissionOrOwnershipChange,
        modifiers: OperationModifiers::default(),
        certainty: OperandCertainty::Known,
        payload: None,
    }
}

fn dynamic_exec() -> ExpectedOp {
    ExpectedOp {
        kind: OperationKind::CodeExecution,
        modifiers: OperationModifiers::default(),
        certainty: OperandCertainty::Dynamic,
        payload: None,
    }
}

fn exec(language: SourceLanguage, payload: &'static str) -> ExpectedOp {
    ExpectedOp {
        kind: OperationKind::CodeExecution,
        modifiers: OperationModifiers::default(),
        certainty: OperandCertainty::Known,
        payload: Some((language, payload)),
    }
}
