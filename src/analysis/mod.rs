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

pub mod heredoc;
pub mod queue;
pub mod recursive;
pub mod router;
pub mod source_reader;
pub mod worker_client;

pub use worker_client::{
    INTERNAL_LANGUAGE_WORKER_FLAG, TargetRequest, TargetResult, Worker, WorkerError, analyze,
};
