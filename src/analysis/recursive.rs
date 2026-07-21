//! Cross-language execution-sink invariant (ADR-022 §3/§7, plan Iteration 5
//! Slice 3).
//!
//! A recognized process/shell/eval sink always emits a `CodeExecution` Match;
//! a statically recovered literal payload additionally becomes a bounded
//! recursive target, while a dynamic/encoded payload records typed degradation
//! and is never evaluated or decoded.

use aegis_language::SourceLanguage;
use aegis_types::{
    AnalysisProvenance, DegradationReason, DetectedOperation, HighlightRange, MatchResult,
    OperationKind, language_match,
};

use super::queue::QueueTarget;

/// The recursive-analysis decision for a recognized process/shell/eval sink's
/// payload (ADR-022 §3, §7).
///
/// Always carries the sink's [`MatchResult`] (a recognized execution sink
/// always emits a `CodeExecution` Match). The payload is either enqueued as a
/// bounded recursive [`QueueTarget`] (a literal payload) or recorded as typed
/// degradation (a dynamic or encoded payload) — never both, and a
/// dynamic/encoded payload is never evaluated or decoded.
#[derive(Debug, Clone)]
pub struct SinkDecision {
    /// The `CodeExecution` Match for the visible sink (always present).
    pub sink_match: MatchResult,
    /// The recursive target to enqueue, if the payload is a literal.
    pub recursive_target: Option<QueueTarget>,
    /// Degradation recorded for the payload, if it is dynamic or encoded.
    pub degradation: Option<DegradationReason>,
}

/// Decide the recursive-analysis handling for a recognized process/shell/eval
/// sink (ADR-022 §3, §7).
///
/// `op` should be a `CodeExecution` operation (the caller — a language adapter
/// — only invokes this for recognized execution sinks). `resolved_payload` is
/// `Some(literal)` when the adapter statically recovered a literal payload
/// (`OperandCertainty::Known`), and `None` for a dynamic, partial, or encoded
/// payload.
///
/// - `Some(literal)` → `CodeExecution` Match + a bounded recursive
///   [`QueueTarget`] at `parent_depth + 1`, parsed as `payload_language`. No
///   degradation.
/// - `None` → `CodeExecution` Match + [`DegradationReason::DynamicSource`], no
///   target. The payload is never evaluated or decoded (ADR-022 §7).
///
/// The sink Match is always `Danger` (`LANG-EXEC`); degradation is orthogonal
/// to `RiskLevel` and never lowers it (ADR-022 §5).
#[allow(clippy::too_many_arguments)]
pub fn handle_sink(
    op: &DetectedOperation,
    payload_language: SourceLanguage,
    resolved_payload: Option<&str>,
    provenance: AnalysisProvenance,
    matched_text: &str,
    highlight_range: Option<HighlightRange>,
    parent_depth: u32,
) -> SinkDecision {
    // Contract: the caller (a language adapter) only invokes this for a
    // recognized execution sink. A non-`CodeExecution` op would mislabel
    // `sink_match` with that op's own classification while still enqueuing a
    // recursive target — a silent semantic break with no adapter yet to catch
    // it. Checked under `debug_assert!` so release builds pay nothing.
    debug_assert!(
        op.kind == OperationKind::CodeExecution,
        "handle_sink is only for recognized CodeExecution sinks; got {:?}",
        op.kind,
    );

    // The visible sink always emits its CodeExecution Match, regardless of
    // payload certainty (ADR-022 §3/§7 REVIEW GATE).
    let sink_match = language_match(op, provenance, matched_text, highlight_range);

    match resolved_payload {
        // A statically recovered literal payload becomes a bounded recursive
        // target at parent_depth + 1, parsed as the payload's own language.
        Some(literal) => SinkDecision {
            sink_match,
            recursive_target: Some(QueueTarget::new(
                payload_language,
                literal.to_string(),
                parent_depth + 1,
            )),
            degradation: None,
        },
        // A dynamic, partial, or encoded payload is never evaluated or decoded
        // (ADR-022 §7); it records typed degradation and no recursive target.
        None => SinkDecision {
            sink_match,
            recursive_target: None,
            degradation: Some(DegradationReason::DynamicSource),
        },
    }
}

#[cfg(test)]
#[path = "recursive_tests.rs"]
mod tests;
