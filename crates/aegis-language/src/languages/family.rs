//! Shared JavaScript-family classification logic (plan Iteration 7).
//!
//! JavaScript and TypeScript share the same destructive / execution-sink API
//! surface (`fs.*Sync`, `eval`, `new Function`, `child_process.*`) and the same
//! Tree-sitter node-type names for call sites and string literals, so the
//! grammar-agnostic interpretation — API-spelling → [`crate::operation::
//! DetectedOperation`], operand certainty, recursive-option resolution, nested
//! payload recovery — lives here once. Each adapter (`javascript`,
//! `typescript`) owns its own pinned grammar, per-thread parser, and
//! `calls.scm` query, then feeds the parsed root into [`collect_operations`].
//!
//! This is per-adapter AST interpretation, NOT the shared operation classifier:
//! it never assigns a final `RiskLevel` (the root crate's `aegis_types::classify`
//! does that — Iteration 5 REVIEW GATE). Bounded symbol resolution (imports,
//! aliases, simple constants → `OperandCertainty::Partial`) is a later slice;
//! every computed operand is `Dynamic` here (ADR-022 §3, §7).

use tree_sitter::{Node, Query, QueryCursor, StreamingIterator};

use crate::language::SourceLanguage;
use crate::operation::{
    ByteSpan, DetectedOperation, NestedTarget, OperandCertainty, OperationKind, OperationModifiers,
};

/// The payload language an execution sink's literal source should be recursively
/// parsed as (ADR-022 §7 cross-language nesting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecLang {
    /// `eval` / `new Function(...)` payloads are JavaScript source.
    Js,
    /// `child_process.exec` / `execSync` string payloads are shell source (Bash).
    Bash,
}

/// Which argument of an execution sink carries the literal source payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecArg {
    /// The first positional argument (`eval(code)`, `exec(cmd)`).
    First,
    /// The last positional argument (`new Function("a", "body")` — the body is
    /// the final string argument).
    Last,
}

/// How a recognized call site is classified, before operand certainty is
/// attached. Routes a function path to the shared operation vocabulary without
/// assigning `RiskLevel` (Iteration 5 REVIEW GATE).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallClass {
    /// A non-execution destructive operation with fixed modifiers.
    Op(OperationKind, OperationModifiers),
    /// `fs.rmSync(...)` — filesystem deletion whose `recursive` modifier is
    /// resolved from the options-object argument (`{recursive: true}`).
    RmSync,
    /// A recognized execution sink whose literal payload is recursively
    /// analyzable in `ExecLang`, taken from the `ExecArg` position.
    Exec(ExecLang, ExecArg),
    /// A program-name + argv execution sink (`child_process.spawn`,
    /// `execFile`, `fork`, …). The first argument is a program name, not shell
    /// source to recurse into, so the sink fires as `CodeExecution` with
    /// `Dynamic` certainty and no nested target (mirrors Python's
    /// `subprocess.run([argv])` argv form).
    ExecArgv,
}

/// Classify a fully-qualified call path into a [`CallClass`], or `None` when the
/// call is not a tracked destructive or execution-sink API.
pub(crate) fn classify_path(path: &str) -> Option<CallClass> {
    Some(match path {
        "fs.unlinkSync" | "fs.rmdirSync" => CallClass::Op(
            OperationKind::FilesystemDelete,
            OperationModifiers::default(),
        ),
        "fs.rmSync" => CallClass::RmSync,
        "fs.writeFileSync" => CallClass::Op(
            OperationKind::FilesystemOverwrite,
            OperationModifiers {
                destructive_mode: true,
                ..OperationModifiers::default()
            },
        ),
        "fs.appendFileSync" => CallClass::Op(
            OperationKind::FilesystemOverwrite,
            OperationModifiers::default(),
        ),
        "fs.chmodSync" | "fs.chownSync" => CallClass::Op(
            OperationKind::PermissionOrOwnershipChange,
            OperationModifiers::default(),
        ),
        "eval" => CallClass::Exec(ExecLang::Js, ExecArg::First),
        "Function" => CallClass::Exec(ExecLang::Js, ExecArg::Last),
        "child_process.exec" | "child_process.execSync" => {
            CallClass::Exec(ExecLang::Bash, ExecArg::First)
        }
        "child_process.spawn"
        | "child_process.spawnSync"
        | "child_process.execFile"
        | "child_process.execFileSync"
        | "child_process.fork" => CallClass::ExecArgv,
        _ => return None,
    })
}

/// Run the call-capture `query` over `root` and interpret each match into a
/// [`DetectedOperation`], returned in source (document) order. Grammar-agnostic:
/// both the JavaScript and TypeScript adapters call this with their own parsed
/// root and compiled query.
pub(crate) fn collect_operations(
    root: Node,
    bytes: &[u8],
    query: &Query,
) -> Vec<DetectedOperation> {
    let mut operations = Vec::new();
    let mut cursor = QueryCursor::new();
    let capture_names = query.capture_names();
    let mut matches = cursor.matches(query, root, bytes);
    while let Some(m) = matches.next() {
        // Resolve this match's captures by name. A match carries @call, @args,
        // and either (@obj, @attr) for a member call or @fname for a bare-
        // identifier call / new-expression constructor.
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
    operations
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
        CallClass::RmSync => {
            let operand = first_positional_arg(args_node);
            let certainty = operand_certainty(operand);
            let recursive = recursive_option(args_node, bytes);
            Some(DetectedOperation {
                kind: OperationKind::FilesystemDelete,
                modifiers: OperationModifiers {
                    recursive,
                    ..OperationModifiers::default()
                },
                certainty,
                span,
                payload: None,
            })
        }
        CallClass::Exec(lang, arg_pos) => {
            let operand = match arg_pos {
                ExecArg::First => first_positional_arg(args_node),
                ExecArg::Last => last_positional_arg(args_node),
            };
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
        CallClass::ExecArgv => Some(DetectedOperation {
            kind: OperationKind::CodeExecution,
            modifiers: OperationModifiers::default(),
            // A program-name + argv form is not shell source to recurse into;
            // the visible sink still fires as CodeExecution (ADR-022 §3/§7).
            certainty: OperandCertainty::Dynamic,
            span,
            payload: None,
        }),
    }
}

/// Map an [`ExecLang`] to the [`SourceLanguage`] its payload is parsed as.
fn exec_language(lang: ExecLang) -> SourceLanguage {
    match lang {
        ExecLang::Js => SourceLanguage::JavaScript,
        ExecLang::Bash => SourceLanguage::Bash,
    }
}

/// The first positional argument of an `arguments` node, or `None` when the
/// call has no arguments. JavaScript has no keyword arguments, so the first named
/// child is the first positional argument.
fn first_positional_arg(args_node: Node) -> Option<Node> {
    let mut cursor = args_node.walk();
    args_node.named_children(&mut cursor).next()
}

/// The last positional argument of an `arguments` node, or `None` when the call
/// has no arguments. Used for the `Function` constructor, whose final string
/// argument is the function body.
fn last_positional_arg(args_node: Node) -> Option<Node> {
    let mut cursor = args_node.walk();
    args_node.named_children(&mut cursor).last()
}

/// Operand certainty for a non-execution operand: `Known` iff it is a pure
/// string literal (a `string`, or a `template_string` with no interpolation);
/// otherwise `Dynamic`. A computed/imported operand is never evidence of
/// safety (ADR-022 §3, §7).
fn operand_certainty(arg: Option<Node>) -> OperandCertainty {
    match arg {
        Some(n) if is_string_literal(n) => OperandCertainty::Known,
        _ => OperandCertainty::Dynamic,
    }
}

/// Whether `node` is a pure string literal: a `string` (always literal — escape
/// sequences do not make it computed), or a `template_string` with no
/// `template_substitution` child. Structural — does not decode the content.
fn is_string_literal(node: Node) -> bool {
    match node.kind() {
        "string" => true,
        "template_string" => {
            let mut cursor = node.walk();
            !node
                .named_children(&mut cursor)
                .any(|c| c.kind() == "template_substitution")
        }
        _ => false,
    }
}

/// The text content of a pure string-literal node, or `None` when the node is
/// not a literal (a variable, a template with interpolation, a non-string
/// child, etc.).
///
/// For a `string`, the content is the raw inner source text — the concatenated
/// text of its `string_fragment` and `escape_sequence` children (the text
/// between the surrounding quotes, with escapes as written). Escape decoding is
/// bounded-resolution work and is deferred to a later slice. For a
/// `template_string` with no interpolation, the content is the concatenated
/// `string_fragment` / `escape_sequence` text (the text between the backticks).
fn string_literal_content(node: Node, bytes: &[u8]) -> Option<String> {
    match node.kind() {
        "string" => {
            let mut cursor = node.walk();
            let mut content = String::new();
            for child in node.named_children(&mut cursor) {
                match child.kind() {
                    "string_fragment" | "escape_sequence" => {
                        content.push_str(child.utf8_text(bytes).ok()?);
                    }
                    _ => return None,
                }
            }
            Some(content)
        }
        "template_string" => {
            let mut cursor = node.walk();
            let mut content = String::new();
            for child in node.named_children(&mut cursor) {
                match child.kind() {
                    "string_fragment" | "escape_sequence" => {
                        content.push_str(child.utf8_text(bytes).ok()?);
                    }
                    "template_substitution" => return None,
                    _ => return None,
                }
            }
            Some(content)
        }
        _ => None,
    }
}

/// Whether an `arguments` node contains an options object with a `recursive:
/// true` pair, as used by `fs.rmSync(path, {recursive: true})`. Structural —
/// inspects only the literal shape (identifier-keyed `recursive: true` or
/// string-keyed `"recursive": true`); a computed options object or an
/// escaped/aliased key is not recognized (the modifier stays false, which is
/// the safe under-count: a non-recursive classification is still a destructive
/// delete, just not flagged as recursive).
fn recursive_option(args_node: Node, bytes: &[u8]) -> bool {
    let mut cursor = args_node.walk();
    for arg in args_node.named_children(&mut cursor) {
        if arg.kind() != "object" {
            continue;
        }
        let mut pair_cursor = arg.walk();
        for pair in arg.named_children(&mut pair_cursor) {
            if pair.kind() != "pair" {
                continue;
            }
            let Some(key) = pair.child_by_field_name("key") else {
                continue;
            };
            // Accept both identifier-keyed (`recursive: true`) and string-keyed
            // (`"recursive": true`) literal shapes — the latter's key node is a
            // `string` whose raw text is `"recursive"` (with quotes), so compare
            // its decoded content rather than the raw text.
            let key_is_recursive = key.utf8_text(bytes).ok() == Some("recursive")
                || (key.kind() == "string"
                    && string_literal_content(key, bytes).as_deref() == Some("recursive"));
            if !key_is_recursive {
                continue;
            }
            let Some(value) = pair.child_by_field_name("value") else {
                continue;
            };
            if value.kind() == "true" {
                return true;
            }
        }
    }
    false
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
