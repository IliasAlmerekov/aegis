#![deny(missing_docs)]

//! Pure policy evaluation for Aegis.
//!
//! Given a scanner [`aegis_scanner::Assessment`] and the surrounding decision
//! context (operating mode, CI state, allowlist/blocklist results, snapshot
//! policy), the [`PolicyEngine`] yields a [`PolicyDecision`]. Evaluation is a
//! pure function with no I/O and no side effects — persistence of decisions
//! ("amend") is a separate, config-layer concern.

mod engine;
mod types;

pub use engine::{DefaultPolicyEngine, PolicyEngine, evaluate_policy};
pub use types::{
    BlockReason, ExecutionTransport, PolicyAction, PolicyAllowlistResult, PolicyBlocklistResult,
    PolicyCiState, PolicyConfigFlags, PolicyDecision, PolicyExecutionContext, PolicyInput,
    PolicyRationale, PolicyRulesResult,
};
