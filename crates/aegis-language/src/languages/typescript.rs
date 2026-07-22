//! TypeScript language adapter (plan Iteration 7, Slice 1).
//!
//! Structural capture via the bundled Tree-sitter query
//! (`queries/typescript/calls.scm`); semantic interpretation in typed Rust. The
//! adapter emits language-neutral [`crate::operation::DetectedOperation`]s and,
//! for execution sinks with a statically recovered literal payload, a
//! [`crate::operation::NestedTarget`] for bounded recursive analysis (ADR-022
//! §3, §7). It never assigns a final `RiskLevel` — the root crate maps these
//! operations through the shared classifier (Iteration 5 REVIEW GATE).
//!
//! TypeScript is a syntactic superset of JavaScript and reuses the JavaScript
//! node types for call sites and string literals, so the grammar-agnostic
//! interpretation (API-spelling → operation, operand certainty, recursive-option
//! resolution, nested payload recovery, the call-capture query loop) is shared
//! via the [`super::family`] module (plan Iteration 7: "share JavaScript-family
//! resolution where syntax permits"). This module owns only the TypeScript
//! grammar, per-thread parser, and compiled query — the grammar and span
//! handling stay explicit per adapter (plan Iteration 7 GREEN).
//!
//! Slice 1 scope: the JavaScript-family destructive / execution-sink API
//! surface (`fs.*Sync`, `eval`, `new Function`, `child_process.*`) over
//! TypeScript source, including calls with explicit type arguments and
//! destructive calls inside generic class methods. Type-only syntax
//! (interfaces, enums, type aliases, `import type`, `as`, `satisfies`,
//! decorators) surfaces no operation. Bounded symbol resolution (imports,
//! aliases, simple constants → `OperandCertainty::Partial`) is a later slice.

use std::cell::RefCell;
use std::sync::LazyLock;

use tree_sitter::{Parser, Query};

use crate::language::SourceLanguage;
use crate::languages::family;
use crate::operation::AdapterResult;

/// The bundled TypeScript call-capture query. Compiled once on first use; a
/// failure here is a build-time query-authoring bug, so panicking on first use
/// is the correct startup behavior (CLAUDE.md: `.expect()` is acceptable in
/// startup initialization).
static CALLS_QUERY: LazyLock<Query> = LazyLock::new(|| {
    Query::new(
        &SourceLanguage::TypeScript.tree_sitter_language(),
        include_str!("../../queries/typescript/calls.scm"),
    )
    .expect("bundled typescript/calls.scm must compile against the pinned grammar")
});

// A per-thread reusable TypeScript parser.
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
            .set_language(&SourceLanguage::TypeScript.tree_sitter_language())
            .expect("pinned typescript grammar is ABI-compatible with the runtime");
        parser
    });
}

/// Analyze TypeScript `source` for destructive effects and execution sinks.
///
/// Pure and in-process: parses `source` with the pinned TypeScript grammar,
/// runs the call-capture query, and interprets each call site in Rust via the
/// shared `family` module. No filesystem access, no subprocess (ADR-022 §2). A
/// nonzero `parse_errors` means the source was malformed; the root mapping
/// records `DegradationReason::IncompleteSyntax`.
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
    let operations = family::collect_operations(root, bytes, &CALLS_QUERY);

    AdapterResult {
        operations,
        parse_errors,
    }
}

// The unit tests live in a sibling file to keep this adapter under the
// 800-line file-size budget (`tests/file_size_budget.rs`); the same `#[path]`
// split is used by `javascript.rs` → `javascript_tests.rs`.
#[cfg(test)]
#[path = "typescript_tests.rs"]
mod tests;
