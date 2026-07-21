//! Root-side mapping from an adapter's `aegis_language` operation vocabulary
//! into the `aegis_types` analysis vocabulary, the shared classifier, and the
//! cross-language execution-sink invariant (ADR-022 §2/§3/§7, plan Iteration 6).
//!
//! `aegis_language` is a workspace leaf and may not depend on `aegis_types`
//! (pinned by `tests/aegis_language_boundary.rs`), so the operation vocabulary
//! an adapter *produces* is a boundary-forced parallel of `aegis_types::analysis`.
//! This module is the single place that converts the parallel types into the
//! shared types and composes the result:
//!
//! - [`map_operation`] converts one [`aegis_language::operation::DetectedOperation`]
//!   into one [`aegis_types::DetectedOperation`] (span/payload dropped; the span
//!   moves into `AnalysisProvenance`, the payload feeds the recursive sink
//!   invariant). No adapter assigns a final `RiskLevel`; every operation routes
//!   through the shared classifier via [`aegis_types::language_match`]
//!   (Iteration 5 REVIEW GATE).
//! - [`map_adapter_result`] runs the full composition for one analysis target:
//!   each detected operation becomes a `LANG-*` `Match`, a `CodeExecution` sink
//!   with a literal payload additionally enqueues a bounded recursive
//!   [`QueueTarget`], a dynamic/encoded payload records
//!   [`DegradationReason::DynamicSource`], and a nonzero `parse_errors` count
//!   records [`DegradationReason::IncompleteSyntax`]. Status aggregates
//!   monotonically (`NotApplicable < Complete < Degraded`).
//!
//! This is the in-process composition the worker wiring (a later slice) will
//! rely on; it owns no I/O and spawns no subprocess.

use aegis_language::SourceLanguage;
use aegis_language::operation::{
    AdapterResult, ByteSpan as LangSpan, DetectedOperation as LangOp, OperandCertainty as LangCert,
    OperationKind as LangKind, OperationModifiers as LangMods,
};
use aegis_types::{
    AnalysisProvenance, AnalysisStatus, ByteSpan, DegradationReason, DetectedOperation,
    LanguageAnalysisResult, OperandCertainty, OperationKind, OperationModifiers, SourceOrigin,
    language_match,
};

use super::queue::QueueTarget;
use super::recursive::handle_sink;

/// The output of mapping one adapter result into the shared analysis vocabulary.
///
/// `analysis` is the [`LanguageAnalysisResult`] ready for [`aegis_types::merge_analysis`];
/// `recursive_targets` are the bounded nested targets the parent process must
/// enqueue for recursive analysis (ADR-022 §7). The targets are returned
/// separately rather than pushed internally so the parent owns the queue and
/// the depth/budget policy (Iteration 5).
#[derive(Debug, Clone)]
pub struct MappingOutcome {
    /// The language-aware analysis result (status, Matches, degradation reasons).
    pub analysis: LanguageAnalysisResult,
    /// Bounded recursive targets enqueued from literal execution-sink payloads.
    pub recursive_targets: Vec<QueueTarget>,
}

/// Convert one adapter operation into the shared `aegis_types` operation.
///
/// Returns `None` when the adapter emitted an `OperationKind` that has no
/// `aegis_types` counterpart yet. Both vocabularies are `#[non_exhaustive]` and
/// kept in lockstep by the `every_operation_kind_maps_one_for_one` pipeline test;
/// a future adapter kind with no shared mapping is a workspace-internal gap that
/// surfaces there. `None` is handled by [`map_adapter_result`] by skipping the
/// op rather than mislabeling it with a wrong kind or panicking.
///
/// `OperationModifiers` and `OperandCertainty` always map (certainty's
/// `non_exhaustive` wildcard falls back to `Dynamic` — the conservative "never
/// evidence of safety" default, ADR-022 §7). The adapter-local `span` and
/// `payload` are dropped here — the span moves into `AnalysisProvenance` in
/// [`map_adapter_result`], and the payload feeds [`handle_sink`].
#[must_use]
pub fn map_operation(op: &LangOp) -> Option<DetectedOperation> {
    Some(DetectedOperation {
        kind: map_kind(op.kind)?,
        modifiers: map_modifiers(op.modifiers),
        certainty: map_certainty(op.certainty),
    })
}

/// Compose one adapter result into a [`MappingOutcome`] for one analysis target
/// (ADR-022 §2/§3/§7).
///
/// `source` is the analyzed source body — used only to slice the matched-text
/// snippet for each `Match` from the operation's byte span (the snippet is the
/// in-memory `MatchResult.matched_text`, not provenance; provenance carries
/// metadata only, ADR-022 §10). `source_hash` is the optional hex digest of the
/// analyzed bytes, persisted into each operation's provenance. `parent_depth`
/// is the depth of the target being mapped; any literal execution-sink payload
/// becomes a recursive target at `parent_depth + 1`.
///
/// Status aggregation is monotonic: `Degraded` if any degradation reason was
/// recorded, else `Complete` if any Match was produced, else `NotApplicable`.
/// Degradation reasons are deduplicated (the same reason from multiple ops
/// records once).
#[must_use]
pub fn map_adapter_result(
    adapter: &AdapterResult,
    source: &str,
    language: SourceLanguage,
    source_origin: SourceOrigin,
    source_hash: Option<String>,
    parent_depth: u32,
) -> MappingOutcome {
    let mut matches = Vec::new();
    let mut recursive_targets = Vec::new();
    let mut reasons: Vec<DegradationReason> = Vec::new();

    for op in &adapter.operations {
        let Some(mapped) = map_operation(op) else {
            // An adapter kind with no shared mapping yet: skip rather than
            // mislabel or panic (see `map_operation`). The lockstep conversion
            // test prevents this for every currently-defined kind.
            continue;
        };
        let matched_text = source
            .get(op.span.byte_start..op.span.byte_end)
            .unwrap_or_default();
        let span = map_span(op.span);

        if op.kind == LangKind::CodeExecution {
            // The payload's own language (cross-language: a Python eval of a
            // JavaScript literal enqueues a JavaScript target). When no payload
            // was recovered, `handle_sink` ignores `payload_language`; the
            // sink's own language is the least-misleading placeholder.
            let payload_language = op.payload.as_ref().map_or(language, |p| p.language);
            let resolved_payload = op.payload.as_ref().map(|p| p.source.as_str());

            let (status, degradation_reason) = if op.payload.is_some() {
                (AnalysisStatus::Complete, None)
            } else {
                (
                    AnalysisStatus::Degraded,
                    Some(DegradationReason::DynamicSource),
                )
            };
            let provenance = build_provenance(
                &mapped,
                language,
                source_origin,
                source_hash.as_deref(),
                span,
                status,
                degradation_reason,
            );

            let decision = handle_sink(
                &mapped,
                payload_language,
                resolved_payload,
                provenance,
                matched_text,
                None,
                parent_depth,
            );
            matches.push(decision.sink_match);
            if let Some(target) = decision.recursive_target {
                recursive_targets.push(target);
            }
            if let Some(reason) = decision.degradation {
                push_unique(&mut reasons, reason);
            }
        } else {
            // A non-execution op emits its classified Match directly. The shared
            // classifier assigns risk certainty-independently (a Dynamic operand
            // never lowers risk, ADR-022 §3), so the Match stands on its own:
            // no typed degradation. `DegradationReason::DynamicSource` is
            // "source or working directory was dynamic" (ADR-022 §4) — a dynamic
            // *operand* (unresolved path) is neither, and ADR-022 mandates
            // degradation only for dynamic execution-sink payloads (§3/§7) or
            // dynamic source/cwd (§6), not for non-execution dynamic operands.
            let provenance = build_provenance(
                &mapped,
                language,
                source_origin,
                source_hash.as_deref(),
                span,
                AnalysisStatus::Complete,
                None,
            );
            let mr = language_match(&mapped, provenance, matched_text, None);
            matches.push(mr);
        }
    }

    if adapter.parse_errors > 0 {
        push_unique(&mut reasons, DegradationReason::IncompleteSyntax);
    }

    let status = if !reasons.is_empty() {
        AnalysisStatus::Degraded
    } else if !matches.is_empty() {
        AnalysisStatus::Complete
    } else {
        AnalysisStatus::NotApplicable
    };

    MappingOutcome {
        analysis: LanguageAnalysisResult {
            status,
            matches,
            degradation_reasons: reasons,
        },
        recursive_targets,
    }
}

/// Build the metadata-only provenance for one detected operation.
fn build_provenance(
    op: &DetectedOperation,
    language: SourceLanguage,
    source_origin: SourceOrigin,
    source_hash: Option<&str>,
    span: ByteSpan,
    status: AnalysisStatus,
    degradation_reason: Option<DegradationReason>,
) -> AnalysisProvenance {
    AnalysisProvenance {
        language: Some(language.id().to_string()),
        source_origin,
        rule_id: None,
        operation: Some(op.clone()),
        file_path: None,
        source_hash: source_hash.map(str::to_string),
        span: Some(span),
        certainty: op.certainty,
        status,
        degradation_reason,
    }
}

/// Map an adapter byte span into the shared byte span.
fn map_span(span: LangSpan) -> ByteSpan {
    ByteSpan {
        line: span.line,
        column: span.column,
        byte_start: span.byte_start,
        byte_end: span.byte_end,
    }
}

/// Map an adapter operation kind into the shared operation kind (one-for-one).
///
/// Returns `None` for a future `aegis_language` kind with no `aegis_types`
/// counterpart yet (both enums are `#[non_exhaustive]`).
fn map_kind(kind: LangKind) -> Option<OperationKind> {
    match kind {
        LangKind::FilesystemDelete => Some(OperationKind::FilesystemDelete),
        LangKind::FilesystemOverwrite => Some(OperationKind::FilesystemOverwrite),
        LangKind::PermissionOrOwnershipChange => Some(OperationKind::PermissionOrOwnershipChange),
        LangKind::DeviceOrCriticalWrite => Some(OperationKind::DeviceOrCriticalWrite),
        LangKind::DatabaseDestructive => Some(OperationKind::DatabaseDestructive),
        LangKind::CodeExecution => Some(OperationKind::CodeExecution),
        LangKind::CloudDestructive => Some(OperationKind::CloudDestructive),
        LangKind::ContainerDestructive => Some(OperationKind::ContainerDestructive),
        LangKind::PackageDestructive => Some(OperationKind::PackageDestructive),
        _ => None,
    }
}

/// Map adapter modifiers into shared modifiers.
fn map_modifiers(mods: LangMods) -> OperationModifiers {
    OperationModifiers {
        recursive: mods.recursive,
        forced: mods.forced,
        destructive_mode: mods.destructive_mode,
    }
}

/// Map adapter certainty into shared certainty (one-for-one).
///
/// The `non_exhaustive` wildcard falls back to `Dynamic` — the conservative
/// "never evidence of safety" default (ADR-022 §7) — for any future certainty
/// variant this crate does not yet understand.
fn map_certainty(certainty: LangCert) -> OperandCertainty {
    match certainty {
        LangCert::Known => OperandCertainty::Known,
        LangCert::Partial => OperandCertainty::Partial,
        _ => OperandCertainty::Dynamic,
    }
}

/// Push `reason` into `reasons` only if it is not already present (dedup).
fn push_unique(reasons: &mut Vec<DegradationReason>, reason: DegradationReason) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}
