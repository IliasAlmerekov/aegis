//! Human-readable explanation generation for decisions.

pub mod formatter;
pub mod templates;

pub use templates::{
    AllowlistExplanation, CommandExplanation, ExecutionContextExplanation,
    ExecutionDecisionExplanation, ExecutionOutcomeExplanation, ExplainedPatternMatch,
    PolicyExplanation, ScanExplanation, SnapshotOutcomeExplanation,
};
