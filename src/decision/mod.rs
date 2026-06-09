//! Pure policy evaluation engine.
//!
//! The implementation lives in the `aegis-policy` crate. This module re-exports
//! its public API so existing `crate::decision::*` call sites remain stable
//! while the workspace split (Phase 4) is in progress.

pub use aegis_policy::{
    BlockReason, DefaultPolicyEngine, ExecutionTransport, PolicyAction, PolicyAllowlistResult,
    PolicyBlocklistResult, PolicyCiState, PolicyConfigFlags, PolicyDecision, PolicyEngine,
    PolicyExecutionContext, PolicyInput, PolicyRationale, evaluate_policy,
};
