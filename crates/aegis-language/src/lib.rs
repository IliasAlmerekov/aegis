//! Language-aware analysis foundation for Aegis.
//!
//! This crate is the focused workspace boundary that owns Tree-sitter parsing,
//! the grammar manifest, and — in later L1 iterations — the language adapters
//! that turn a parsed tree into language-neutral detected operations. (The
//! detected-operation vocabulary is introduced only when its classifier ships,
//! per the plan's "not a design scratchpad" rule; Iteration 0 has no adapter.)
//! Per ADR-022:
//!
//! - It is an *additive* slow path; it never replaces the shell `Scanner` or
//!   regresses the no-source safe-command hot path.
//! - Parsing runs in an ephemeral worker process; there is no daemon, plugin
//!   loader, or network service.
//! - It is the only crate permitted to add the narrowly scoped native C
//!   toolchain (pinned Tree-sitter runtime plus production-qualified
//!   grammars). It must not be depended on by `aegis-types` (ADR-022 §4 review
//!   gate).
//!
//! Iteration 0 establishes only the grammar-manifest qualification contract
//! and the crate skeleton. The worker, adapters, and runtime dependencies
//! land in later iterations of the L1 plan.

pub mod language;
pub mod languages;
pub mod manifest;
pub mod operation;
pub mod protocol;
pub mod router;
pub mod worker;

pub use language::{ParseError, SourceLanguage, parse};
pub use protocol::{
    DecodeError, DecodedFrame, EncodeError, MAX_FRAME_PAYLOAD, MAX_SOURCE_BYTES, Request, Response,
};
pub use router::SourceTarget;
pub use worker::{Outcome, RunOutcome, analyze, run};
