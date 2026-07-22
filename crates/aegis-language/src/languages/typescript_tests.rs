//! Unit tests for the TypeScript adapter (`typescript.rs`).
//!
//! Lives in a sibling file via `#[path = "typescript_tests.rs"]` to keep the
//! adapter source under the workspace 800-line file-size budget
//! (`tests/file_size_budget.rs`); the same `#[path]` split is used by
//! `javascript.rs` → `javascript_tests.rs`. `use super::*` resolves to the
//! `typescript` module.
//!
//! TypeScript shares the JavaScript-family classification (`family` module) but
//! owns its own grammar (`LANGUAGE_TYPESCRIPT`) and query. These tests prove the
//! TS adapter routes TS source through that shared classifier (one tracer per
//! operation category), and pin TypeScript-only syntax the JS suite does not
//! cover: calls with type arguments, generic class methods, type annotations,
//! interfaces / enums / type aliases / `import type` / `as` / `satisfies` /
//! decorators as negatives, and a modern-TS parse-clean case. The genuine
//! RED-risk is whether the pinned tree-sitter-typescript 0.23.2 grammar parses
//! current TypeScript cleanly and whether the `calls.scm` query matches TS
//! ASTs — including type-argument calls — unchanged.

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

// --- One tracer per operation category (shared JS-family classifier) --------

#[test]
fn fs_unlinksync_literal_path_yields_filesystem_delete_known() {
    let op = one_op("fs.unlinkSync(\"data.txt\")");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert_eq!(op.certainty, OperandCertainty::Known);
    assert_eq!(op.modifiers, OperationModifiers::default());
    assert!(op.payload.is_none());
}

#[test]
fn fs_rmsync_recursive_option_sets_recursive_modifier() {
    let op = one_op("fs.rmSync(\"d\", {recursive: true})");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert!(op.modifiers.recursive);
}

#[test]
fn fs_writesync_is_destructive_overwrite() {
    let op = one_op("fs.writeFileSync(\"f\", \"x\")");
    assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
    assert!(op.modifiers.destructive_mode);
    assert_eq!(op.certainty, OperandCertainty::Known);
}

#[test]
fn fs_chmodsync_is_permission_or_ownership() {
    let op = one_op("fs.chmodSync(\"f\", 0o644)");
    assert_eq!(op.kind, OperationKind::PermissionOrOwnershipChange);
}

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
    let op = one_op("new Function(\"return fs.unlinkSync('x')\")");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    let payload = op.payload.expect("literal payload must be recovered");
    assert_eq!(payload.language, SourceLanguage::JavaScript);
    assert_eq!(payload.source, "return fs.unlinkSync('x')");
}

#[test]
fn child_process_exec_literal_emits_code_execution_with_bash_payload() {
    let op = one_op("child_process.exec(\"rm -rf /tmp/x\")");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    let payload = op.payload.expect("literal shell payload recovered");
    assert_eq!(payload.language, SourceLanguage::Bash);
    assert_eq!(payload.source, "rm -rf /tmp/x");
}

#[test]
fn child_process_spawn_argv_form_is_dynamic_without_payload() {
    let op = one_op("child_process.spawn(\"rm\", [\"-rf\", \"x\"])");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none());
}

// --- Dynamic operands ----------------------------------------------------

#[test]
fn fs_unlinksync_with_variable_path_is_dynamic() {
    let op = one_op("fs.unlinkSync(path)");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none());
}

#[test]
fn fs_unlinksync_with_template_interpolation_is_dynamic() {
    let op = one_op("fs.unlinkSync(`${name}`)");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
}

// --- TypeScript-only syntax: type arguments on a call (RED-risk) -----------
// A call with explicit type arguments (`fs.unlinkSync<void>("x")`) must still
// match the `calls.scm` query and surface its operation — the `type_arguments`
// node is a separate child of `call_expression`, not the `function` field, so
// the query's `function: (member_expression …)` pattern must still bind.

#[test]
fn call_with_type_arguments_still_surfaces_the_operation() {
    let op = one_op("fs.unlinkSync<void>(\"x\")");
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert_eq!(op.certainty, OperandCertainty::Known, "literal path");
    assert!(op.payload.is_none());
}

#[test]
fn exec_with_type_arguments_still_surfaces_code_execution() {
    let op = one_op("eval<string>(\"fs.unlinkSync('x')\")");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    let payload = op.payload.expect("literal payload must be recovered");
    assert_eq!(payload.language, SourceLanguage::JavaScript);
    assert_eq!(payload.source, "fs.unlinkSync('x')");
}

// `new`-expression with explicit type arguments exercises the third
// `calls.scm` pattern (`new_expression constructor: (identifier)`) under the
// same RED-risk as the two `call_expression` patterns: the `type_arguments`
// child must not detach the `constructor: (identifier)` field binding. A
// non-idiomatic construct, but it closes the type-argument triangle so all
// three query patterns are empirically confirmed over TS type-arg syntax.
#[test]
fn new_expression_with_type_arguments_still_surfaces_code_execution() {
    let op = one_op("new Function<string>(\"return fs.unlinkSync('x')\")");
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Known);
    let payload = op.payload.expect("literal payload must be recovered");
    assert_eq!(payload.language, SourceLanguage::JavaScript);
    assert_eq!(payload.source, "return fs.unlinkSync('x')");
}

// --- TypeScript-only syntax: generic class method --------------------------
// A destructive call inside a generic class method must surface.

#[test]
fn generic_class_method_calling_fs_surfaces_the_operation() {
    let src = "class C<T> {\n  m(): void {\n    fs.unlinkSync(\"x\");\n  }\n}";
    let op = one_op(src);
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert_eq!(op.certainty, OperandCertainty::Known);
}

// --- TypeScript-only syntax: type annotations on operands ------------------
// A typed variable operand is still `Dynamic` (bounded resolution is deferred;
// a typed variable is never evidence of safety — ADR-022 §3/§7).

#[test]
fn typed_variable_operand_is_dynamic() {
    let src = "function f(p: string) {\n  fs.unlinkSync(p);\n}";
    let op = one_op(src);
    assert_eq!(op.kind, OperationKind::FilesystemDelete);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none());
}

#[test]
fn typed_exec_operand_is_dynamic() {
    let src = "function f(cmd: string) {\n  child_process.exec(cmd);\n}";
    let op = one_op(src);
    assert_eq!(op.kind, OperationKind::CodeExecution);
    assert_eq!(op.certainty, OperandCertainty::Dynamic);
    assert!(op.payload.is_none());
}

// --- Negatives: TS-only declarations and constructs ------------------------
// Interfaces, enums, type aliases, type-only imports, `as` casts, decorators,
// and ordinary comment/string/member-reference forms must NOT surface as
// operations. These pin the adapter's narrowness over TS-only syntax.

#[test]
fn comment_mentioning_fs_unlinksync_is_not_an_operation() {
    no_ops("// fs.unlinkSync(\"x\")");
}

#[test]
fn string_literal_mentioning_fs_unlinksync_is_not_an_operation() {
    no_ops("\"fs.unlinkSync('x')\"");
}

#[test]
fn member_reference_with_as_cast_is_not_an_operation() {
    // `fs.unlinkSync` referenced (not called), then `as`-cast — no call site.
    no_ops("const ref = fs.unlinkSync as (p: string) => void");
}

#[test]
fn unrelated_call_is_not_an_operation() {
    no_ops("console.log(\"hello\")");
}

#[test]
fn interface_declaration_is_not_an_operation() {
    no_ops("interface F {\n  unlink(path: string): void;\n}");
}

#[test]
fn enum_declaration_is_not_an_operation() {
    no_ops("enum E { A, B, C }");
}

#[test]
fn type_alias_is_not_an_operation() {
    no_ops("type T = string;");
}

#[test]
fn type_only_import_is_not_an_operation() {
    no_ops("import type { Stats } from \"fs\";");
}

#[test]
fn as_cast_expression_is_not_an_operation() {
    no_ops("const x = \"y\" as const;");
}

#[test]
fn satisfies_expression_parses_cleanly_and_is_not_an_operation() {
    // `satisfies` (TS 4.9+): pins the pinned grammar parses current TS.
    no_ops("const config = { a: 1 } satisfies Record<string, number>;");
}

#[test]
fn decorator_parses_cleanly_and_is_not_an_operation() {
    no_ops("@logged\nclass C {}");
}

// --- Modern TS parse-clean ------------------------------------------------
// A representative mix of current TypeScript (generics, type annotations,
// optional chaining, template literal types, utility types) must parse cleanly
// and surface no false operations over non-tracked call sites.

#[test]
fn modern_typescript_parses_cleanly_with_no_false_operations() {
    let src = "type Box<T> = { value: T };\n\
               const unwrap = <T>(b: Box<T>): T => b.value;\n\
               const n = unwrap({ value: 1 });\n\
               const m = data?.items?.[0]?.name ?? \"default\";\n\
               const id = <U,>(x: U): U => x;\n";
    no_ops(src);
}

// --- Composition / ordering ------------------------------------------------

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

// --- Malformed source ------------------------------------------------------

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
