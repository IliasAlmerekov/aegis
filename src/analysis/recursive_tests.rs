//! RED tests for the cross-language execution-sink invariant (plan
//! Iteration 5, Slice 3; ADR-022 §3/§7).
//!
//! Pins the one invariant the plan names: a recognized process/shell/eval
//! sink always emits a `CodeExecution` Match. A literal payload additionally
//! becomes a bounded recursive target; a dynamic or encoded payload records
//! `Analysis` degradation instead and is never evaluated or decoded.

use super::{SinkDecision, handle_sink};
use crate::analysis::queue::{AnalysisQueue, PushOutcome, QueueBudget, QueueTarget};
use aegis_language::SourceLanguage;
use aegis_types::{
    AnalysisProvenance, AnalysisStatus, ByteSpan, Category, DegradationReason, DetectedOperation,
    DetectionMechanism, OperandCertainty, OperationKind, OperationModifiers, RiskLevel,
    SourceOrigin,
};

fn code_exec(certainty: OperandCertainty) -> DetectedOperation {
    DetectedOperation {
        kind: OperationKind::CodeExecution,
        modifiers: OperationModifiers::default(),
        certainty,
    }
}

fn provenance(op: &DetectedOperation) -> AnalysisProvenance {
    AnalysisProvenance {
        language: Some("python".to_string()),
        source_origin: SourceOrigin::Inline,
        rule_id: None,
        operation: Some(op.clone()),
        file_path: None,
        source_hash: Some("0".repeat(64)),
        span: Some(ByteSpan {
            line: 1,
            column: 1,
            byte_start: 0,
            byte_end: 4,
        }),
        certainty: op.certainty,
        status: AnalysisStatus::Complete,
        degradation_reason: None,
    }
}

#[test]
fn literal_payload_emits_match_and_recursive_target() {
    let op = code_exec(OperandCertainty::Known);
    let d: SinkDecision = handle_sink(
        &op,
        SourceLanguage::Bash,
        Some("rm -rf /tmp/x"),
        provenance(&op),
        "subprocess.run(['bash','-c','rm -rf /tmp/x'])",
        None,
        0,
    );

    // The visible sink always emits a CodeExecution Match.
    assert_eq!(d.sink_match.pattern.risk, RiskLevel::Danger);
    assert_eq!(d.sink_match.pattern.category, Category::Process);
    assert_eq!(d.sink_match.pattern.id.as_ref(), "LANG-EXEC");
    assert_eq!(
        d.sink_match.evidence.mechanism(),
        DetectionMechanism::LanguageRule
    );
    // A literal payload becomes a bounded recursive target at parent_depth + 1.
    let target = d
        .recursive_target
        .expect("literal payload enqueues a target");
    assert_eq!(target.language, SourceLanguage::Bash);
    assert_eq!(
        target.depth, 1,
        "root sink at depth 0 → payload target at depth 1"
    );
    assert_eq!(target.source, "rm -rf /tmp/x");
    // No degradation when the payload is a literal.
    assert_eq!(d.degradation, None);
}

#[test]
fn dynamic_payload_emits_match_and_degradation_without_target() {
    let op = code_exec(OperandCertainty::Dynamic);
    let d = handle_sink(
        &op,
        SourceLanguage::Bash,
        None, // dynamic payload — not statically recoverable
        provenance(&op),
        "subprocess.run(user_input)",
        None,
        0,
    );

    // The sink Match is retained (REVIEW GATE: dynamic sink keeps its Match).
    assert_eq!(d.sink_match.pattern.risk, RiskLevel::Danger);
    assert_eq!(d.sink_match.pattern.id.as_ref(), "LANG-EXEC");
    // No recursive target — the payload is never evaluated.
    assert!(
        d.recursive_target.is_none(),
        "a dynamic payload must not be enqueued"
    );
    // Typed degradation instead.
    assert_eq!(d.degradation, Some(DegradationReason::DynamicSource));
}

#[test]
fn partial_payload_emits_match_and_degradation_without_target() {
    // A Partial operand (alias / adjacent literal, not fully resolved) is not
    // a complete literal payload, so it is not enqueued; the sink still Matchs.
    let op = code_exec(OperandCertainty::Partial);
    let d = handle_sink(
        &op,
        SourceLanguage::Bash,
        None,
        provenance(&op),
        "subprocess.run(alias)",
        None,
        0,
    );
    assert_eq!(d.sink_match.pattern.risk, RiskLevel::Danger);
    assert!(d.recursive_target.is_none());
    assert_eq!(d.degradation, Some(DegradationReason::DynamicSource));
}

#[test]
fn decode_to_eval_emits_match_and_degradation_without_decoding() {
    // A base64-wrapped payload is a decode-to-eval shape (ADR-022 §7). Aegis
    // does not decode it: no recursive target carries decoded bytes, and the
    // raw encoded string is not treated as literal source either. The visible
    // sink still emits a CodeExecution Match plus degradation.
    let op = code_exec(OperandCertainty::Dynamic);
    let encoded = "eval(base64.b64decode('b3MucmVtb3ZlKCcp'))";
    let d = handle_sink(
        &op,
        SourceLanguage::Python,
        None, // encoded payload is not a resolved literal
        provenance(&op),
        encoded,
        None,
        0,
    );
    assert_eq!(d.sink_match.pattern.id.as_ref(), "LANG-EXEC");
    assert_eq!(d.sink_match.pattern.risk, RiskLevel::Danger);
    assert!(
        d.recursive_target.is_none(),
        "encoded payload must not be decoded/enqueued"
    );
    assert_eq!(d.degradation, Some(DegradationReason::DynamicSource));
}

#[test]
fn recursive_target_depth_is_parent_depth_plus_one() {
    let op = code_exec(OperandCertainty::Known);
    let d = handle_sink(
        &op,
        SourceLanguage::Bash,
        Some("rm x"),
        provenance(&op),
        "sink",
        None,
        3,
    );
    assert_eq!(d.recursive_target.unwrap().depth, 4);
}

#[test]
fn cross_language_payload_target_uses_payload_language() {
    // A Python sink whose literal payload is JavaScript: the recursive target
    // is parsed as JavaScript, not Python (cross-language nesting, ADR-022 §7).
    let op = code_exec(OperandCertainty::Known);
    let d = handle_sink(
        &op,
        SourceLanguage::JavaScript,
        Some("fs.rmSync('x')"),
        provenance(&op),
        "subprocess.run(['node','-e','fs.rmSync(\\'x\\')'])",
        None,
        0,
    );
    assert_eq!(
        d.recursive_target.unwrap().language,
        SourceLanguage::JavaScript
    );
}

#[test]
fn sink_match_is_code_execution_for_both_literal_and_dynamic() {
    // REVIEW GATE: every recognized process/shell/eval sink retains its
    // CodeExecution Match regardless of payload certainty.
    let lit = handle_sink(
        &code_exec(OperandCertainty::Known),
        SourceLanguage::Bash,
        Some("rm x"),
        provenance(&code_exec(OperandCertainty::Known)),
        "sink",
        None,
        0,
    );
    let dyn_ = handle_sink(
        &code_exec(OperandCertainty::Dynamic),
        SourceLanguage::Bash,
        None,
        provenance(&code_exec(OperandCertainty::Dynamic)),
        "sink",
        None,
        0,
    );
    for (name, d) in [("literal", lit), ("dynamic", dyn_)] {
        assert_eq!(
            d.sink_match.pattern.id.as_ref(),
            "LANG-EXEC",
            "{name} sink must keep the CodeExecution Match",
        );
        assert_eq!(d.sink_match.pattern.risk, RiskLevel::Danger);
    }
}

#[test]
fn degradation_never_lowers_sink_risk() {
    // Degradation is orthogonal to RiskLevel (ADR-022 §5): a degraded sink is
    // still Danger, never lowered to authorize auto-execution.
    let d = handle_sink(
        &code_exec(OperandCertainty::Dynamic),
        SourceLanguage::Bash,
        None,
        provenance(&code_exec(OperandCertainty::Dynamic)),
        "sink",
        None,
        0,
    );
    assert_eq!(d.degradation, Some(DegradationReason::DynamicSource));
    assert_eq!(d.sink_match.pattern.risk, RiskLevel::Danger);
}

#[test]
fn cross_language_literal_payload_is_accepted_by_the_queue() {
    // Composition: a Python sink produces a JavaScript recursive target, which
    // the parent-owned queue accepts (cross-language nesting).
    let op = code_exec(OperandCertainty::Known);
    let d = handle_sink(
        &op,
        SourceLanguage::JavaScript,
        Some("fs.rmSync('x')"),
        provenance(&op),
        "subprocess.run(['node','-e','fs.rmSync(\\'x\\')'])",
        None,
        0,
    );
    let target = d.recursive_target.expect("literal payload → target");

    let mut q = AnalysisQueue::new(QueueBudget::L1_DEFAULT);
    // Root sink target (Python) first, then the nested JS target.
    q.push(QueueTarget::new(
        SourceLanguage::Python,
        "subprocess.run(['node','-e','fs.rmSync(\\'x\\')'])".to_string(),
        0,
    ));
    let outcome = q.push(target);
    assert_eq!(outcome, PushOutcome::Accepted);
    assert_eq!(q.accepted_count(), 2);
}

#[test]
fn dynamic_payload_target_is_not_pushed_to_queue() {
    // The parent never enqueues a dynamic payload: there is no target to push,
    // so the queue stays at its prior state.
    let op = code_exec(OperandCertainty::Dynamic);
    let d = handle_sink(
        &op,
        SourceLanguage::Bash,
        None,
        provenance(&op),
        "subprocess.run(user_input)",
        None,
        0,
    );
    assert!(d.recursive_target.is_none());
    let q = AnalysisQueue::new(QueueBudget::L1_DEFAULT);
    assert_eq!(q.accepted_count(), 0);
}
