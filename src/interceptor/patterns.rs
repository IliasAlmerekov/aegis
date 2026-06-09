//! Pattern definitions, categories, and built-in pattern loading.
//!
//! The implementation lives in the `aegis-scanner` crate. This module re-exports
//! its pattern types so existing `crate::interceptor::patterns::*` call sites
//! remain stable while the workspace split (Phase 4) is in progress.

pub use aegis_scanner::{
    Category, Pattern, PatternSet, PatternSource, PatternToken, PrefixPattern, PrefixRule,
};
