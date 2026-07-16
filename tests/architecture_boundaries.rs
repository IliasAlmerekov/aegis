//! Architecture boundary tests.
//!
//! These tests enforce the structural rules in `ARCHITECTURE.md` §4 (module
//! boundaries), §5 (invariants), §7 (file-size budgets), and §8 (public API
//! surface). A PR that breaks one of these fails CI.
//!
//! Conventions:
//! - All grep-style checks run against **production code only**. Helpers
//!   `strip_test_code` / `read_production` remove `#[cfg(test)]`-gated items
//!   before matching. This keeps test-only scaffolding (e.g. a `decide_command`
//!   helper in `shell_flow.rs`) from tripping the boundary rules.
//! - UI's data-type imports from `snapshot` (specifically `SnapshotRecord`) are
//!   an **explicit allow-leak** documented in ARCHITECTURE.md §4.
//! - Invariant I6 (snapshot registry laziness) is already covered by the
//!   `*_does_not_materialize_snapshot_registry` tests in
//!   `src/planning/core.rs`. Not duplicated here.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_file(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|err| panic!("failed to read {relative}: {err}"))
}

/// Read a source file and strip `#[cfg(test)]`-gated items so that boundary
/// checks only see production code.
fn read_production(relative: &str) -> String {
    strip_test_code(&read_file(relative))
}

/// Strip `#[cfg(test)]`-gated items and `mod tests { … }` blocks.
///
/// Not a full Rust parser — it operates line-by-line and is good enough for
/// the idioms used in this repo:
/// - `#[cfg(test)] use …;` → single-line use
/// - `#[cfg(test)] fn … { … }` / `impl … { … }` / `mod tests { … }` → brace-balanced
/// - `mod tests {` at column 0 without a preceding `#[cfg(test)]` → still stripped
///   because the convention in this repo is that `mod tests` is always test-only.
fn strip_test_code(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = String::with_capacity(content.len());
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim_start();

        let is_cfg_test = trimmed.starts_with("#[cfg(test)]");
        let is_mod_tests = trimmed.starts_with("mod tests {")
            || trimmed.starts_with("mod tests{")
            || trimmed == "mod tests {"
            || trimmed == "mod tests{";

        if !is_cfg_test && !is_mod_tests {
            out.push_str(lines[i]);
            out.push('\n');
            i += 1;
            continue;
        }

        if is_cfg_test {
            // Skip the attribute (and any following attribute lines).
            i += 1;
            while i < lines.len() && lines[i].trim_start().starts_with("#[") {
                i += 1;
            }
            if i >= lines.len() {
                break;
            }
        }

        // Look ahead to decide mode: does this item have a body (`{…}`) or is
        // it a declaration ending at `;`? Multi-line function signatures put
        // the opening `{` several lines below the `fn` keyword, so we cannot
        // decide from the first line alone.
        let mut look = i;
        let mut has_brace = false;
        while look < lines.len() {
            let l = lines[look];
            if let (Some(b), Some(s)) = (l.find('{'), l.find(';')) {
                has_brace = b < s;
                break;
            }
            if l.contains('{') {
                has_brace = true;
                break;
            }
            if l.contains(';') {
                break;
            }
            look += 1;
        }

        if has_brace {
            // Brace-balanced skip. Naive: does not ignore braces in strings or
            // comments, but acceptable for this repo's code. `seen_open`
            // avoids the false exit when the `fn` signature spans multiple
            // lines and the opening `{` is not on the first line.
            let mut depth: i32 = 0;
            let mut seen_open = false;
            loop {
                if i >= lines.len() {
                    break;
                }
                for c in lines[i].chars() {
                    if c == '{' {
                        depth += 1;
                        seen_open = true;
                    } else if c == '}' {
                        depth -= 1;
                    }
                }
                i += 1;
                if seen_open && depth <= 0 {
                    break;
                }
            }
            continue;
        }

        // Single-line item ending at ';'.
        while i < lines.len() && !lines[i].contains(';') {
            i += 1;
        }
        i += 1;
    }

    out
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}")) {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
            out.push(path);
        }
    }
}

fn rs_files_under(relative: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let root = repo_root().join(relative);
    if root.exists() {
        collect_rs_files(&root, &mut out);
    }
    out
}

fn assert_absent(source: &str, needle: &str, file: &str, rule: &str) {
    assert!(
        !source.contains(needle),
        "{file}: forbidden pattern {needle:?} — rule: {rule}"
    );
}

// ── §4 Forbidden edges — Policy engine is pure ────────────────────────────────

/// I1 + §4: `decision.rs` is a pure function. No I/O, no process spawning,
/// no tokio, no filesystem, no logging.
#[test]
fn decision_engine_is_pure_no_io() {
    for path in rs_files_under("src/decision") {
        let src = strip_test_code(&fs::read_to_string(&path).unwrap());
        let rel = path
            .strip_prefix(repo_root())
            .unwrap()
            .display()
            .to_string();
        for forbidden in [
            "std::fs",
            "std::process",
            "tokio::",
            "std::io",
            "std::env",
            "tracing::",
            "eprintln!",
            "println!",
        ] {
            assert_absent(
                &src,
                forbidden,
                &rel,
                "I1: policy engine must be a pure function — no I/O",
            );
        }
    }
}

// ── §4 Forbidden edges — Interceptor is a leaf ────────────────────────────────

/// §4: `interceptor/**` may not depend on `audit`, `snapshot`, `ui`, or
/// `runtime`. Scanner is transport-agnostic and has no recovery/logging/UI
/// concerns.
#[test]
fn interceptor_has_no_downstream_dependencies() {
    for path in rs_files_under("src/interceptor") {
        let src = strip_test_code(&fs::read_to_string(&path).unwrap());
        let rel = path
            .strip_prefix(repo_root())
            .unwrap()
            .display()
            .to_string();
        for forbidden in [
            "use crate::audit",
            "use crate::snapshot",
            "use crate::ui",
            "use crate::runtime",
            "use crate::planning",
            "use crate::decision",
        ] {
            assert_absent(
                &src,
                forbidden,
                &rel,
                "§4: interceptor must not depend on audit/snapshot/ui/runtime/planning/decision",
            );
        }
    }
}

// ── §4 Forbidden edges — UI is rendering only ─────────────────────────────────

/// §4: `ui/**` may not write audit entries, run snapshot business logic, or
/// depend on runtime/planning. The documented allow-leak: importing the
/// `SnapshotRecord` display type from `snapshot`. Calling
/// `SnapshotRegistry::*`, `.snapshot_all(`, or `.rollback(` is forbidden.
#[test]
fn ui_does_not_call_audit_or_snapshot_business_logic() {
    // Check both the shim layer (src/ui) and the real implementation (crates/aegis-tui/src).
    let paths: Vec<_> = rs_files_under("src/ui")
        .into_iter()
        .chain(rs_files_under("crates/aegis-tui/src"))
        .collect();

    for path in paths {
        let src = strip_test_code(&fs::read_to_string(&path).unwrap());
        let rel = path
            .strip_prefix(repo_root())
            .unwrap()
            .display()
            .to_string();

        // No audit coupling at all (binary-crate or workspace-crate form).
        assert_absent(
            &src,
            "use crate::audit",
            &rel,
            "§4: UI must not depend on audit",
        );
        assert_absent(
            &src,
            "use aegis_audit",
            &rel,
            "§4: UI must not depend on audit",
        );
        assert_absent(
            &src,
            "AuditLogger",
            &rel,
            "§4: UI must not reference AuditLogger",
        );

        // No runtime/planning orchestration leaks.
        assert_absent(
            &src,
            "use crate::runtime",
            &rel,
            "§4: UI must not depend on runtime",
        );
        assert_absent(
            &src,
            "use crate::planning",
            &rel,
            "§4: UI must not depend on planning",
        );

        // Snapshot business logic is forbidden; only SnapshotRecord
        // (data type used for display) is allowed.
        for forbidden in [
            "SnapshotRegistry",
            ".snapshot_all(",
            ".rollback(",
            "snapshot_registry",
        ] {
            assert_absent(
                &src,
                forbidden,
                &rel,
                "§4: UI may import SnapshotRecord but must not invoke snapshot business logic",
            );
        }
    }
}

// ── §4 Forbidden edges — Config is a leaf ─────────────────────────────────────

/// §4: `config/**` is a leaf module. It does not depend on runtime, planning,
/// UI, snapshot, or audit.
#[test]
fn config_is_a_leaf() {
    for path in rs_files_under("src/config") {
        let src = strip_test_code(&fs::read_to_string(&path).unwrap());
        let rel = path
            .strip_prefix(repo_root())
            .unwrap()
            .display()
            .to_string();
        for forbidden in [
            "use crate::runtime",
            "use crate::planning",
            "use crate::ui",
            "use crate::snapshot",
            "use crate::audit",
        ] {
            assert_absent(&src, forbidden, &rel, "§4: config must stay a leaf module");
        }
    }
}

// ── §4 Forbidden edges — Transports go through planning ───────────────────────

/// I4 + §4: transport modules (`shell_flow`, `watch`, `install`) must not
/// call `evaluate_policy` directly; they must go through `planning::*`.
/// Only `src/decision.rs` defines it and `src/planning/core.rs` consumes it.
#[test]
fn transports_route_policy_through_planning_module() {
    for relative in [
        "src/shell_flow.rs",
        "src/watch/mod.rs",
        "src/watch/runner.rs",
        "src/watch/sandbox.rs",
        "src/watch/protocol.rs",
        "src/install/mod.rs",
        "src/install/hook.rs",
        "src/install/claude.rs",
        "src/install/codex.rs",
    ] {
        let src = read_production(relative);
        assert_absent(
            &src,
            "evaluate_policy",
            relative,
            "I4: transports must not call evaluate_policy directly — route through planning::*",
        );
    }
}

/// Watch persists across commands, so synchronous platform preparation probes
/// must run off the Tokio worker that owns the control loop.
#[test]
fn watch_sandbox_preparation_does_not_block_the_async_control_loop() {
    let source = read_production("src/watch/sandbox.rs");

    assert!(
        source.contains("async fn prepare_watch_command"),
        "Watch Sandbox preparation must expose an async boundary"
    );
    assert!(
        source.contains("tokio::task::spawn_blocking"),
        "Watch Sandbox preparation must move synchronous platform probes to the blocking pool"
    );
}

// ── §7 File size budgets (simple hardcoded table) ─────────────────────────────

/// §7: enforce file-size budgets. Any `.rs` file under `src/` must stay below
/// the hard limit, except for the explicit allow-list of known breaches
/// tracked in ARCHITECTURE.md §7. If a file grows past its allow-list
/// ceiling, either split it or raise the ceiling explicitly (and update the
/// architecture doc).
///
/// Default hard limit: 2 000 lines.
/// Entrypoint (`src/main.rs`) gets its own tighter budget.
#[test]
fn file_size_budgets_are_respected() {
    const DEFAULT_HARD_LIMIT: usize = 2_000;

    // (path, allowed_max_lines) — current ceilings. Shrink these over time as
    // files get split; grow them only after updating ARCHITECTURE.md §7.
    let allowlist: &[(&str, usize)] = &[
        ("src/main.rs", 1_000),
        ("src/audit/logger.rs", 2_300),
        ("src/config/model.rs", 2_000),
        ("src/ui/confirm.rs", 1_800),
        ("src/snapshot/supabase.rs", 1_700),
        ("src/interceptor/scanner/mod.rs", 1_400),
    ];

    let mut failures: Vec<String> = Vec::new();

    for path in rs_files_under("src") {
        let rel = path
            .strip_prefix(repo_root())
            .unwrap()
            .display()
            .to_string()
            .replace('\\', "/");
        let line_count = fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .count();

        let limit = allowlist
            .iter()
            .find(|(p, _)| *p == rel)
            .map(|(_, l)| *l)
            .unwrap_or(DEFAULT_HARD_LIMIT);

        if line_count > limit {
            failures.push(format!(
                "{rel}: {line_count} lines exceeds budget of {limit} — \
                 either split the file or raise the budget in tests/architecture_boundaries.rs \
                 AND in ARCHITECTURE.md §7"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "file-size budgets exceeded:\n{}",
        failures.join("\n")
    );
}

// ── §8 Public API surface ─────────────────────────────────────────────────────

/// §8: `src/lib.rs` exports a fixed set of modules. Adding or removing a
/// top-level module is a public-API change and requires updating
/// ARCHITECTURE.md §8 and this test together.
#[test]
fn public_api_surface_is_stable() {
    let src = read_file("src/lib.rs");

    let expected: BTreeSet<&str> = [
        "audit",
        "config",
        "decision",
        "error",
        "explanation",
        "interceptor",
        "planning",
        "runtime",
        "runtime_gate",
        "snapshot",
        "toggle",
        "ui",
        "watch",
    ]
    .into_iter()
    .collect();

    let found: BTreeSet<String> = src
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("pub mod ")?;
            let name = rest.trim_end_matches(';').trim_end_matches('{').trim();
            Some(name.to_string())
        })
        .collect();

    let found_refs: BTreeSet<&str> = found.iter().map(String::as_str).collect();

    let added: Vec<&&str> = found_refs.difference(&expected).collect();
    let removed: Vec<&&str> = expected.difference(&found_refs).collect();

    assert!(
        added.is_empty() && removed.is_empty(),
        "public API surface changed — update ARCHITECTURE.md §8 and this test.\n\
         added modules: {added:?}\nremoved modules: {removed:?}"
    );
}

// ── Workspace crate dependency DAG ───────────────────────────────────────────

/// Reads a workspace member's `Cargo.toml` and returns its `[dependencies]`
/// section as a single string (lowercased) for substring checks.
fn crate_deps_section(crate_name: &str) -> String {
    let path = repo_root()
        .join("crates")
        .join(crate_name)
        .join("Cargo.toml");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    // Extract from [dependencies] onward so we don't accidentally match the
    // [package] description field.
    let lower = content.to_lowercase();
    if let Some(start) = lower.find("[dependencies]") {
        lower[start..].to_string()
    } else {
        String::new()
    }
}

/// Asserts that a workspace crate does NOT list a forbidden dependency.
fn assert_no_dep(crate_name: &str, forbidden: &str) {
    let deps = crate_deps_section(crate_name);
    assert!(
        !deps.contains(&forbidden.to_lowercase()),
        "architecture boundary violated: `{crate_name}` must not depend on `{forbidden}` \
         (dependency DAG enforced by tests/architecture_boundaries.rs)"
    );
}

#[test]
fn aegis_parser_must_not_depend_on_aegis_audit() {
    assert_no_dep("aegis-parser", "aegis-audit");
}

#[test]
fn aegis_parser_must_not_depend_on_aegis_config() {
    assert_no_dep("aegis-parser", "aegis-config");
}

#[test]
fn aegis_parser_must_not_depend_on_aegis_explanation() {
    assert_no_dep("aegis-parser", "aegis-explanation");
}

#[test]
fn aegis_parser_must_not_depend_on_aegis_tui() {
    assert_no_dep("aegis-parser", "aegis-tui");
}

#[test]
fn aegis_parser_must_not_depend_on_aegis_snapshot() {
    assert_no_dep("aegis-parser", "aegis-snapshot");
}

#[test]
fn aegis_scanner_must_not_depend_on_aegis_audit() {
    assert_no_dep("aegis-scanner", "aegis-audit");
}

#[test]
fn aegis_scanner_must_not_depend_on_aegis_config() {
    assert_no_dep("aegis-scanner", "aegis-config");
}

#[test]
fn aegis_scanner_must_not_depend_on_aegis_explanation() {
    assert_no_dep("aegis-scanner", "aegis-explanation");
}

#[test]
fn aegis_scanner_must_not_depend_on_aegis_tui() {
    assert_no_dep("aegis-scanner", "aegis-tui");
}

#[test]
fn aegis_scanner_must_not_depend_on_aegis_snapshot() {
    assert_no_dep("aegis-scanner", "aegis-snapshot");
}

#[test]
fn aegis_types_must_not_depend_on_aegis_audit() {
    assert_no_dep("aegis-types", "aegis-audit");
}

#[test]
fn aegis_types_must_not_depend_on_aegis_config() {
    assert_no_dep("aegis-types", "aegis-config");
}

#[test]
fn aegis_types_must_not_depend_on_aegis_explanation() {
    assert_no_dep("aegis-types", "aegis-explanation");
}

#[test]
fn aegis_types_must_not_depend_on_aegis_tui() {
    assert_no_dep("aegis-types", "aegis-tui");
}

#[test]
fn aegis_types_must_not_depend_on_aegis_snapshot() {
    assert_no_dep("aegis-types", "aegis-snapshot");
}

// ── Missing edges for parser (scanner and policy are downstream) ──────────────

#[test]
fn aegis_parser_must_not_depend_on_aegis_scanner() {
    assert_no_dep("aegis-parser", "aegis-scanner");
}

#[test]
fn aegis_parser_must_not_depend_on_aegis_policy() {
    assert_no_dep("aegis-parser", "aegis-policy");
}

// ── Missing edges for scanner (policy is downstream) ─────────────────────────

#[test]
fn aegis_scanner_must_not_depend_on_aegis_policy() {
    assert_no_dep("aegis-scanner", "aegis-policy");
}

// ── Missing edges for types (all other crates are downstream) ────────────────

#[test]
fn aegis_types_must_not_depend_on_aegis_parser() {
    assert_no_dep("aegis-types", "aegis-parser");
}

#[test]
fn aegis_types_must_not_depend_on_aegis_scanner() {
    assert_no_dep("aegis-types", "aegis-scanner");
}

#[test]
fn aegis_types_must_not_depend_on_aegis_policy() {
    assert_no_dep("aegis-types", "aegis-policy");
}

// ── DAG boundaries for policy (config/explanation/tui/audit/snapshot are downstream) ──

#[test]
fn aegis_policy_must_not_depend_on_aegis_config() {
    assert_no_dep("aegis-policy", "aegis-config");
}

#[test]
fn aegis_policy_must_not_depend_on_aegis_explanation() {
    assert_no_dep("aegis-policy", "aegis-explanation");
}

#[test]
fn aegis_policy_must_not_depend_on_aegis_tui() {
    assert_no_dep("aegis-policy", "aegis-tui");
}

#[test]
fn aegis_policy_must_not_depend_on_aegis_audit() {
    assert_no_dep("aegis-policy", "aegis-audit");
}

#[test]
fn aegis_policy_must_not_depend_on_aegis_snapshot() {
    assert_no_dep("aegis-policy", "aegis-snapshot");
}

// ── DAG boundaries for config (explanation/tui/audit are downstream) ─────────

#[test]
fn aegis_config_must_not_depend_on_aegis_explanation() {
    assert_no_dep("aegis-config", "aegis-explanation");
}

#[test]
fn aegis_config_must_not_depend_on_aegis_tui() {
    assert_no_dep("aegis-config", "aegis-tui");
}

#[test]
fn aegis_config_must_not_depend_on_aegis_audit() {
    assert_no_dep("aegis-config", "aegis-audit");
}

// ── DAG boundaries for explanation (tui/audit are downstream) ────────────────

#[test]
fn aegis_explanation_must_not_depend_on_aegis_tui() {
    assert_no_dep("aegis-explanation", "aegis-tui");
}

#[test]
fn aegis_explanation_must_not_depend_on_aegis_audit() {
    assert_no_dep("aegis-explanation", "aegis-audit");
}

// ── DAG boundaries for snapshot (explanation/tui/audit/policy are downstream) ─

#[test]
fn aegis_snapshot_must_not_depend_on_aegis_explanation() {
    assert_no_dep("aegis-snapshot", "aegis-explanation");
}

#[test]
fn aegis_snapshot_must_not_depend_on_aegis_tui() {
    assert_no_dep("aegis-snapshot", "aegis-tui");
}

#[test]
fn aegis_snapshot_must_not_depend_on_aegis_audit() {
    assert_no_dep("aegis-snapshot", "aegis-audit");
}

#[test]
fn aegis_snapshot_must_not_depend_on_aegis_policy() {
    assert_no_dep("aegis-snapshot", "aegis-policy");
}

// ── DAG boundaries for tui (audit is downstream) ─────────────────────────────

#[test]
fn aegis_tui_must_not_depend_on_aegis_audit() {
    assert_no_dep("aegis-tui", "aegis-audit");
}

// ── Self-tests for the stripper ───────────────────────────────────────────────

#[cfg(test)]
mod stripper_self_tests {
    use super::strip_test_code;

    #[test]
    fn strips_cfg_test_mod_block() {
        let input = "\
fn prod() { 1 }

#[cfg(test)]
mod tests {
    fn inner() { 2 }
}

fn more() { 3 }
";
        let out = strip_test_code(input);
        assert!(out.contains("fn prod()"));
        assert!(out.contains("fn more()"));
        assert!(!out.contains("fn inner()"));
        assert!(!out.contains("#[cfg(test)]"));
    }

    #[test]
    fn strips_cfg_test_use_line() {
        let input = "\
use crate::real;

#[cfg(test)]
use crate::only_in_tests;

fn prod() {}
";
        let out = strip_test_code(input);
        assert!(out.contains("use crate::real"));
        assert!(!out.contains("only_in_tests"));
    }

    #[test]
    fn strips_cfg_test_free_fn() {
        let input = "\
fn prod() {}

#[cfg(test)]
fn test_only_helper() {
    let _x = 1;
}

fn more() {}
";
        let out = strip_test_code(input);
        assert!(out.contains("fn prod()"));
        assert!(out.contains("fn more()"));
        assert!(!out.contains("test_only_helper"));
    }

    #[test]
    fn strips_bare_mod_tests() {
        let input = "\
fn prod() {}

mod tests {
    fn inner() {}
}
";
        let out = strip_test_code(input);
        assert!(out.contains("fn prod()"));
        assert!(!out.contains("fn inner()"));
    }

    #[test]
    fn preserves_production_code_with_no_tests() {
        let input = "\
use std::fs;

fn prod() { 1 }
";
        let out = strip_test_code(input);
        assert!(out.contains("use std::fs"));
        assert!(out.contains("fn prod()"));
    }
}
