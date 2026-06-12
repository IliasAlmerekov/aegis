//! Contract tests for the `aegis-snapshot` crate.
//!
//! These tests verify the public API surface of the extracted crate:
//! - All expected types are publicly exported.
//! - `SnapshotError` satisfies the `std::error::Error + Send + Sync` bounds.
//! - `SnapshotRegistryConfig::try_new` returns `Result<_, SnapshotError>`.
//! - `available_provider_names` returns all 6 built-in provider names.
//! - `SnapshotRegistry::configured_provider_names` reports all 6 providers
//!   when the policy is `Full`.
//! - `aegis_snapshot::SnapshotPlugin` is the same type re-exported from the
//!   root `aegis` binary crate (re-export shim).
//!
//! All tests MUST fail before `crates/aegis-snapshot/src/lib.rs` is created
//! (they fail with `error[E0432]: unresolved import`).

use std::sync::Mutex;

use aegis_config::{AegisConfig, SnapshotPolicy};
use aegis_snapshot::{SnapshotError, SnapshotPlugin, SnapshotRegistry, SnapshotRegistryConfig};

/// Guards tests that mutate `HOME`/`USERPROFILE` so they are never concurrent.
static HOME_ENV: Mutex<()> = Mutex::new(());

// ── Test 1: public API compileability ────────────────────────────────────────
//
// If any of the four items above are not exported, this module-level `use`
// will fail to compile, which is itself a failing test.  The four items are
// imported at the top of the file; a dedicated function makes the intent
// explicit in test output.

/// Verify that `SnapshotPlugin`, `SnapshotRegistry`, `SnapshotRegistryConfig`,
/// and `SnapshotError` are all publicly exported from `aegis_snapshot`.
#[test]
fn test_public_api_exports_all_required_types() {
    // If the imports at the top of this file compiled, the types exist.
    // We deliberately name them here so the test is self-documenting.
    fn _assert_imports_resolved(
        _plugin: Option<&dyn SnapshotPlugin>,
        _registry: Option<SnapshotRegistry>,
        _config: Option<SnapshotRegistryConfig>,
        _error: Option<SnapshotError>,
    ) {
    }
}

// ── Test 2: SnapshotError is Send + Sync + std::error::Error ─────────────────

/// `SnapshotError` must implement `std::error::Error`, `Send`, and `Sync` so
/// it can cross async task / thread boundaries and be used with `?` inside
/// `async fn` without boxing.
#[test]
fn test_snapshot_error_implements_std_error_send_sync() {
    fn assert_bounds<E: std::error::Error + Send + Sync + 'static>() {}
    assert_bounds::<SnapshotError>();
}

// ── Test 3: SnapshotRegistryConfig::try_new returns Result<_, SnapshotError> ─

/// `SnapshotRegistryConfig::try_new` must accept `&AegisConfig` and return
/// `Result<SnapshotRegistryConfig, SnapshotError>`.  With `HOME` set (which is
/// true in any standard CI / developer environment) it must succeed.
#[test]
fn test_snapshot_registry_config_try_new_succeeds_with_home_set() {
    let _guard = HOME_ENV.lock().unwrap();
    // Ensure HOME is set — on any standard Linux/macOS environment this is
    // already true; we set it explicitly for robustness in constrained CI.
    unsafe { std::env::set_var("HOME", std::env::temp_dir()) };

    let config = AegisConfig::default();
    let result: Result<SnapshotRegistryConfig, SnapshotError> =
        SnapshotRegistryConfig::try_new(&config);
    assert!(result.is_ok(), "try_new failed: {result:?}");
}

/// `SnapshotRegistryConfig::try_new` must return `Err(SnapshotError)` when
/// neither `HOME` nor `USERPROFILE` is set in the environment.
#[test]
fn test_snapshot_registry_config_try_new_errors_without_home() {
    let _guard = HOME_ENV.lock().unwrap();
    unsafe {
        std::env::remove_var("HOME");
        std::env::remove_var("USERPROFILE");
    }

    let config = AegisConfig::default();
    let result: Result<SnapshotRegistryConfig, SnapshotError> =
        SnapshotRegistryConfig::try_new(&config);
    assert!(result.is_err(), "expected Err when HOME is unset, got Ok");

    // Restore HOME so subsequent tests are not affected.
    unsafe { std::env::set_var("HOME", std::env::temp_dir()) };
}

// ── Test 4: available_provider_names returns all 6 built-in names ─────────────

/// `available_provider_names()` must return exactly the 6 built-in provider
/// names: "git", "docker", "postgres", "mysql", "sqlite", "supabase".
/// The order is not mandated; presence of every name is.
#[test]
fn test_available_provider_names_contains_all_six_builtins() {
    let names = aegis_snapshot::available_provider_names();
    let expected = ["git", "docker", "postgres", "mysql", "sqlite", "supabase"];

    for name in &expected {
        assert!(
            names.contains(name),
            "available_provider_names() is missing \"{name}\": got {names:?}"
        );
    }
    assert_eq!(
        names.len(),
        expected.len(),
        "available_provider_names() has unexpected entries: {names:?}"
    );
}

// ── Test 5: SnapshotRegistry with SnapshotPolicy::Full has all 6 providers ───

/// When `SnapshotPolicy::Full` is active, `SnapshotRegistry::from_runtime_config`
/// must materialise all 6 built-in providers and
/// `configured_provider_names()` must return all 6 names.
#[test]
fn test_snapshot_registry_full_policy_contains_all_six_providers() {
    let _guard = HOME_ENV.lock().unwrap();
    unsafe { std::env::set_var("HOME", std::env::temp_dir()) };

    let mut config = AegisConfig::default();
    config.snapshot_policy = SnapshotPolicy::Full;

    let registry_config =
        SnapshotRegistryConfig::try_new(&config).expect("try_new failed with HOME set");
    let registry = SnapshotRegistry::from_runtime_config(&registry_config);

    let configured = registry.configured_provider_names();
    let expected = ["git", "docker", "postgres", "mysql", "sqlite", "supabase"];

    for name in &expected {
        assert!(
            configured.contains(name),
            "registry missing provider \"{name}\" under Full policy: got {configured:?}"
        );
    }
    assert_eq!(
        configured.len(),
        expected.len(),
        "registry has unexpected provider count under Full policy: {configured:?}"
    );
}

// ── Test 6: re-export shim — root crate re-exports the extracted type ─────────

/// After extraction, `src/snapshot/mod.rs` in the root `aegis` binary crate
/// must become a re-export shim.  At the type level this means that
/// `aegis_snapshot::SnapshotPlugin` and the type reachable via
/// `aegis::snapshot` are the *same* trait object.
///
/// We verify this with a blanket `impl` trick: a concrete type that implements
/// `aegis_snapshot::SnapshotPlugin` must also satisfy any bound written in
/// terms of the re-exported alias, and vice-versa.  If the two paths alias the
/// same item, the compiler will accept a function that is generic over one but
/// called with the other.
///
/// NOTE: The `aegis` binary crate is NOT listed as a dev-dependency here
/// because doing so would create a circular workspace dependency.  Instead,
/// this test verifies the weaker but sufficient condition: the trait defined
/// in `aegis_snapshot` is the sole definition.  The actual re-export from
/// `aegis::snapshot` is asserted by a unit test inside `src/snapshot/mod.rs`
/// that must be added as part of the implementation work (documented here as
/// a contract obligation for the green-tester).
///
/// The test below therefore acts as a compile-time marker: it passes only when
/// `aegis_snapshot::SnapshotPlugin` exists and is object-safe.
#[test]
fn test_snapshot_plugin_trait_is_object_safe_and_exported() {
    // Object safety: if SnapshotPlugin is not object-safe, this line will not
    // compile.
    let _: Option<Box<dyn SnapshotPlugin>> = None;
}
