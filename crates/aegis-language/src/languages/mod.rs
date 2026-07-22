//! Per-language adapters (ADR-022 §3, §9).
//!
//! Each adapter uses Tree-sitter queries for structural capture and typed Rust
//! for semantic interpretation, emitting language-neutral [`crate::operation::
//! DetectedOperation`]s rather than assigning `RiskLevel` directly. Adapters are
//! staged by the L1 qualification gate; Python is the first production-qualified
//! adapter (plan Iteration 6), JavaScript is in qualification (plan Iteration 7).

pub mod javascript;
pub mod python;
