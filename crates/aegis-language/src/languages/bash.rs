//! Bash language adapter (plan Iteration 8, Slice 1).
//!
//! Structural capture via the bundled Tree-sitter query
//! (`queries/bash/calls.scm`); semantic interpretation in typed Rust. The
//! adapter emits language-neutral [`crate::operation::DetectedOperation`]s and,
//! for execution sinks with a statically recovered literal payload, a
//! [`crate::operation::NestedTarget`] for bounded recursive analysis (ADR-022
//! §3, §7). It never assigns a final `RiskLevel` — the root crate maps these
//! operations through the shared classifier (Iteration 5 REVIEW GATE).
//!
//! Unlike the Python / JavaScript / TypeScript adapters, Bash is the *outer*
//! shell language Aegis already classifies with the shell Scanner. The Bash
//! adapter analyzes command-visible *nested* shell source (the body of
//! `bash -c "…"`, `sh -c "…"`, `eval "…"`, `source`, heredoc bodies fed back by
//! the router) and emits operations for what it finds. Deduplication of
//! semantically identical evidence with the outer Scanner Matches is a
//! merge-layer concern (plan Iteration 8: "outer Scanner Matches remain even
//! when the Bash adapter reports a richer duplicate operation" + "deduplicate
//! semantically identical evidence while retaining mechanism provenance"), not
//! the adapter's job.
//!
//! Slice 1 scope: fully-qualified destructive commands (`rm`, `rmdir`,
//! `unlink`, `chmod`, `chown`, `chgrp`), truncating/append file redirects
//! (`>`, `>>`, `&>`, `&>>`, `>|`), the `tee` write command, and execution sinks
//! (`eval`, `source`/`.`, `bash`/`sh`/… `-c`, `python* -c`, `node -e`) with
//! literal or dynamic operand certainty. Bounded symbol resolution, the
//! `exec`/`command` builtins, multi-argument `eval` joining, `--` option
//! termination, command prefixes (`sudo`/`nohup`/`nice`/`time`/`command` —
//! already stripped by `router::source_targets` for the outer command but not
//! yet for nested command names inside a payload, so `sudo rm` inside `bash -c`
//! is not yet recognized), and unsupported interpreters (`perl`/`ruby`/`php`/…)
//! are later slices.

use std::cell::RefCell;
use std::sync::LazyLock;

use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator};

use crate::language::SourceLanguage;
use crate::operation::{
    AdapterResult, ByteSpan, DetectedOperation, NestedTarget, OperandCertainty, OperationKind,
    OperationModifiers,
};

/// The bundled Bash call-capture query. Compiled once on first use; a failure
/// here is a build-time query-authoring bug, so panicking on first use is the
/// correct startup behavior (CLAUDE.md: `.expect()` is acceptable in startup
/// initialization).
static CALLS_QUERY: LazyLock<Query> = LazyLock::new(|| {
    Query::new(
        &SourceLanguage::Bash.tree_sitter_language(),
        include_str!("../../queries/bash/calls.scm"),
    )
    .expect("bundled bash/calls.scm must compile against the pinned grammar")
});

// A per-thread reusable Bash parser.
//
// `set_language` runs once per thread on first use (one-time initialization,
// not on every `analyze` call) — a failure here is a build-time grammar-pin
// bug, not user input, so panicking on init is the correct startup behavior
// (CLAUDE.md: `.expect()` is acceptable in startup initialization). Keeping the
// parser out of `analyze`'s body avoids re-running `set_language` on each call
// and avoids `.expect()` on a per-invocation path. `thread_local!` gives each
// thread its own parser with no locking; `analyze` is non-reentrant, so the
// `borrow_mut` cannot clash.
thread_local! {
    static PARSER: RefCell<Parser> = RefCell::new({
        let mut parser = Parser::new();
        parser
            .set_language(&SourceLanguage::Bash.tree_sitter_language())
            .expect("pinned bash grammar is ABI-compatible with the runtime");
        parser
    });
}

/// Analyze Bash `source` for destructive effects and execution sinks.
///
/// Pure and in-process: parses `source` with the pinned Bash grammar, runs the
/// call-capture query, and interprets each command and redirect in Rust. No
/// filesystem access, no subprocess (ADR-022 §2). A nonzero `parse_errors` means
/// the source was malformed; the root mapping records
/// `DegradationReason::IncompleteSyntax`.
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
    let operations = collect_operations(root, bytes, &CALLS_QUERY);

    AdapterResult {
        operations,
        parse_errors,
    }
}

/// Run the call-capture `query` over `root` and interpret each match into a
/// [`DetectedOperation`], returned in source (document) order.
///
/// Each query match is either a `command` (carries `@cmd` + `@name`) or a
/// `file_redirect` (carries `@redirect`); the two are dispatched to
/// [`interpret_command`] and [`interpret_redirect`] respectively. Commands and
/// redirects nested inside command substitution `$(…)`, subshells, compound
/// statements, and `list`/`pipeline` tails are matched recursively, so nested
/// destructive commands and writes surface as separate operations (the
/// foundation of recursive analysis, ADR-022 §7).
fn collect_operations(root: Node, bytes: &[u8], query: &Query) -> Vec<DetectedOperation> {
    let mut operations = Vec::new();
    let mut cursor = QueryCursor::new();
    let capture_names = query.capture_names();
    let mut matches = cursor.matches(query, root, bytes);
    while let Some(m) = matches.next() {
        // Resolve this match's captures by name. A `command` match carries
        // `@cmd` + `@name`; a `file_redirect` match carries `@redirect` only.
        let mut cmd = None;
        let mut name = None;
        let mut redirect = None;
        for cap in m.captures {
            match capture_names[cap.index as usize] {
                "cmd" => cmd = Some(cap.node),
                "name" => name = Some(cap.node),
                "redirect" => redirect = Some(cap.node),
                _ => {}
            }
        }
        let op = match (cmd, name) {
            (Some(cmd_node), Some(name_node)) => interpret_command(name_node, cmd_node, bytes),
            _ => redirect.and_then(interpret_redirect),
        };
        if let Some(op) = op {
            operations.push(op);
        }
    }

    // Tree-sitter yields matches in capture order; sort by source position so
    // callers see operations in document order regardless of query internals.
    operations.sort_by_key(|op| (op.span.byte_start, op.span.byte_end));
    operations
}

/// How a recognized command is classified, before operand certainty is
/// attached. Routes a command name to the shared operation vocabulary without
/// assigning `RiskLevel` (Iteration 5 REVIEW GATE).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandClass {
    /// `rm` — filesystem deletion; recursive/forced modifiers come from flags.
    Rm,
    /// `rmdir` / `unlink` — plain filesystem deletion (no modifiers).
    Rmdir,
    Unlink,
    /// `chmod` / `chown` / `chgrp` — a permission or ownership change.
    Chmod,
    Chown,
    Chgrp,
    /// `tee` — a file write; truncating by default, appending with `-a`.
    Tee,
    /// `eval` — an execution sink whose literal payload is Bash source.
    Eval,
    /// `source` / `.` — an execution sink whose operand is a path (the file's
    /// contents are not in the source, so no inline payload is recovered).
    Source,
    /// `bash` / `sh` / … — an execution sink whose `-c` payload is Bash source.
    ShellExec,
    /// `python* -c` / `node -e` — an execution sink whose payload is the
    /// interpreter's own language (cross-language, ADR-022 §7).
    InterpExec(SourceLanguage),
}

/// Classify a command name into a [`CommandClass`], or `None` when the command
/// is not a tracked destructive or execution-sink operation.
fn classify_command(name: &str) -> Option<CommandClass> {
    Some(match name {
        "rm" => CommandClass::Rm,
        "rmdir" => CommandClass::Rmdir,
        "unlink" => CommandClass::Unlink,
        "chmod" => CommandClass::Chmod,
        "chown" => CommandClass::Chown,
        "chgrp" => CommandClass::Chgrp,
        "tee" => CommandClass::Tee,
        "eval" => CommandClass::Eval,
        "source" | "." => CommandClass::Source,
        "bash" | "sh" | "dash" | "ash" | "zsh" | "ksh" | "mksh" => CommandClass::ShellExec,
        _ if is_python(name) => CommandClass::InterpExec(SourceLanguage::Python),
        _ if is_node(name) => CommandClass::InterpExec(SourceLanguage::JavaScript),
        _ => return None,
    })
}

/// Whether `name` is a Python interpreter (`python`, `python2`, `python3`, and
/// versioned `python3.11` / `python2.7`). Bash-local: `aegis-language` may not
/// depend on the root crate's router, so this duplicates the router's
/// interpreter-name recognition rather than reusing it — the same
/// boundary-forced duplication shape as the operation vocabulary (ADR-022 §4).
/// The router (`router::INTERPRETERS`) recognizes only the exact `python` /
/// `python3` basenames; this registry is intentionally *broader* (it also
/// accepts `python2` and `N.M`-versioned forms), so the two are not kept in
/// lockstep — a future change to one must be mirrored by hand in the other.
fn is_python(name: &str) -> bool {
    if name == "python" {
        return true;
    }
    // After `python`, accept `2`, `3`, `2.7`, `3.11` — digits and dots only —
    // so `python3foo` is not misread as an interpreter. Symmetric with
    // [`is_node`]'s trailing-digit guard.
    let Some(rest) = name.strip_prefix("python") else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// Whether `name` is a Node interpreter (`node`, `nodejs`, and versioned
/// `node20`). See [`is_python`] for the boundary-forced local-registry note
/// and the not-in-lockstep caveat versus the router.
fn is_node(name: &str) -> bool {
    name == "node"
        || name == "nodejs"
        || (name.starts_with("node") && name[4..].chars().all(|c| c.is_ascii_digit()))
}

/// Build a [`DetectedOperation`] for one classified command, attaching operand
/// certainty and (for execution sinks) a nested literal payload.
fn interpret_command(name_node: Node, cmd_node: Node, bytes: &[u8]) -> Option<DetectedOperation> {
    let name = name_node.utf8_text(bytes).ok()?;
    let class = classify_command(name)?;
    let args = command_args(cmd_node);
    let span = span_for(cmd_node);
    match class {
        CommandClass::Rm => {
            let modifiers = rm_modifiers(&args, bytes);
            let certainty = operand_certainty(&args);
            Some(DetectedOperation {
                kind: OperationKind::FilesystemDelete,
                modifiers,
                certainty,
                span,
                payload: None,
            })
        }
        CommandClass::Rmdir | CommandClass::Unlink => Some(DetectedOperation {
            kind: OperationKind::FilesystemDelete,
            modifiers: OperationModifiers::default(),
            certainty: operand_certainty(&args),
            span,
            payload: None,
        }),
        CommandClass::Chmod | CommandClass::Chown | CommandClass::Chgrp => {
            Some(DetectedOperation {
                kind: OperationKind::PermissionOrOwnershipChange,
                modifiers: OperationModifiers::default(),
                certainty: operand_certainty(&args),
                span,
                payload: None,
            })
        }
        CommandClass::Tee => Some(DetectedOperation {
            kind: OperationKind::FilesystemOverwrite,
            modifiers: OperationModifiers {
                // `tee` truncates by default and appends with `-a` / `--append`.
                destructive_mode: !(has_short_flag(&args, 'a', bytes)
                    || has_long_flag(&args, "append", bytes)),
                ..OperationModifiers::default()
            },
            certainty: operand_certainty(&args),
            span,
            payload: None,
        }),
        CommandClass::Eval => {
            // `eval` joins *all* its arguments with spaces and evaluates the
            // result, so a literal first argument alongside a dynamic one
            // (`eval "rm" "$x"`) evaluates a dynamic string. Recovering a
            // `Known` payload from only the first argument would hide the
            // dynamic tail — the unsafe direction ADR-022 §3/§7 guards
            // against. Multi-argument joining is a later slice; until then,
            // only a single literal argument is safe to recover, and every
            // other shape degrades to `Dynamic` with no nested target.
            let payload = (args.len() == 1)
                .then(|| args.first())
                .flatten()
                .and_then(|&n| literal_payload(n, bytes, SourceLanguage::Bash));
            Some(exec_op(payload, span))
        }
        CommandClass::Source => Some(DetectedOperation {
            kind: OperationKind::CodeExecution,
            modifiers: OperationModifiers::default(),
            // The file's contents are not in the source, so no inline nested
            // target is recovered (a ScriptFile recursive target is the
            // orchestration layer's job, like Python `subprocess` on a script).
            certainty: OperandCertainty::Dynamic,
            span,
            payload: None,
        }),
        CommandClass::ShellExec => {
            let payload = arg_after_flag(&args, &["-c"], bytes)
                .and_then(|n| literal_payload(n, bytes, SourceLanguage::Bash));
            Some(exec_op(payload, span))
        }
        CommandClass::InterpExec(lang) => {
            let flags: &[&str] = if matches!(lang, SourceLanguage::JavaScript) {
                &["-e", "--eval"]
            } else {
                &["-c"]
            };
            let payload =
                arg_after_flag(&args, flags, bytes).and_then(|n| literal_payload(n, bytes, lang));
            Some(exec_op(payload, span))
        }
    }
}

/// Build a `CodeExecution` [`DetectedOperation`] for an execution sink, with
/// certainty `Known` iff a literal payload was recovered (ADR-022 §3/§7).
fn exec_op(payload: Option<NestedTarget>, span: ByteSpan) -> DetectedOperation {
    let certainty = if payload.is_some() {
        OperandCertainty::Known
    } else {
        OperandCertainty::Dynamic
    };
    DetectedOperation {
        kind: OperationKind::CodeExecution,
        modifiers: OperationModifiers::default(),
        certainty,
        span,
        payload,
    }
}

/// Build a [`DetectedOperation`] for one `file_redirect`, classifying its
/// operator. Truncating redirects (`>`, `>|`, `&>`) and append redirects (`>>`,
/// `&>>`) emit a `FilesystemOverwrite` (truncating ⇒ `destructive_mode`);
/// input redirects (`<`), file-descriptor duplicates (`>&`, `<&`), and closes
/// (`>&-`, `<&-`) are not file writes and emit nothing.
fn interpret_redirect(node: Node) -> Option<DetectedOperation> {
    let destructive_mode = match redirect_operator(node)? {
        ">" | ">|" | "&>" => true,
        ">>" | "&>>" => false,
        // `<`, `<&`, `>&`, `<&-`, `>&-` — input / fd-dup / close: not a write.
        _ => return None,
    };
    let destination = node.child_by_field_name("destination")?;
    let certainty = if has_expansion(destination) {
        OperandCertainty::Dynamic
    } else {
        OperandCertainty::Known
    };
    Some(DetectedOperation {
        kind: OperationKind::FilesystemOverwrite,
        modifiers: OperationModifiers {
            destructive_mode,
            ..OperationModifiers::default()
        },
        certainty,
        span: span_for(node),
        payload: None,
    })
}

/// The anonymous operator token of a `file_redirect` (`>`, `>>`, `<`, …), read
/// by walking the node's children for the first whose kind is a known operator.
/// The operator is an anonymous token, so its node kind is the literal text.
fn redirect_operator(node: Node) -> Option<&'static str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            ">" | ">>" | ">|" | "&>" | "&>>" | "<" | "<&" | ">&" | "<&-" | ">&-" => {
                return Some(child.kind());
            }
            _ => {}
        }
    }
    None
}

/// The `argument`-field children of a `command` node (the positional
/// arguments), in source order. Variable assignments, redirects, and the
/// command name live in other fields and are excluded — only `argument`-field
/// literals are returned.
fn command_args(cmd_node: Node) -> Vec<Node> {
    let mut cursor = cmd_node.walk();
    cmd_node
        .children_by_field_name("argument", &mut cursor)
        .collect()
}

/// Resolve `rm` recursive/forced modifiers from flag arguments: long flags
/// `--recursive` / `--force`, and short flags (possibly combined, e.g. `-rf`).
/// `--` option termination is a later slice; a path argument beginning with `-`
/// after `--` would be misread here, which is the safe over-count (a
/// destructive delete is still flagged).
fn rm_modifiers(args: &[Node], bytes: &[u8]) -> OperationModifiers {
    let mut mods = OperationModifiers::default();
    for arg in args {
        let Some(text) = arg.utf8_text(bytes).ok() else {
            continue;
        };
        if text == "--recursive" {
            mods.recursive = true;
            continue;
        }
        if text == "--force" {
            mods.forced = true;
            continue;
        }
        if let Some(rest) = text.strip_prefix('-') {
            // A lone `-` or a `--` long flag we did not match above is skipped.
            if rest.is_empty() || rest.starts_with('-') {
                continue;
            }
            for c in rest.chars() {
                match c {
                    'r' | 'R' => mods.recursive = true,
                    'f' => mods.forced = true,
                    _ => {}
                }
            }
        }
    }
    mods
}

/// Whether any argument carries a short flag containing `flag` (e.g. `tee -a`).
fn has_short_flag(args: &[Node], flag: char, bytes: &[u8]) -> bool {
    for arg in args {
        let Ok(text) = arg.utf8_text(bytes) else {
            continue;
        };
        let Some(rest) = text.strip_prefix('-') else {
            continue;
        };
        if !rest.is_empty() && !rest.starts_with('-') && rest.contains(flag) {
            return true;
        }
    }
    false
}

/// Whether any argument is the long flag `--{flag}` (e.g. `tee --append`).
fn has_long_flag(args: &[Node], flag: &str, bytes: &[u8]) -> bool {
    let needle = format!("--{flag}");
    args.iter().any(|arg| {
        arg.utf8_text(bytes)
            .map(|text| text == needle.as_str())
            .unwrap_or(false)
    })
}

/// The argument immediately following the first argument whose text equals one
/// of `flags` (e.g. the payload after `bash -c` / `node -e`), or `None`.
fn arg_after_flag<'a>(args: &[Node<'a>], flags: &[&str], bytes: &[u8]) -> Option<Node<'a>> {
    for (i, arg) in args.iter().enumerate() {
        let Ok(cow) = arg.utf8_text(bytes) else {
            continue;
        };
        let text: &str = cow;
        if flags.contains(&text) {
            return args.get(i + 1).copied();
        }
    }
    None
}

/// Operand certainty for a non-execution command: `Dynamic` if any argument
/// carries an expansion/substitution (a variable, `$(…)`, `$((…))`, process
/// substitution, or an expansion inside a string); otherwise `Known`. A
/// computed operand is never evidence of safety (ADR-022 §3, §7). The
/// "any expansion ⇒ Dynamic" rule is a safe over-approximation: it never marks
/// a dynamic operand as `Known`.
fn operand_certainty(args: &[Node]) -> OperandCertainty {
    if args.iter().any(|a| has_expansion(*a)) {
        OperandCertainty::Dynamic
    } else {
        OperandCertainty::Known
    }
}

/// Whether `node` is or contains any shell expansion or substitution
/// (`simple_expansion`, `expansion`, `command_substitution`,
/// `process_substitution`, `arithmetic_expansion`). Structural — does not
/// evaluate the expansion.
fn has_expansion(node: Node) -> bool {
    match node.kind() {
        "simple_expansion"
        | "expansion"
        | "command_substitution"
        | "process_substitution"
        | "arithmetic_expansion" => return true,
        _ => {}
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor).any(has_expansion)
}

/// Recover a literal execution-sink payload from `node` as a [`NestedTarget`]
/// parsed as `lang`, or `None` when the node carries an expansion (a dynamic
/// payload is never evaluated, ADR-022 §7) or is not a string literal.
fn literal_payload(node: Node, bytes: &[u8], lang: SourceLanguage) -> Option<NestedTarget> {
    if has_expansion(node) {
        return None;
    }
    let source = literal_content(node, bytes)?;
    Some(NestedTarget {
        language: lang,
        source,
        span: span_for(node),
    })
}

/// The inner text of a string-literal node, or `None` when the node is not a
/// literal form the adapter recovers (`raw_string`, `string`, `ansi_c_string`,
/// `word`). Escape decoding is deferred to a later slice; the raw inner text is
/// returned with escapes as written (consistent with the JS-family adapters).
fn literal_content(node: Node, bytes: &[u8]) -> Option<String> {
    let text = node.utf8_text(bytes).ok()?;
    match node.kind() {
        "raw_string" | "string" => {
            let end = text.len().checked_sub(1)?;
            Some(text.get(1..end)?.to_string())
        }
        "ansi_c_string" => {
            // `$'…'` — strip the leading `$'` and the trailing `'`.
            let end = text.len().checked_sub(1)?;
            Some(text.get(2..end)?.to_string())
        }
        "word" => Some(text.to_string()),
        _ => None,
    }
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

// The unit tests live in a sibling file to keep this adapter under the
// 800-line file-size budget (`tests/file_size_budget.rs`); the same `#[path]`
// split is used by `typescript.rs` → `typescript_tests.rs`.
#[cfg(test)]
#[path = "bash_tests.rs"]
mod tests;
