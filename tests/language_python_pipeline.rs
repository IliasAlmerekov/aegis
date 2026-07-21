//! In-process pipeline test for the Python language adapter → root mapping →
//! `LanguageAnalysisResult` (plan Iteration 6, ADR-022 §2/§3/§7).
//!
//! This is the seam where the boundary-forced parallel operation vocabulary in
//! `aegis_language::operation` is converted into the `aegis_types::analysis`
//! vocabulary and run through the shared classifier (`classify` / `language_match`)
//! and the cross-language execution-sink invariant (`handle_sink`). No worker
//! subprocess is involved — the adapter runs in-process and the root mapping
//! composes its output directly, so the test pins the contract the worker
//! wiring will later rely on.
//!
//! Invariants pinned here:
//! - Every `aegis_language::OperationKind` variant maps one-for-one onto its
//!   `aegis_types::OperationKind` counterpart, preserving modifiers and certainty
//!   (the `operation.rs` doc promises this is pinned by a root-crate test).
//! - A `CodeExecution` sink with a statically recovered literal payload emits a
//!   `LANG-EXEC` `Danger` Match **and** a bounded recursive `QueueTarget` at
//!   `parent_depth + 1`, parsed as the payload's own language (cross-language).
//! - A `CodeExecution` sink with a dynamic payload emits the `LANG-EXEC` Match
//!   plus `DegradationReason::DynamicSource` and **no** recursive target.
//! - A non-execution destructive op emits its classified Match with no target.
//! - A nonzero `parse_errors` count records `IncompleteSyntax` degradation.
//! - Status aggregates monotonically: `NotApplicable < Complete < Degraded`.
//! - The recursive target is accepted by the parent-owned `AnalysisQueue`.
//! - `merge_analysis` lifts a baseline `Safe` `Assessment` to the language
//!   `Match`'s risk and carries the analysis status onto `Assessment.analysis`.

use aegis::analysis::mapping::{map_adapter_result, map_operation};
use aegis::analysis::queue::{AnalysisQueue, PushOutcome, QueueBudget, QueueTarget};
use aegis_language::SourceLanguage;
use aegis_language::languages::python::analyze;
use aegis_language::operation::{
    AdapterResult, ByteSpan as LangSpan, DetectedOperation as LangOp, OperandCertainty as LangCert,
    OperationKind as LangKind, OperationModifiers as LangMods,
};
use aegis_types::{
    AnalysisStatus, Assessment, Category, DegradationReason, DetectionMechanism,
    OperandCertainty as TypesCert, OperationKind as TypesKind, ParsedCommand, RiskLevel,
    SourceOrigin, merge_analysis,
};

/// A throwaway `Assessment` baseline (Safe, no matches) for the merge test.
fn safe_baseline() -> Assessment {
    Assessment {
        risk: RiskLevel::Safe,
        effect_opaque: false,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: ParsedCommand {
            program: Some("python3".to_string()),
            argv: Vec::new(),
            normalized: "python3".to_string(),
            inline_scripts: Vec::new(),
            raw: "python3".to_string(),
        },
        analysis: None,
    }
}

fn map_python(source: &str) -> aegis::analysis::mapping::MappingOutcome {
    let adapter = analyze(source);
    map_adapter_result(
        &adapter,
        source,
        SourceLanguage::Python,
        SourceOrigin::Inline,
        None,
        0,
    )
}

#[test]
fn every_operation_kind_maps_one_for_one() {
    // The boundary-forced parallel vocabulary must convert losslessly: each
    // aegis-language kind → its aegis-types counterpart, modifiers and certainty
    // preserved. This is the conversion test the `operation.rs` doc promises.
    let cases: [(LangKind, TypesKind); 9] = [
        (LangKind::FilesystemDelete, TypesKind::FilesystemDelete),
        (
            LangKind::FilesystemOverwrite,
            TypesKind::FilesystemOverwrite,
        ),
        (
            LangKind::PermissionOrOwnershipChange,
            TypesKind::PermissionOrOwnershipChange,
        ),
        (
            LangKind::DeviceOrCriticalWrite,
            TypesKind::DeviceOrCriticalWrite,
        ),
        (
            LangKind::DatabaseDestructive,
            TypesKind::DatabaseDestructive,
        ),
        (LangKind::CodeExecution, TypesKind::CodeExecution),
        (LangKind::CloudDestructive, TypesKind::CloudDestructive),
        (
            LangKind::ContainerDestructive,
            TypesKind::ContainerDestructive,
        ),
        (LangKind::PackageDestructive, TypesKind::PackageDestructive),
    ];
    for (lang_kind, expected) in cases {
        let lang_op = LangOp {
            kind: lang_kind,
            modifiers: LangMods {
                recursive: true,
                forced: true,
                destructive_mode: true,
            },
            certainty: LangCert::Known,
            span: LangSpan {
                line: 1,
                column: 1,
                byte_start: 0,
                byte_end: 4,
            },
            payload: None,
        };
        let mapped = map_operation(&lang_op).expect("every known kind maps to Some");
        assert_eq!(
            mapped.kind, expected,
            "OperationKind {:?} must map one-for-one",
            lang_kind,
        );
        assert!(
            mapped.modifiers.recursive,
            "{:?}: recursive preserved",
            lang_kind
        );
        assert!(mapped.modifiers.forced, "{:?}: forced preserved", lang_kind);
        assert!(
            mapped.modifiers.destructive_mode,
            "{:?}: destructive_mode preserved",
            lang_kind,
        );
        assert_eq!(
            mapped.certainty,
            TypesCert::Known,
            "{:?}: certainty preserved",
            lang_kind,
        );
    }
}

#[test]
fn known_exec_payload_emits_match_and_cross_language_recursive_target() {
    // os.system("rm -rf /tmp/x"): a Python execution sink whose literal payload
    // is shell source → LANG-EXEC Danger Match + a recursive target parsed as
    // Bash at depth 1 (ADR-022 §7 cross-language nesting). No degradation.
    let outcome = map_python("os.system(\"rm -rf /tmp/x\")");

    let exec = outcome
        .analysis
        .matches
        .iter()
        .find(|m| m.pattern.id.as_ref() == "LANG-EXEC")
        .expect("a CodeExecution sink emits a LANG-EXEC Match");
    assert_eq!(exec.pattern.risk, RiskLevel::Danger);
    assert_eq!(exec.pattern.category, Category::Process);
    assert_eq!(exec.evidence.mechanism(), DetectionMechanism::LanguageRule,);

    let target = outcome
        .recursive_targets
        .first()
        .expect("a literal payload enqueues a recursive target");
    assert_eq!(
        target.language,
        SourceLanguage::Bash,
        "cross-language payload"
    );
    assert_eq!(
        target.depth, 1,
        "root sink at depth 0 → payload target at depth 1"
    );
    assert_eq!(target.source, "rm -rf /tmp/x");

    assert!(
        outcome.analysis.degradation_reasons.is_empty(),
        "a literal payload records no degradation",
    );
    assert_eq!(outcome.analysis.status, AnalysisStatus::Complete);
}

#[test]
fn dynamic_exec_payload_emits_match_and_degradation_without_target() {
    // subprocess.run(cmd): a dynamic payload is never evaluated or decoded
    // (ADR-022 §7). The sink still emits its LANG-EXEC Match, but records
    // DynamicSource degradation and enqueues no recursive target.
    let outcome = map_python("subprocess.run(cmd)");

    assert_eq!(
        outcome
            .analysis
            .matches
            .iter()
            .filter(|m| m.pattern.id.as_ref() == "LANG-EXEC")
            .count(),
        1,
        "a dynamic sink keeps its CodeExecution Match",
    );
    assert!(
        outcome.recursive_targets.is_empty(),
        "a dynamic payload must not be enqueued",
    );
    assert!(
        outcome
            .analysis
            .degradation_reasons
            .contains(&DegradationReason::DynamicSource),
        "a dynamic payload records DynamicSource degradation",
    );
    assert_eq!(outcome.analysis.status, AnalysisStatus::Degraded);
}

#[test]
fn filesystem_delete_emits_match_without_target_or_degradation() {
    // os.remove('x'): a non-execution destructive op emits its classified Match
    // (LANG-FS-DEL, Warn) with no recursive target and no degradation.
    let outcome = map_python("os.remove('x')");

    let del = outcome
        .analysis
        .matches
        .iter()
        .find(|m| m.pattern.id.as_ref() == "LANG-FS-DEL")
        .expect("os.remove emits a filesystem-delete Match");
    assert_eq!(del.pattern.risk, RiskLevel::Warn);
    assert_eq!(del.pattern.category, Category::Filesystem);
    assert!(
        outcome.recursive_targets.is_empty(),
        "a non-execution op enqueues no target",
    );
    assert!(
        outcome.analysis.degradation_reasons.is_empty(),
        "a Known non-execution op records no degradation",
    );
    assert_eq!(outcome.analysis.status, AnalysisStatus::Complete);
}

#[test]
fn parse_errors_record_incomplete_syntax_degradation() {
    // A malformed parse (nonzero ERROR nodes) records IncompleteSyntax
    // degradation. The detected op's Match is still retained (ADR-022 §5:
    // degradation is orthogonal to risk and never drops a Match).
    let op = LangOp {
        kind: LangKind::FilesystemDelete,
        modifiers: LangMods::default(),
        certainty: LangCert::Known,
        span: LangSpan {
            line: 1,
            column: 1,
            byte_start: 0,
            byte_end: 13,
        },
        payload: None,
    };
    let adapter = AdapterResult {
        operations: vec![op],
        parse_errors: 1,
    };
    let outcome = map_adapter_result(
        &adapter,
        "os.remove('x')",
        SourceLanguage::Python,
        SourceOrigin::Inline,
        None,
        0,
    );

    assert!(
        outcome
            .analysis
            .degradation_reasons
            .contains(&DegradationReason::IncompleteSyntax),
        "parse errors record IncompleteSyntax degradation",
    );
    assert_eq!(outcome.analysis.status, AnalysisStatus::Degraded);
    assert!(
        outcome
            .analysis
            .matches
            .iter()
            .any(|m| m.pattern.id.as_ref() == "LANG-FS-DEL"),
        "the detected op's Match is retained despite the parse error",
    );
}

#[test]
fn empty_adapter_result_is_not_applicable() {
    // No operations and no parse errors → analysis does not apply to this target.
    let outcome = map_adapter_result(
        &AdapterResult::default(),
        "print(1)",
        SourceLanguage::Python,
        SourceOrigin::Inline,
        None,
        0,
    );
    assert_eq!(outcome.analysis.status, AnalysisStatus::NotApplicable);
    assert!(outcome.analysis.matches.is_empty());
    assert!(outcome.recursive_targets.is_empty());
    assert!(outcome.analysis.degradation_reasons.is_empty());
}

#[test]
fn recursive_target_is_accepted_by_parent_queue() {
    // Composition: the recursive target the mapping produces is shaped exactly
    // for the parent-owned AnalysisQueue (cross-language nesting accepted).
    let outcome = map_python("os.system(\"rm -rf /tmp/x\")");
    let target = outcome
        .recursive_targets
        .into_iter()
        .next()
        .expect("literal payload → recursive target");

    let mut q = AnalysisQueue::new(QueueBudget::L1_DEFAULT);
    // Root sink target (Python) first, then the nested Bash target.
    q.push(QueueTarget::new(
        SourceLanguage::Python,
        "os.system(\"rm -rf /tmp/x\")".to_string(),
        0,
    ));
    let outcome = q.push(target);
    assert_eq!(outcome, PushOutcome::Accepted);
    assert_eq!(q.accepted_count(), 2);
}

#[test]
fn merge_lifts_baseline_safe_to_danger_and_carries_status() {
    // merge_analysis (ADR-022 §1/§5): a baseline Safe Assessment + a language
    // result with a Danger LANG-EXEC Match → merged risk Danger, the language
    // Match is appended, and the analysis status is carried onto Assessment.
    let outcome = map_python("subprocess.run(cmd)");
    let merged = merge_analysis(&safe_baseline(), &outcome.analysis);

    assert_eq!(
        merged.risk,
        RiskLevel::Danger,
        "risk never decreases: a Danger language Match lifts Safe → Danger",
    );
    assert!(
        merged
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "LANG-EXEC"),
        "the language Match is appended to the merged Assessment",
    );
    let summary = merged
        .analysis
        .as_ref()
        .expect("merge carries the analysis summary");
    assert_eq!(summary.status, AnalysisStatus::Degraded);
    assert!(
        summary
            .degradation_reasons
            .contains(&DegradationReason::DynamicSource),
    );
}

#[test]
fn non_exec_dynamic_operand_emits_match_without_degradation() {
    // ADR-022 §3/§4/§7: typed degradation is mandated for dynamic *execution-
    // sink payloads* and for dynamic *source/cwd* — NOT for a non-execution op
    // whose operand is a variable. os.remove(path): the path is dynamic, but
    // the shared classifier already assigns the correct risk certainty-
    // independently (a Dynamic operand never lowers risk, ADR-022 §3), so the
    // operation's Match stands on its own with NO DynamicSource degradation
    // (that variant is "source or working directory was dynamic", not "operand
    // was dynamic"). Status is Complete, not Degraded.
    let outcome = map_python("os.remove(path)");

    assert!(
        outcome
            .analysis
            .matches
            .iter()
            .any(|m| m.pattern.id.as_ref() == "LANG-FS-DEL"),
        "a dynamic-operand filesystem delete still emits its Match",
    );
    assert!(
        !outcome
            .analysis
            .degradation_reasons
            .contains(&DegradationReason::DynamicSource),
        "a non-execution dynamic operand must NOT record DynamicSource",
    );
    assert!(
        outcome.analysis.degradation_reasons.is_empty(),
        "a non-execution dynamic operand records no degradation",
    );
    assert_eq!(outcome.analysis.status, AnalysisStatus::Complete);
    assert!(
        outcome.recursive_targets.is_empty(),
        "a non-execution op enqueues no recursive target",
    );
}
