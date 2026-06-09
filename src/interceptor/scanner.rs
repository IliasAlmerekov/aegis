//! Scanner: keyword-based quick scan + regex full scan.
//!
//! The implementation lives in the `aegis-scanner` crate. This module re-exports
//! its public types so existing `crate::interceptor::scanner::*` call sites
//! remain stable while the workspace split (Phase 4) is in progress.

pub use aegis_scanner::{
    Assessment, DecisionSource, HighlightRange, MatchResult, PatternToken, PrefixPattern,
    PrefixRule, Scanner,
};
