#![deny(missing_docs)]

//! Core data types shared across the Aegis crates.
//!
//! This crate is the foundation of the dependency DAG (Phase 4 of the
//! roadmap): it carries the pure data vocabulary — risk levels, the unified
//! pattern representation, and the human decision outcome — with no
//! dependencies on any other Aegis crate. Logic that *produces* these types
//! (the scanner, parser, policy engine, audit logger) lives in the crates that
//! depend on this one.

mod assessment;
mod command;
mod decision;
mod pattern;
mod policy;
mod risk;

pub use assessment::{Assessment, DecisionSource, HighlightRange, MatchResult};
pub use command::{InlineScript, ParsedCommand};
pub use decision::Decision;
pub use pattern::{Category, Pattern, PatternSource, PatternToken, PrefixPattern};
pub use policy::{AllowlistOverrideLevel, CiPolicy, Mode, SnapshotPolicy};
pub use risk::RiskLevel;
