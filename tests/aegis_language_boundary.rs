//! aegis-language architectural boundary tests (ADR-022).
//!
//! These pin the two `aegis-language` dependency invariants declared in
//! `crates/aegis-language/src/lib.rs` and ADR-022 §4, so a future regression
//! is caught at `cargo test` time rather than only in review:
//!
//! 1. **Downstream isolation** — no other workspace member may depend on
//!    `aegis-language`. It is an additive slow path and must not leak into the
//!    safe-command hot path or the type layer; `aegis-types` is the named
//!    ADR-022 §4 review gate.
//! 2. **Leaf self-containment** — `aegis-language` must not depend on any
//!    other workspace member. Its only dependencies are the pinned Tree-sitter
//!    runtime, the four qualified grammars, and `thiserror` (ADR-022 §8).
//!
//! The `assert_no_dep` helper is shared with `tests/architecture_boundaries.rs`
//! via `tests/common`; this boundary lives in its own file because
//! `tests/architecture_boundaries.rs` sits at its 800-line budget.

mod common;
use common::assert_no_dep;

/// Every workspace member except `aegis-language` itself. Used in both
/// directions: none of these may depend on `aegis-language`, and
/// `aegis-language` (a leaf) may not depend on any of them.
const OTHER_WORKSPACE_CRATES: &[&str] = &[
    "aegis-types",
    "aegis-parser",
    "aegis-scanner",
    "aegis-policy",
    "aegis-config",
    "aegis-explanation",
    "aegis-tui",
    "aegis-audit",
    "aegis-snapshot",
    "aegis-starlark",
    "aegis-sandbox",
];

#[test]
fn no_workspace_crate_depends_on_aegis_language() {
    for &crate_name in OTHER_WORKSPACE_CRATES {
        assert_no_dep(crate_name, "aegis-language");
    }
}

#[test]
fn aegis_language_does_not_depend_on_any_workspace_crate() {
    for &forbidden in OTHER_WORKSPACE_CRATES {
        assert_no_dep("aegis-language", forbidden);
    }
}
