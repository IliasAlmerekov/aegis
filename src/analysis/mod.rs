//! Parent-side language analysis orchestration (ADR-022 §2, L1 Iteration 3).
//!
//! The parent process owns async orchestration: spawning the ephemeral worker,
//! framing requests/responses, enforcing deadlines, and converting worker
//! failures into typed degradation. The worker subprocess itself and the
//! framing protocol live in the `aegis-language` crate; this module is the
//! client the rest of the binary will call after source routing (Iteration 4).

pub mod worker_client;

pub use worker_client::{
    INTERNAL_LANGUAGE_WORKER_FLAG, TargetRequest, TargetResult, Worker, WorkerError, analyze,
};
