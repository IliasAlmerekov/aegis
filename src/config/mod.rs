//! Configuration loading, validation, and layered merge.
//!
//! The implementation lives in the `aegis-config` crate. This module re-exports
//! its full public API (including submodules `allowlist`, `amend`, `model`,
//! `snapshot`, `validate`, `error`) so existing `crate::config::*` and
//! `crate::config::<submodule>::*` call sites remain stable while the workspace
//! split (Phase 4) is in progress.

pub use aegis_config::*;
