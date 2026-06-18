//! Test hooks for observing snapshot registry materialization.
//!
//! Exposed so that upstream crates can observe how many times the registry
//! was materialised in a single test thread — without introducing a hard
//! dependency on the internal implementation.

use std::cell::Cell;

thread_local! {
    static REGISTRY_BUILD_COUNT: Cell<usize> = const { Cell::new(0) };
}

/// Increment the per-thread registry build counter.
pub fn increment_registry_build_count() {
    REGISTRY_BUILD_COUNT.with(|c| c.set(c.get() + 1));
}

/// Read the per-thread registry build counter.
pub fn registry_build_count() -> usize {
    REGISTRY_BUILD_COUNT.with(Cell::get)
}

/// Reset the per-thread registry build counter to zero.
pub fn reset_registry_build_count() {
    REGISTRY_BUILD_COUNT.with(|c| c.set(0));
}
