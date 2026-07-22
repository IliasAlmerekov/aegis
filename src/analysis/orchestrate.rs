//! Parent-side language analysis orchestration (ADR-022 §2/§6/§7, L1
//! Iteration 6 Slice C).
//!
//! [`run`] routes a command's analyzable source targets, spawns the ephemeral
//! worker, and drains the parent-owned [`AnalysisQueue`]: pop a target →
//! analyze it via one `Request::Analyze` → map the response through the
//! in-process [`map_adapter_result`] → push any recursive targets back onto
//! the queue → repeat until the queue empties or a budget cap fires. Per-target
//! [`LanguageAnalysisResult`]s are folded into the baseline [`Assessment`]
//! via a single aggregated [`merge_analysis`] call.
//!
//! Slice C scope (per ADR-022 plan): recursive drain only. This is NOT wired
//! into `RuntimeContext::assess` and does NOT influence live intercepted
//! Assessments. `ScriptFile`/`DirectExec` fs reads, live-assess integration,
//! and full `aegis-config` budget/alias wiring remain deferred. One worker is
//! spawned per queue pop (≤ `max_targets` spawns per session): `Worker::analyze`
//! closes stdin and reaps the child every session, so worker reuse across pops
//! is a perf optimization deferred to a later slice.

use std::time::Duration;

use aegis_language::SourceLanguage;
use aegis_language::protocol::Response;
use aegis_types::{
    AnalysisStatus, Assessment, DegradationReason, LanguageAnalysisResult, SourceOrigin,
    merge_analysis,
};

use super::mapping::map_adapter_result;
use super::queue::{AnalysisQueue, QueueBudget, QueueTarget};
use super::router::{RoutedTarget, route};
use super::worker_client::{RequestKind, TargetRequest, TargetResult, Worker, WorkerError};

/// The outcome of [`run`]: distinguishes "no analyzable inline targets — no
/// subprocess spawned" from "the worker ran and results were folded in".
#[derive(Debug, Clone)]
pub enum Outcome {
    /// `route` yielded no [`RoutedTarget::Inline`]. No subprocess was spawned;
    /// the `baseline` is returned unchanged (`analysis` still `None`). This is
    /// the ADR-022 §0 contract: language analysis never starts without an
    /// analyzable source target.
    NotStarted {
        /// The baseline `Assessment`, untouched.
        baseline: Assessment,
    },
    /// The worker was spawned and at least one inline target was analyzed.
    /// Per-target [`LanguageAnalysisResult`]s have been folded into `baseline`
    /// via a single aggregated [`merge_analysis`] call.
    Analyzed {
        /// The merged `Assessment` (baseline + aggregated language analysis).
        assessment: Assessment,
        /// The number of targets analyzed, top-level + recursive (one
        /// `Request::Analyze` each, one worker spawn each).
        target_count: usize,
    },
}

/// Route a command's inline source targets, spawn the ephemeral worker, and
/// drain the recursive analysis queue: each popped target is analyzed by a
/// fresh worker, mapped through [`map_adapter_result`], and any literal
/// execution-sink payloads it discovers are pushed back onto the queue for
/// recursive analysis. Per-target results are folded into `baseline`.
///
/// ADR-022 §0 contract: when [`route`] yields no [`RoutedTarget::Inline`]
/// targets, NO subprocess is spawned and `baseline` is returned unchanged via
/// [`Outcome::NotStarted`]. ADR-022 §7 contract: depth, target-count, and
/// aggregate-byte caps record [`DegradationReason::LimitExceeded`] while
/// preserving the Matches already produced.
///
/// `aegis_path` overrides the worker binary (tests pass
/// `Some(env!("CARGO_BIN_EXE_aegis"))`); `None` re-execs `current_exe` (the
/// production path). `trusted_aliases` is forwarded to [`route`]. `deadline`
/// bounds each worker session. The queue budget is [`QueueBudget::L1_DEFAULT`]
/// (config wiring is deferred).
pub async fn run(
    command: &str,
    baseline: &Assessment,
    aegis_path: Option<&str>,
    trusted_aliases: &[(&str, &str)],
    deadline: Duration,
) -> Outcome {
    // Route. Only Inline targets are analyzable without fs I/O this slice;
    // ScriptFile/DirectExec require async fs reads (deferred) and Dynamic
    // already carries a DegradationReason (deferred to live-assess wiring).
    let inline: Vec<(SourceLanguage, String)> = route(command, trusted_aliases)
        .into_iter()
        .filter_map(|t| match t {
            RoutedTarget::Inline { language, source } => Some((language, source)),
            _ => None,
        })
        .collect();

    if inline.is_empty() {
        return Outcome::NotStarted {
            baseline: baseline.clone(),
        };
    }

    let mut queue = AnalysisQueue::new(QueueBudget::L1_DEFAULT);
    let mut per_target: Vec<LanguageAnalysisResult> = Vec::new();

    // Seed the queue with the top-level inline targets at depth 0. Caps never
    // fire under L1_DEFAULT for a handful of inline targets, but a cap is
    // recorded as LimitExceeded rather than silently dropping work (ADR-022 §7).
    for (language, source) in &inline {
        push_with_degradation(
            &mut queue,
            QueueTarget::new(*language, source.clone(), 0),
            &mut per_target,
        );
    }

    // Drain loop: pop → spawn a fresh worker → analyze one target → map the
    // response → enqueue recursive targets → repeat. LIFO order makes this
    // depth-first; the aggregate is order-independent (matches concatenated,
    // reasons deduplicated). One worker per pop: Worker::analyze closes stdin
    // and reaps the child every session, so the worker cannot be reused across
    // pops without refactoring worker_client (deferred perf optimization).
    while let Some(target) = queue.pop() {
        let request = TargetRequest {
            request_id: 1,
            language: target.language,
            source: target.source.as_bytes().to_vec(),
            kind: RequestKind::Analyze,
        };
        let results: Vec<TargetResult> = match Worker::spawn(aegis_path).await {
            Ok(mut worker) => worker.analyze(vec![request], deadline).await,
            // A spawn failure degrades this target as WorkerFailure; the parent
            // never treats a worker failure as evidence of safety (ADR-022 §2).
            Err(_) => vec![TargetResult::Failed(WorkerError::Closed)],
        };
        let result = results
            .into_iter()
            .next()
            .unwrap_or(TargetResult::Failed(WorkerError::Closed));
        let (analysis, recursive_targets) = map_target_result(result, &target);
        per_target.push(analysis);
        for recursive in recursive_targets {
            push_with_degradation(&mut queue, recursive, &mut per_target);
        }
    }

    // Aggregate across every analyzed target (top-level + recursive) into ONE
    // result and merge once: merge_analysis overwrites Assessment.analysis with
    // the latest status/reasons, so a per-target merge would clobber earlier
    // reasons (D3).
    let aggregated = aggregate(&per_target);
    Outcome::Analyzed {
        assessment: merge_analysis(baseline, &aggregated),
        target_count: per_target.len(),
    }
}

/// Push `target` onto `queue`, recording a [`DegradationReason::LimitExceeded`]
/// degradation into `per_target` when a budget cap rejects it (ADR-022 §7:
/// preserve Matches already produced, record the limit). A duplicate or an
/// acceptance records nothing — a duplicate was already analyzed, an
/// acceptance is normal flow.
fn push_with_degradation(
    queue: &mut AnalysisQueue,
    target: QueueTarget,
    per_target: &mut Vec<LanguageAnalysisResult>,
) {
    if let Some(reason) = queue.push(target).degradation_reason() {
        per_target.push(LanguageAnalysisResult {
            status: AnalysisStatus::Degraded,
            matches: Vec::new(),
            degradation_reasons: vec![reason],
        });
    }
}

/// Map one worker [`TargetResult`] into a per-target [`LanguageAnalysisResult`]
/// and the recursive targets its literal execution-sink payloads produced.
///
/// - `Responded(Analyzed{result})` → [`map_adapter_result`] with the target's
///   own `depth` and `source_hash` so recursive targets land at `depth + 1`
///   with correct provenance. Returns `(analysis, recursive_targets)`.
/// - `Responded(UnsupportedLanguage)` → `Degraded` with `[GrammarUnavailable]`.
/// - `Responded(Parsed|ParseFailed)` (defensive — unreachable when sending
///   `Analyze`) → `Degraded` with `[WorkerFailure]`.
/// - `Failed(WorkerError)` → `Degraded` with `[WorkerFailure]`.
fn map_target_result(
    result: TargetResult,
    target: &QueueTarget,
) -> (LanguageAnalysisResult, Vec<QueueTarget>) {
    match result {
        TargetResult::Responded(Response::Analyzed { result }) => {
            let outcome = map_adapter_result(
                &result,
                &target.source,
                target.language,
                SourceOrigin::Inline,
                Some(target.source_hash.clone()),
                target.depth,
            );
            (outcome.analysis, outcome.recursive_targets)
        }
        TargetResult::Responded(Response::UnsupportedLanguage) => (
            LanguageAnalysisResult {
                status: AnalysisStatus::Degraded,
                matches: Vec::new(),
                degradation_reasons: vec![DegradationReason::GrammarUnavailable],
            },
            Vec::new(),
        ),
        // Defensive: Parsed/ParseFailed are unreachable when sending Analyze,
        // since the worker routes Analyze to Analyzed/UnsupportedLanguage only.
        TargetResult::Responded(_) => (
            LanguageAnalysisResult {
                status: AnalysisStatus::Degraded,
                matches: Vec::new(),
                degradation_reasons: vec![DegradationReason::WorkerFailure],
            },
            Vec::new(),
        ),
        TargetResult::Failed(err) => (
            LanguageAnalysisResult {
                status: AnalysisStatus::Degraded,
                matches: Vec::new(),
                degradation_reasons: vec![DegradationReason::from(err)],
            },
            Vec::new(),
        ),
    }
}

/// Aggregate per-target results into ONE [`LanguageAnalysisResult`] for a
/// single [`merge_analysis`] call.
///
/// Matches are concatenated; degradation reasons are deduplicated across
/// targets; status is `Degraded` if any target degraded, else `Complete` if
/// any target produced a match, else `NotApplicable`. This preserves earlier
/// targets' reasons rather than letting a per-target `merge_analysis` clobber
/// them (D3).
fn aggregate(per_target: &[LanguageAnalysisResult]) -> LanguageAnalysisResult {
    let mut matches = Vec::new();
    let mut reasons: Vec<DegradationReason> = Vec::new();
    let mut any_degraded = false;
    for r in per_target {
        matches.extend(r.matches.iter().cloned());
        for reason in &r.degradation_reasons {
            if !reasons.contains(reason) {
                reasons.push(*reason);
            }
        }
        if r.status == AnalysisStatus::Degraded {
            any_degraded = true;
        }
    }
    let status = if any_degraded {
        AnalysisStatus::Degraded
    } else if !matches.is_empty() {
        AnalysisStatus::Complete
    } else {
        AnalysisStatus::NotApplicable
    };
    LanguageAnalysisResult {
        status,
        matches,
        degradation_reasons: reasons,
    }
}
