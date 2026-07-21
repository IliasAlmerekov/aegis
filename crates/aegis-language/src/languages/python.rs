//! Python language adapter (plan Iteration 6, Slice 1).
//!
//! Structural capture via the bundled Tree-sitter query
//! (`queries/python/calls.scm`); semantic interpretation in typed Rust. The
//! adapter emits language-neutral [`DetectedOperation`]s and, for execution
//! sinks with a statically recovered literal payload, a [`NestedTarget`] for
//! bounded recursive analysis (ADR-022 §3, §7). It never assigns a final
//! `RiskLevel` — the root crate maps these operations through the shared
//! classifier (Iteration 5 REVIEW GATE).
//!
//! Slice 1 scope: fully-qualified destructive and execution-sink calls with
//! literal/dynamic operand certainty. Bounded symbol resolution (imports,
//! aliases, simple constants → `OperandCertainty::Partial`) is a later slice;
//! the `os.exec*` family (argv, not source) is deferred with the same note.

use std::cell::RefCell;
use std::sync::LazyLock;

use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator};

use crate::language::SourceLanguage;
use crate::operation::{
    AdapterResult, ByteSpan, DetectedOperation, NestedTarget, OperandCertainty, OperationKind,
    OperationModifiers,
};

/// The bundled Python call-capture query. Compiled once on first use; a failure
/// here is a build-time query-authoring bug, so panicking on first use is the
/// correct startup behavior (CLAUDE.md: `.expect()` is acceptable in startup
/// initialization).
static CALLS_QUERY: LazyLock<Query> = LazyLock::new(|| {
    Query::new(
        &SourceLanguage::Python.tree_sitter_language(),
        include_str!("../../queries/python/calls.scm"),
    )
    .expect("bundled python/calls.scm must compile against the pinned grammar")
});

// A per-thread reusable Python parser.
//
// `set_language` runs once per thread on first use (one-time initialization,
// not on every `analyze` call) — a failure here is a build-time grammar-pin
// bug, not user input, so panicking on init is the correct startup behavior
// (CLAUDE.md: `.expect()` is acceptable in startup initialization). Keeping
// the parser out of `analyze`'s body avoids re-running `set_language` on each
// call and avoids `.expect()` on a per-invocation path. `thread_local!` gives
// each thread its own parser with no locking; `analyze` is non-reentrant, so
// the `borrow_mut` cannot clash.
thread_local! {
    static PARSER: RefCell<Parser> = RefCell::new({
        let mut parser = Parser::new();
        parser
            .set_language(&SourceLanguage::Python.tree_sitter_language())
            .expect("pinned python grammar is ABI-compatible with the runtime");
        parser
    });
}

/// The payload language an execution sink's literal source should be recursively
/// parsed as (ADR-022 §7 cross-language nesting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecLang {
    /// `eval` / `exec` payloads are Python source.
    Python,
    /// `os.system` / `subprocess.*` string payloads are shell source (Bash).
    Bash,
}

/// How a recognized call site is classified, before operand certainty is
/// attached. Routes a function path to the shared operation vocabulary without
/// assigning `RiskLevel` (Iteration 5 REVIEW GATE).
#[derive(Debug, Clone, PartialEq, Eq)]
enum CallClass {
    /// A non-execution destructive operation with fixed modifiers.
    Op(OperationKind, OperationModifiers),
    /// `open(...)` — overwrite/truncation depends on the mode argument.
    Open,
    /// A recognized execution sink whose literal payload is recursively
    /// analyzable in `ExecLang`.
    Exec(ExecLang),
}

/// Classify a fully-qualified call path into a [`CallClass`], or `None` when the
/// call is not a tracked destructive or execution-sink API.
fn classify_path(path: &str) -> Option<CallClass> {
    Some(match path {
        "os.remove" | "os.unlink" | "os.rmdir" => CallClass::Op(
            OperationKind::FilesystemDelete,
            OperationModifiers::default(),
        ),
        "shutil.rmtree" => CallClass::Op(
            OperationKind::FilesystemDelete,
            OperationModifiers {
                recursive: true,
                ..OperationModifiers::default()
            },
        ),
        "os.chmod" | "os.chown" | "shutil.chown" => CallClass::Op(
            OperationKind::PermissionOrOwnershipChange,
            OperationModifiers::default(),
        ),
        "open" => CallClass::Open,
        "eval" | "exec" => CallClass::Exec(ExecLang::Python),
        "os.system"
        | "subprocess.run"
        | "subprocess.call"
        | "subprocess.Popen"
        | "subprocess.check_call"
        | "subprocess.check_output" => CallClass::Exec(ExecLang::Bash),
        _ => return None,
    })
}

/// Analyze Python `source` for destructive effects and execution sinks.
///
/// Pure and in-process: parses `source` with the pinned Python grammar, runs
/// the call-capture query, and interprets each call site in Rust. No filesystem
/// access, no subprocess (ADR-022 §2). A nonzero `parse_errors` means the source
/// was malformed; the root mapping records `DegradationReason::IncompleteSyntax`.
#[must_use]
pub fn analyze(source: &str) -> AdapterResult {
    // `parse` returns `None` only on a NULL C result; treat as a malformed
    // (unrecoverable) parse. A real empty program still produces a tree. The
    // parser is held in `PARSER` (per-thread, one-time `set_language`); the
    // borrow is released before any node traversal below.
    let Some(tree) = PARSER.with(|cell| cell.borrow_mut().parse(source.as_bytes(), None)) else {
        return AdapterResult {
            operations: Vec::new(),
            parse_errors: 1,
        };
    };

    let parse_errors = u32::from(tree.root_node().has_error());
    let root = tree.root_node();
    let bytes = source.as_bytes();

    let mut operations = Vec::new();
    let mut cursor = QueryCursor::new();
    let capture_names = CALLS_QUERY.capture_names();
    let mut matches = cursor.matches(&CALLS_QUERY, root, bytes);
    while let Some(m) = matches.next() {
        // Resolve this match's captures by name. A match carries @call, @args,
        // and either (@obj, @attr) for an attribute call or @fname for a bare
        // identifier call (the query's two patterns are disjoint on the
        // `function` field shape).
        let mut call = None;
        let mut args = None;
        let mut obj = None;
        let mut attr = None;
        let mut fname = None;
        for cap in m.captures {
            let name = capture_names[cap.index as usize];
            match name {
                "call" => call = Some(cap.node),
                "args" => args = Some(cap.node),
                "obj" => obj = Some(cap.node),
                "attr" => attr = Some(cap.node),
                "fname" => fname = Some(cap.node),
                _ => {}
            }
        }
        let (Some(call_node), Some(args_node)) = (call, args) else {
            // A well-formed query match always carries both; skip defensively.
            continue;
        };

        let path = if let (Some(obj), Some(attr)) = (obj, attr) {
            format!(
                "{}.{}",
                obj.utf8_text(bytes).unwrap_or_default(),
                attr.utf8_text(bytes).unwrap_or_default()
            )
        } else if let Some(fname) = fname {
            fname.utf8_text(bytes).unwrap_or_default().to_string()
        } else {
            continue;
        };

        let Some(class) = classify_path(&path) else {
            continue; // not a tracked API
        };

        if let Some(op) = interpret(&class, call_node, args_node, bytes) {
            operations.push(op);
        }
    }

    // Tree-sitter yields matches in capture order; sort by source position so
    // callers see operations in document order regardless of query internals.
    operations.sort_by_key(|op| (op.span.byte_start, op.span.byte_end));

    AdapterResult {
        operations,
        parse_errors,
    }
}

/// Build a [`DetectedOperation`] for one classified call site, attaching operand
/// certainty and (for execution sinks) a nested literal payload.
fn interpret(
    class: &CallClass,
    call_node: Node,
    args_node: Node,
    bytes: &[u8],
) -> Option<DetectedOperation> {
    let span = span_for(call_node);
    match class {
        CallClass::Op(kind, mods) => {
            let operand = first_positional_arg(args_node);
            let certainty = operand_certainty(operand);
            Some(DetectedOperation {
                kind: *kind,
                modifiers: *mods,
                certainty,
                span,
                payload: None,
            })
        }
        CallClass::Open => {
            let mode = open_mode(args_node, bytes)?;
            let operand = first_positional_arg(args_node);
            let certainty = operand_certainty(operand);
            Some(DetectedOperation {
                kind: OperationKind::FilesystemOverwrite,
                modifiers: OperationModifiers {
                    destructive_mode: mode.destructive_mode,
                    ..OperationModifiers::default()
                },
                certainty,
                span,
                payload: None,
            })
            .filter(|_| mode.overwrite)
        }
        CallClass::Exec(lang) => {
            let operand = first_positional_arg(args_node);
            let payload = operand.and_then(|n| {
                string_literal_content(n, bytes).map(|content| NestedTarget {
                    language: exec_language(*lang),
                    source: content,
                    span: span_for(n),
                })
            });
            // certainty == Known iff a literal source payload was recovered.
            let certainty = if payload.is_some() {
                OperandCertainty::Known
            } else {
                OperandCertainty::Dynamic
            };
            Some(DetectedOperation {
                kind: OperationKind::CodeExecution,
                modifiers: OperationModifiers::default(),
                certainty,
                span,
                payload,
            })
        }
    }
}

/// Map an [`ExecLang`] to the [`SourceLanguage`] its payload is parsed as.
fn exec_language(lang: ExecLang) -> SourceLanguage {
    match lang {
        ExecLang::Python => SourceLanguage::Python,
        ExecLang::Bash => SourceLanguage::Bash,
    }
}

/// The first positional argument of an `argument_list`, or `None` when the call
/// has no positional argument (a `keyword_argument` is not positional).
fn first_positional_arg(args_node: Node) -> Option<Node> {
    let mut cursor = args_node.walk();
    args_node
        .named_children(&mut cursor)
        .find(|c| c.kind() != "keyword_argument")
}

/// Operand certainty for a non-execution operand: `Known` iff it is a pure
/// string literal (a `string` with no interpolation, or a `concatenated_string`
/// of pure strings); otherwise `Dynamic`. A computed/imported operand is never
/// evidence of safety (ADR-022 §3, §7).
fn operand_certainty(arg: Option<Node>) -> OperandCertainty {
    match arg {
        Some(n) if is_string_literal(n) => OperandCertainty::Known,
        _ => OperandCertainty::Dynamic,
    }
}

/// Whether `node` is a pure string literal: a `string` with no interpolation,
/// or a `concatenated_string` whose every child is a pure string literal.
/// Structural — does not decode the content.
fn is_string_literal(node: Node) -> bool {
    match node.kind() {
        "string" => {
            let mut cursor = node.walk();
            !node
                .named_children(&mut cursor)
                .any(|c| c.kind() == "interpolation")
        }
        "concatenated_string" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor).all(is_string_literal)
        }
        _ => false,
    }
}

/// The text content of a pure string-literal node, or `None` when the node is
/// not a literal (a variable, f-string with interpolation, non-string child in
/// a concatenation, etc.).
fn string_literal_content(node: Node, bytes: &[u8]) -> Option<String> {
    match node.kind() {
        "string" => {
            // An f-string carries `interpolation` children; that makes it
            // non-literal. A plain string has exactly one `string_content`.
            let mut cursor = node.walk();
            let mut content: Option<String> = None;
            for child in node.named_children(&mut cursor) {
                match child.kind() {
                    "string_content" => {
                        content = Some(child.utf8_text(bytes).ok()?.to_string());
                    }
                    "interpolation" => return None,
                    _ => {}
                }
            }
            content
        }
        "concatenated_string" => {
            let mut cursor = node.walk();
            let mut joined = String::new();
            for child in node.named_children(&mut cursor) {
                if child.kind() != "string" {
                    return None;
                }
                joined.push_str(&string_literal_content(child, bytes)?);
            }
            Some(joined)
        }
        _ => None,
    }
}

/// The destructive shape of an `open(...)` call, resolved from its mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OpenMode {
    /// Whether the call overwrites or appends to an existing file.
    overwrite: bool,
    /// Whether the mode truncates an existing file (`'w'`).
    destructive_mode: bool,
}

/// Determine whether an `open(...)` call is an overwrite/truncation, and whether
/// it is in a truncating (`'w'`) destructive mode. Returns `None` when the call
/// is not destructive (read-only, exclusive-create, or no mode argument).
///
/// Mode resolution: a `mode=` keyword argument wins; otherwise the second
/// positional argument. Slice-1 simplification: `'w'` (truncate) and `'a'`
/// (append) are treated as overwrite of an existing file; `'w'` additionally
/// sets `destructive_mode`. `'x'` (exclusive create, fails if the file exists)
/// and `'r'` (read) are not destructive. `'r+'` is read-write without truncate.
fn open_mode(args_node: Node, bytes: &[u8]) -> Option<OpenMode> {
    let mode = open_mode_string(args_node, bytes)?;
    Some(OpenMode {
        overwrite: mode.contains('w') || mode.contains('a'),
        destructive_mode: mode.contains('w'),
    })
}

/// The `open` mode string, from a `mode=` keyword argument or the second
/// positional argument, when it is a pure string literal.
fn open_mode_string(args_node: Node, bytes: &[u8]) -> Option<String> {
    let mut cursor = args_node.walk();
    let mut positional = 0u32;
    for child in args_node.named_children(&mut cursor) {
        if child.kind() == "keyword_argument" {
            // `mode=...`: the value is the second named child of the
            // keyword_argument (after the `name` identifier).
            let name = child.child_by_field_name("name")?;
            if name.utf8_text(bytes).ok()? == "mode" {
                let value = child.child_by_field_name("value")?;
                return string_literal_content(value, bytes);
            }
        } else {
            positional += 1;
            if positional == 2 {
                // Second positional argument is the mode by convention.
                return string_literal_content(child, bytes);
            }
        }
    }
    None
}

/// Build a [`ByteSpan`] for `node` (1-based line/column, byte offsets into the
/// source).
fn span_for(node: Node) -> ByteSpan {
    let start = node.start_position();
    ByteSpan {
        line: start.row as u32 + 1,
        column: start.column as u32 + 1,
        byte_start: node.start_byte(),
        byte_end: node.end_byte(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::{OperandCertainty, OperationKind, OperationModifiers};

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
    fn os_remove_literal_path_yields_filesystem_delete_known() {
        let op = one_op("os.remove(\"data.txt\")");
        assert_eq!(op.kind, OperationKind::FilesystemDelete);
        assert_eq!(op.certainty, OperandCertainty::Known);
        assert_eq!(op.modifiers, OperationModifiers::default());
        assert!(op.payload.is_none());
    }

    #[test]
    fn os_unlink_and_os_rmdir_are_filesystem_delete() {
        for src in ["os.unlink(\"a\")", "os.rmdir(\"d\")"] {
            let op = one_op(src);
            assert_eq!(op.kind, OperationKind::FilesystemDelete, "{src}");
            assert_eq!(op.modifiers, OperationModifiers::default(), "{src}");
        }
    }

    #[test]
    fn shutil_rmtree_is_recursive_filesystem_delete() {
        let op = one_op("shutil.rmtree(\"d\")");
        assert_eq!(op.kind, OperationKind::FilesystemDelete);
        assert!(op.modifiers.recursive, "shutil.rmtree must set recursive");
        assert!(!op.modifiers.forced);
    }

    #[test]
    fn os_remove_with_variable_path_is_dynamic() {
        let op = one_op("os.remove(path)");
        assert_eq!(op.kind, OperationKind::FilesystemDelete);
        assert_eq!(op.certainty, OperandCertainty::Dynamic);
        assert!(op.payload.is_none());
    }

    #[test]
    fn os_remove_with_concatenated_string_literal_is_known() {
        let op = one_op("os.remove(\"a\" \"b\")");
        assert_eq!(op.kind, OperationKind::FilesystemDelete);
        assert_eq!(op.certainty, OperandCertainty::Known);
    }

    #[test]
    fn os_remove_with_fstring_is_dynamic() {
        let op = one_op("os.remove(f\"{name}\")");
        assert_eq!(op.kind, OperationKind::FilesystemDelete);
        assert_eq!(
            op.certainty,
            OperandCertainty::Dynamic,
            "an f-string with interpolation is not a known literal"
        );
    }

    // --- Permission / ownership ---------------------------------------------

    #[test]
    fn os_chmod_chown_and_shutil_chown_are_permission_or_ownership() {
        for src in [
            "os.chmod(\"f\", 0o000)",
            "os.chown(\"f\", 0, 0)",
            "shutil.chown(\"f\")",
        ] {
            let op = one_op(src);
            assert_eq!(op.kind, OperationKind::PermissionOrOwnershipChange, "{src}");
        }
    }

    // --- open() overwrite ----------------------------------------------------

    #[test]
    fn open_write_mode_is_destructive_overwrite() {
        let op = one_op("open(\"f\", \"w\")");
        assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
        assert!(
            op.modifiers.destructive_mode,
            "'w' truncates and must set destructive_mode"
        );
        assert_eq!(op.certainty, OperandCertainty::Known, "literal path");
    }

    #[test]
    fn open_append_mode_is_overwrite_without_destructive_mode() {
        let op = one_op("open(\"f\", \"a\")");
        assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
        assert!(
            !op.modifiers.destructive_mode,
            "'a' appends without truncating"
        );
    }

    #[test]
    fn open_mode_keyword_argument_is_resolved() {
        let op = one_op("open(\"f\", mode=\"w\")");
        assert_eq!(op.kind, OperationKind::FilesystemOverwrite);
        assert!(op.modifiers.destructive_mode);
    }

    #[test]
    fn open_read_mode_is_not_destructive() {
        no_ops("open(\"f\", \"r\")");
    }

    #[test]
    fn open_without_mode_is_not_destructive() {
        no_ops("open(\"f\")");
    }

    #[test]
    fn open_exclusive_create_is_not_destructive() {
        no_ops("open(\"f\", \"x\")");
    }

    // --- Execution sinks: eval / exec (Python payload) ----------------------

    #[test]
    fn eval_literal_emits_code_execution_with_python_payload() {
        let op = one_op("eval(\"os.remove('x')\")");
        assert_eq!(op.kind, OperationKind::CodeExecution);
        assert_eq!(op.certainty, OperandCertainty::Known);
        let payload = op.payload.expect("literal payload must be recovered");
        assert_eq!(payload.language, SourceLanguage::Python);
        assert_eq!(payload.source, "os.remove('x')");
    }

    #[test]
    fn exec_literal_emits_code_execution_with_python_payload() {
        let op = one_op("exec(\"print(1)\")");
        assert_eq!(op.kind, OperationKind::CodeExecution);
        assert_eq!(op.certainty, OperandCertainty::Known);
        assert_eq!(
            op.payload.as_ref().unwrap().language,
            SourceLanguage::Python
        );
        assert_eq!(op.payload.as_ref().unwrap().source, "print(1)");
    }

    #[test]
    fn eval_dynamic_payload_is_code_execution_without_nested_target() {
        let op = one_op("eval(user_input)");
        assert_eq!(op.kind, OperationKind::CodeExecution);
        assert_eq!(op.certainty, OperandCertainty::Dynamic);
        assert!(op.payload.is_none(), "a dynamic payload is never evaluated");
    }

    // --- Execution sinks: os.system / subprocess (Bash payload) --------------

    #[test]
    fn os_system_literal_emits_code_execution_with_bash_payload() {
        let op = one_op("os.system(\"rm -rf /tmp/x\")");
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
    fn subprocess_run_with_string_arg_emits_bash_payload() {
        let op = one_op("subprocess.run(\"rm x\")");
        assert_eq!(op.kind, OperationKind::CodeExecution);
        assert_eq!(op.certainty, OperandCertainty::Known);
        assert_eq!(op.payload.as_ref().unwrap().language, SourceLanguage::Bash);
        assert_eq!(op.payload.as_ref().unwrap().source, "rm x");
    }

    #[test]
    fn subprocess_call_check_call_check_output_popen_are_code_execution() {
        for src in [
            "subprocess.call(\"rm x\")",
            "subprocess.check_call(\"rm x\")",
            "subprocess.check_output(\"rm x\")",
            "subprocess.Popen(\"rm x\")",
        ] {
            let op = one_op(src);
            assert_eq!(op.kind, OperationKind::CodeExecution, "{src}");
            assert_eq!(
                op.payload.as_ref().unwrap().language,
                SourceLanguage::Bash,
                "{src}"
            );
        }
    }

    #[test]
    fn subprocess_run_with_list_argv_is_dynamic_without_payload() {
        // A literal argv list is not shell source to recursively parse; the
        // visible sink still fires as CodeExecution, but no nested target is
        // recovered (the parent records DynamicSource degradation).
        let op = one_op("subprocess.run([\"ls\", \"-la\"])");
        assert_eq!(op.kind, OperationKind::CodeExecution);
        assert_eq!(op.certainty, OperandCertainty::Dynamic);
        assert!(op.payload.is_none());
    }

    #[test]
    fn subprocess_run_with_variable_arg_is_dynamic() {
        let op = one_op("subprocess.run(cmd)");
        assert_eq!(op.kind, OperationKind::CodeExecution);
        assert_eq!(op.certainty, OperandCertainty::Dynamic);
        assert!(op.payload.is_none());
    }

    // --- Negatives ----------------------------------------------------------

    #[test]
    fn comment_mentioning_os_remove_is_not_an_operation() {
        no_ops("# os.remove(\"x\")");
    }

    #[test]
    fn string_literal_mentioning_os_remove_is_not_an_operation() {
        no_ops("\"os.remove('x')\"");
    }

    #[test]
    fn attribute_reference_without_call_is_not_an_operation() {
        no_ops("f = os.remove");
    }

    #[test]
    fn unrelated_call_is_not_an_operation() {
        no_ops("print(\"hello\")");
    }

    // --- Composition / ordering / spans -------------------------------------

    #[test]
    fn multiple_operations_are_emitted_in_source_order() {
        let result = analyze("os.remove(\"a\")\nos.system(\"rm b\")\n");
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
        // subprocess.run(eval("x")): the outer sink takes a non-literal (the
        // eval call), so it is Dynamic; the inner eval has a literal payload.
        let result = analyze("subprocess.run(eval(\"x\"))");
        assert_eq!(result.operations.len(), 2);
        let kinds: Vec<_> = result.operations.iter().map(|o| o.kind).collect();
        assert_eq!(
            kinds,
            vec![OperationKind::CodeExecution, OperationKind::CodeExecution]
        );
        // The inner eval recovered a literal; the outer subprocess.run did not.
        assert!(
            result.operations.iter().any(|o| o
                .payload
                .as_ref()
                .is_some_and(|p| p.language == SourceLanguage::Python && p.source == "x")),
            "the inner eval payload must be recovered"
        );
        assert!(
            result
                .operations
                .iter()
                .any(|o| o.certainty == OperandCertainty::Dynamic),
            "the outer subprocess.run must be dynamic"
        );
    }

    #[test]
    fn operation_span_covers_the_call() {
        let src = "os.remove(\"data.txt\")";
        let op = one_op(src);
        assert_eq!(op.span.byte_start, 0);
        assert_eq!(op.span.byte_end, src.len());
        assert_eq!(op.span.line, 1);
        assert_eq!(op.span.column, 1);
    }

    // --- Malformed source ---------------------------------------------------

    #[test]
    fn malformed_source_records_parse_errors() {
        let result = analyze("os.remove(");
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
}
