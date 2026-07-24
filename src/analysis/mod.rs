//! Parent-side language analysis orchestration (ADR-022 §2/§6, L1 Iterations
//! 3-4).
//!
//! The parent process owns async orchestration: spawning the ephemeral worker,
//! framing requests/responses, enforcing deadlines, and converting worker
//! failures into typed degradation. The worker subprocess itself and the
//! framing protocol live in the `aegis-language` crate. [`router`] resolves
//! which parts of an intercepted command are analyzable source (Iteration 4);
//! wiring its output into [`worker_client`] and merging results into an
//! `Assessment` remains a later slice.

use std::path::Path;

/// Working-directory state used to resolve relative language-analysis sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisCwd<'a> {
    /// Relative source paths resolve from this directory.
    Resolved(&'a Path),
    /// The command working directory is unavailable; relative sources degrade.
    Unavailable,
}

pub mod heredoc;
pub mod mapping;
pub mod orchestrate;
pub mod queue;
pub mod recursive;
pub mod router;
pub mod source_reader;
pub mod worker_client;

pub use orchestrate::{OrchestrationBudget, Outcome, run, run_with_budget, run_with_budget_in_cwd};
pub use worker_client::{
    INTERNAL_LANGUAGE_WORKER_FLAG, RequestKind, TargetRequest, TargetResult, Worker, WorkerError,
    analyze,
};
