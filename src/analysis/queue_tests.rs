//! RED tests for the parent-owned recursive analysis work queue (plan
//! Iteration 5, Slice 2; ADR-022 §7).
//!
//! Pins: literal-payload recursion, cross-language nesting, duplicate-hash
//! dedup, cycle breaking, the depth-8 ceiling, target-count and
//! aggregate-byte caps, the session deadline, and that every budget cap
//! records `DegradationReason::LimitExceeded` while dedup records none.

use super::{AnalysisQueue, PushOutcome, QueueBudget, QueueTarget};
use aegis_language::SourceLanguage;
use aegis_types::DegradationReason;
use std::time::{Duration, Instant};

fn tgt(lang: SourceLanguage, source: &str, depth: u32) -> QueueTarget {
    QueueTarget::new(lang, source.to_string(), depth)
}

#[test]
fn root_target_is_accepted_and_poppable() {
    let mut q = AnalysisQueue::new(QueueBudget::L1_DEFAULT);
    assert_eq!(
        q.push(tgt(SourceLanguage::Python, "os.remove('a')", 0)),
        PushOutcome::Accepted,
    );
    assert_eq!(q.accepted_count(), 1);
    let popped = q.pop().expect("root target was just pushed");
    assert_eq!(popped.language, SourceLanguage::Python);
    assert!(q.is_empty());
}

#[test]
fn nested_literal_target_recurses() {
    // A root Python target whose analysis produces a nested literal Bash
    // payload → the parent pushes it at depth 1 and the queue accepts it.
    let mut q = AnalysisQueue::new(QueueBudget::L1_DEFAULT);
    q.push(tgt(
        SourceLanguage::Python,
        "subprocess.run(['bash','-c','rm x'])",
        0,
    ));
    let _ = q.pop();
    assert_eq!(
        q.push(tgt(SourceLanguage::Bash, "rm x", 1)),
        PushOutcome::Accepted,
    );
    assert_eq!(q.accepted_count(), 2);
}

#[test]
fn duplicate_target_hash_is_skipped_not_reanalyzed() {
    let mut q = AnalysisQueue::new(QueueBudget::L1_DEFAULT);
    q.push(tgt(SourceLanguage::Python, "os.remove('a')", 0));
    // Same language + same source → same hash → duplicate.
    assert_eq!(
        q.push(tgt(SourceLanguage::Python, "os.remove('a')", 0)),
        PushOutcome::DuplicateSkipped,
    );
    assert_eq!(
        q.accepted_count(),
        1,
        "a duplicate must not inflate the accepted count",
    );
}

#[test]
fn cycle_is_broken_by_dedup() {
    // A target whose analysis produces a nested target with the same
    // (language, hash) as an ancestor is a cycle. Dedup breaks it without
    // degradation.
    let mut q = AnalysisQueue::new(QueueBudget::L1_DEFAULT);
    q.push(tgt(SourceLanguage::Bash, "bash -c 'bash -c x'", 0));
    let _ = q.pop();
    assert_eq!(
        q.push(tgt(SourceLanguage::Bash, "bash -c 'bash -c x'", 1)),
        PushOutcome::DuplicateSkipped,
    );
}

#[test]
fn cross_language_nesting_is_allowed() {
    // Dedup is keyed by (language, hash): identical source bytes under a
    // different language are a distinct target, not a duplicate.
    let mut q = AnalysisQueue::new(QueueBudget::L1_DEFAULT);
    q.push(tgt(SourceLanguage::Python, "shared", 0));
    assert_eq!(
        q.push(tgt(SourceLanguage::Bash, "shared", 1)),
        PushOutcome::Accepted,
    );
    assert_eq!(q.accepted_count(), 2);
}

#[test]
fn depth_ceiling_8_is_enforced() {
    let mut q = AnalysisQueue::new(QueueBudget::L1_DEFAULT);
    // Depth 8 (the ceiling) is accepted.
    assert_eq!(
        q.push(tgt(SourceLanguage::Python, "d8", 8)),
        PushOutcome::Accepted,
    );
    // Depth 9 exceeds the ceiling.
    assert_eq!(
        q.push(tgt(SourceLanguage::Python, "d9", 9)),
        PushOutcome::DepthExceeded,
    );
}

#[test]
fn target_count_cap_is_enforced() {
    let budget = QueueBudget {
        max_targets: 2,
        ..QueueBudget::L1_DEFAULT
    };
    let mut q = AnalysisQueue::new(budget);
    assert_eq!(
        q.push(tgt(SourceLanguage::Python, "a", 0)),
        PushOutcome::Accepted
    );
    assert_eq!(
        q.push(tgt(SourceLanguage::Python, "b", 0)),
        PushOutcome::Accepted
    );
    assert_eq!(
        q.push(tgt(SourceLanguage::Python, "c", 0)),
        PushOutcome::CountExceeded,
    );
}

#[test]
fn aggregate_bytes_cap_is_enforced() {
    let budget = QueueBudget {
        max_aggregate_bytes: 5,
        ..QueueBudget::L1_DEFAULT
    };
    let mut q = AnalysisQueue::new(budget);
    assert_eq!(
        q.push(tgt(SourceLanguage::Python, "aaaa", 0)),
        PushOutcome::Accepted
    );
    // 4 + 3 = 7 > 5.
    assert_eq!(
        q.push(tgt(SourceLanguage::Python, "bbb", 0)),
        PushOutcome::BytesExceeded,
    );
    assert_eq!(q.accepted_bytes(), 4);
}

#[test]
fn deadline_in_past_rejects_push() {
    let past = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("now is well after a 1s offset");
    let budget = QueueBudget {
        deadline: Some(past),
        ..QueueBudget::L1_DEFAULT
    };
    let mut q = AnalysisQueue::new(budget);
    assert_eq!(
        q.push(tgt(SourceLanguage::Python, "x", 0)),
        PushOutcome::DeadlineExceeded,
    );
}

#[test]
fn cap_exceeded_outcomes_map_to_limit_exceeded_degradation() {
    // Every budget cap (depth/count/bytes/deadline) records the same typed
    // degradation bucket (ADR-022 §4): LimitExceeded. Accepted and
    // duplicate-skipped record no degradation.
    assert_eq!(
        PushOutcome::DepthExceeded.degradation_reason(),
        Some(DegradationReason::LimitExceeded),
    );
    assert_eq!(
        PushOutcome::CountExceeded.degradation_reason(),
        Some(DegradationReason::LimitExceeded),
    );
    assert_eq!(
        PushOutcome::BytesExceeded.degradation_reason(),
        Some(DegradationReason::LimitExceeded),
    );
    assert_eq!(
        PushOutcome::DeadlineExceeded.degradation_reason(),
        Some(DegradationReason::LimitExceeded),
    );
    assert_eq!(PushOutcome::DuplicateSkipped.degradation_reason(), None);
    assert_eq!(PushOutcome::Accepted.degradation_reason(), None);
}

#[test]
fn l1_default_budget_matches_adr_022_ceilings() {
    let b = QueueBudget::L1_DEFAULT;
    // Explicit ADR-022 §7 ceilings.
    assert_eq!(b.max_depth, 8, "ADR-022 §7 recursion-depth ceiling is 8");
    assert_eq!(
        b.max_aggregate_bytes,
        1024 * 1024,
        "ADR-022 §7 aggregate-source ceiling is 1 MiB",
    );
    assert!(
        b.deadline.is_none(),
        "deadline is caller-set, not a static default",
    );
    // The target-count ceiling must exceed the depth ceiling so a linear
    // depth-8 chain (9 targets) is bounded by depth, not by count.
    assert!(
        b.max_targets > b.max_depth as usize,
        "max_targets ({}) must exceed max_depth ({}) so depth is the binding recursion guard",
        b.max_targets,
        b.max_depth,
    );
}

#[test]
fn queue_target_hash_matches_source_reader_hex_sha256_format() {
    // The dedup key is the source hash; it must be the same hex SHA-256
    // format `source_reader` records so a BOM-free script-file target and an
    // inline target over the same body collapse (see `QueueTarget.source_hash`
    // doc for the BOM caveat — `source_reader` hashes raw bytes pre-BOM-strip,
    // `QueueTarget` hashes the post-strip `String`).
    let t = QueueTarget::new(SourceLanguage::Python, "abc".to_string(), 0);
    assert_eq!(
        t.source_hash, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "SHA-256 of \"abc\" in lowercase hex",
    );
}

#[test]
fn pop_is_lifo_depth_first() {
    // `pop` is documented LIFO so recursion is depth-first: the most recently
    // pushed target is analyzed before older siblings. A future swap to FIFO
    // would invert the traversal strategy and should fail this test.
    let mut q = AnalysisQueue::new(QueueBudget::L1_DEFAULT);
    q.push(tgt(SourceLanguage::Python, "first", 0));
    q.push(tgt(SourceLanguage::Bash, "second", 0));
    assert_eq!(
        q.pop().expect("two targets were pushed").source,
        "second",
        "LIFO: the last pushed target pops first",
    );
    assert_eq!(q.pop().expect("one target remains").source, "first",);
    assert!(q.is_empty());
}

#[test]
fn cap_rejected_target_does_not_poison_dedup() {
    // A target rejected by a budget cap must not be inserted into `seen`; if
    // it were, an identical target later presented at a valid depth would be
    // wrongly skipped as a duplicate. Depth is the cleanest cap to exercise:
    // reject at depth 9, then accept the same (language, source) at depth 0.
    let mut q = AnalysisQueue::new(QueueBudget::L1_DEFAULT);
    assert_eq!(
        q.push(tgt(SourceLanguage::Python, "rm x", 9)),
        PushOutcome::DepthExceeded,
    );
    assert_eq!(
        q.push(tgt(SourceLanguage::Python, "rm x", 0)),
        PushOutcome::Accepted,
        "a cap-rejected target must not preclude the same source at a valid depth",
    );
    assert_eq!(q.accepted_count(), 1);
}
