#![deny(missing_docs)]

//! Aegis — a lightweight Rust CLI that acts as a `$SHELL` proxy,
//! intercepting AI agent commands and requiring human confirmation before
//! destructive operations.

/// Parent-side language analysis orchestration (worker client).
pub mod analysis;
/// Append-only audit recorder and integrity checker.
pub mod audit;
/// Configuration loading, validation, and layered merge.
pub mod config;
/// Pure policy evaluation engine.
pub mod decision;
/// Typed error hierarchy for the whole crate.
pub mod error;
/// Human-readable explanation generation for decisions.
pub mod explanation;
/// Command scanner: tokenisation, pattern matching, risk assessment.
pub mod interceptor;
/// Orchestration layer that wraps the pure policy engine.
pub mod planning;
/// Runtime context and dependency wiring.
pub mod runtime;
/// Shared CI detection used by CLI entrypoints.
pub mod runtime_gate;
/// Snapshot plugin trait and built-in providers.
pub mod snapshot;
/// Global on/off toggle state helpers.
pub mod toggle;
/// Terminal UI confirmation dialogs.
pub mod ui;
/// Watch-mode NDJSON protocol and runner.
pub mod watch;
