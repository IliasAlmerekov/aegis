#![deny(missing_docs)]

//! Command risk scanning for Aegis.
//!
//! This crate owns the [`Scanner`] (Aho-Corasick quick scan + regex full scan +
//! token-prefix matching) and the [`PatternSet`] that feeds it (built-in
//! patterns embedded from `patterns.toml`, merged with caller-supplied custom
//! patterns). It depends on `aegis-types` for the data vocabulary and
//! `aegis-parser` for tokenization. It is deliberately ignorant of where custom
//! patterns come from — callers convert their config types into [`Pattern`]
//! before handing them over.

mod error;
mod nested;
mod patterns;
mod scanner;

pub use aegis_types::{
    AssessmentBasis, Category, DetectionMechanism, DetectionSource, MatchEvidence, Pattern,
    PatternSource, PatternToken, PrefixPattern,
};
pub use error::ScannerError;
pub use patterns::{PatternSet, PrefixRule};
pub use scanner::{Assessment, DecisionSource, HighlightRange, MatchResult, Scanner};
