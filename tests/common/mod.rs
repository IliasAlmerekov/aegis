//! Shared helpers for architecture-boundary tests.
//!
//! Used by `tests/architecture_boundaries.rs` and
//! `tests/aegis_language_boundary.rs`. Lives in a subdirectory so Cargo does
//! not compile it as a standalone test target — it is pulled in via
//! `mod common;` from each consumer.

use std::fs;
use std::path::PathBuf;

/// Resolve a path relative to the repository root via the test crate's
/// `CARGO_MANIFEST_DIR` (the workspace root for the `aegis` test harness).
#[must_use]
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Read a workspace member's `Cargo.toml` and return its `[dependencies]`
/// section onward as a single lowercased string for substring checks. Extracting
/// from `[dependencies]` onward avoids matching the `[package]` description.
#[must_use]
pub fn crate_deps_section(crate_name: &str) -> String {
    let path = repo_root()
        .join("crates")
        .join(crate_name)
        .join("Cargo.toml");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    let lower = content.to_lowercase();
    lower
        .find("[dependencies]")
        .map_or(String::new(), |start| lower[start..].to_string())
}

/// Assert that a workspace crate does NOT list a forbidden dependency.
pub fn assert_no_dep(crate_name: &str, forbidden: &str) {
    let deps = crate_deps_section(crate_name);
    assert!(
        !deps.contains(&forbidden.to_lowercase()),
        "architecture boundary violated: `{crate_name}` must not depend on `{forbidden}` \
         (dependency DAG enforced by tests/architecture_boundaries.rs and \
         tests/aegis_language_boundary.rs)"
    );
}
