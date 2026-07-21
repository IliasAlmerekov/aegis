//! RED tests for the shared language-aware operation classifier (plan
//! Iteration 5, Slice 1). The classifier is the single place that maps a
//! `DetectedOperation` into the existing `Category` / `RiskLevel` / `Match`
//! vocabulary (ADR-022 §3), so no adapter may assign a final `RiskLevel`
//! directly (Iteration 5 REVIEW GATE).
//!
//! These tests are written before the implementation exists. They pin:
//! - the language-neutral matrix over every `OperationKind` and the
//!   recursive/forced/destructive-mode modifiers;
//! - the REVIEW GATE invariants — a `Dynamic` operand never lowers risk,
//!   `CodeExecution` always emits a `Danger` `Match`, and the classifier
//!   never assigns `RiskLevel::Block` (language-aware Matches are non-`Block`
//!   by ADR-022 §5);
//! - the `Match` builder carrying `MatchEvidence::LanguageRule` with the
//!   operation and metadata-only provenance, always built in.

use super::classifier::{classify, language_match};
use super::{
    AnalysisProvenance, AnalysisStatus, ByteSpan, DetectedOperation, DetectionMechanism,
    DetectionSource, MatchEvidence, OperandCertainty, OperationKind, OperationModifiers,
    SourceOrigin,
};
use crate::{Category, HighlightRange, MatchResult, PatternSource, RiskLevel};

/// Shorthand: a `DetectedOperation` with the given kind, no modifiers, and
/// `Known` certainty.
fn op(kind: OperationKind) -> DetectedOperation {
    DetectedOperation {
        kind,
        modifiers: OperationModifiers::default(),
        certainty: OperandCertainty::Known,
    }
}

fn op_with(kind: OperationKind, modifiers: OperationModifiers) -> DetectedOperation {
    DetectedOperation {
        kind,
        modifiers,
        certainty: OperandCertainty::Known,
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

// ── Language-neutral matrix: kind + modifiers → (Category, RiskLevel, id) ──

#[test]
fn classify_filesystem_delete_single_is_warn() {
    let c = classify(&op(OperationKind::FilesystemDelete));
    assert_eq!(c.category, Category::Filesystem);
    assert_eq!(c.risk, RiskLevel::Warn);
    assert_eq!(c.rule_id, "LANG-FS-DEL");
}

#[test]
fn classify_filesystem_delete_recursive_is_danger() {
    let c = classify(&op_with(
        OperationKind::FilesystemDelete,
        OperationModifiers {
            recursive: true,
            ..Default::default()
        },
    ));
    assert_eq!(c.risk, RiskLevel::Danger);
    assert_eq!(c.rule_id, "LANG-FS-DEL-R");
}

#[test]
fn classify_filesystem_delete_forced_is_warn() {
    let c = classify(&op_with(
        OperationKind::FilesystemDelete,
        OperationModifiers {
            forced: true,
            ..Default::default()
        },
    ));
    assert_eq!(c.risk, RiskLevel::Warn);
    assert_eq!(c.rule_id, "LANG-FS-DEL-F");
}

#[test]
fn classify_filesystem_delete_recursive_forced_is_danger() {
    let c = classify(&op_with(
        OperationKind::FilesystemDelete,
        OperationModifiers {
            recursive: true,
            forced: true,
            ..Default::default()
        },
    ));
    assert_eq!(c.risk, RiskLevel::Danger);
    assert_eq!(c.rule_id, "LANG-FS-DEL-RF");
}

#[test]
fn classify_filesystem_overwrite_is_warn() {
    let c = classify(&op(OperationKind::FilesystemOverwrite));
    assert_eq!(c.category, Category::Filesystem);
    assert_eq!(c.risk, RiskLevel::Warn);
    assert_eq!(c.rule_id, "LANG-FS-OVR");
}

#[test]
fn classify_filesystem_overwrite_destructive_mode_keeps_separate_rule_id() {
    let c = classify(&op_with(
        OperationKind::FilesystemOverwrite,
        OperationModifiers {
            destructive_mode: true,
            ..Default::default()
        },
    ));
    assert_eq!(c.risk, RiskLevel::Warn);
    assert_eq!(c.rule_id, "LANG-FS-OVR-W");
}

#[test]
fn classify_permission_or_ownership_change_is_danger() {
    let c = classify(&op(OperationKind::PermissionOrOwnershipChange));
    assert_eq!(c.category, Category::Filesystem);
    assert_eq!(c.risk, RiskLevel::Danger);
    assert_eq!(c.rule_id, "LANG-FS-CHMOD");
}

#[test]
fn classify_device_or_critical_write_is_danger() {
    let c = classify(&op(OperationKind::DeviceOrCriticalWrite));
    assert_eq!(c.category, Category::Filesystem);
    assert_eq!(c.risk, RiskLevel::Danger);
    assert_eq!(c.rule_id, "LANG-FS-DEV");
}

#[test]
fn classify_database_destructive_is_danger() {
    let c = classify(&op(OperationKind::DatabaseDestructive));
    assert_eq!(c.category, Category::Database);
    assert_eq!(c.risk, RiskLevel::Danger);
    assert_eq!(c.rule_id, "LANG-DB-DEST");
}

#[test]
fn classify_code_execution_is_danger_process_category() {
    let c = classify(&op(OperationKind::CodeExecution));
    assert_eq!(c.category, Category::Process);
    assert_eq!(c.risk, RiskLevel::Danger);
    assert_eq!(c.rule_id, "LANG-EXEC");
}

#[test]
fn classify_cloud_destructive_is_danger() {
    let c = classify(&op(OperationKind::CloudDestructive));
    assert_eq!(c.category, Category::Cloud);
    assert_eq!(c.risk, RiskLevel::Danger);
    assert_eq!(c.rule_id, "LANG-CLOUD-DEST");
}

#[test]
fn classify_container_destructive_is_danger() {
    let c = classify(&op(OperationKind::ContainerDestructive));
    assert_eq!(c.category, Category::Docker);
    assert_eq!(c.risk, RiskLevel::Danger);
    assert_eq!(c.rule_id, "LANG-DOCKER-DEST");
}

#[test]
fn classify_package_destructive_is_warn() {
    let c = classify(&op(OperationKind::PackageDestructive));
    assert_eq!(c.category, Category::Package);
    assert_eq!(c.risk, RiskLevel::Warn);
    assert_eq!(c.rule_id, "LANG-PKG-DEST");
}

// ── REVIEW GATE: the classifier never assigns Block ──────────────────────
//
// ADR-022 §5: an intrinsic `Block` remains unbypassable, and the Strict
// Analysis override applies only to a non-`Block` language-aware Match. So a
// language-aware rule may never classify to `Block` — that tier is reserved
// for intrinsic shell-level denials.

#[test]
fn classify_never_returns_block_over_all_kinds_and_modifiers() {
    let kinds = [
        OperationKind::FilesystemDelete,
        OperationKind::FilesystemOverwrite,
        OperationKind::PermissionOrOwnershipChange,
        OperationKind::DeviceOrCriticalWrite,
        OperationKind::DatabaseDestructive,
        OperationKind::CodeExecution,
        OperationKind::CloudDestructive,
        OperationKind::ContainerDestructive,
        OperationKind::PackageDestructive,
    ];
    let modifier_sets = [
        OperationModifiers::default(),
        OperationModifiers {
            recursive: true,
            ..Default::default()
        },
        OperationModifiers {
            forced: true,
            ..Default::default()
        },
        OperationModifiers {
            destructive_mode: true,
            ..Default::default()
        },
        OperationModifiers {
            recursive: true,
            forced: true,
            destructive_mode: true,
        },
    ];
    for &kind in &kinds {
        for &mods in &modifier_sets {
            let c = classify(&DetectedOperation {
                kind,
                modifiers: mods,
                certainty: OperandCertainty::Known,
            });
            assert_ne!(
                c.risk,
                RiskLevel::Block,
                "language-aware classify must never return Block for {kind:?} {mods:?}",
            );
        }
    }
}

// ── REVIEW GATE: a Dynamic operand is never evidence of safety ───────────
//
// ADR-022 §3/§7: certainty governs whether a recursive target is enqueued and
// whether degradation is recorded — it never lowers the operation's own risk.
// The classifier's risk mapping is therefore certainty-independent, and a
// `Dynamic` operand classifies at *least* as high as a `Known` one.

#[test]
fn classify_risk_is_identical_across_known_partial_dynamic() {
    for kind in [
        OperationKind::FilesystemDelete,
        OperationKind::FilesystemOverwrite,
        OperationKind::PermissionOrOwnershipChange,
        OperationKind::DeviceOrCriticalWrite,
        OperationKind::DatabaseDestructive,
        OperationKind::CodeExecution,
        OperationKind::CloudDestructive,
        OperationKind::ContainerDestructive,
        OperationKind::PackageDestructive,
    ] {
        let known = classify(&DetectedOperation {
            kind,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Known,
        });
        let partial = classify(&DetectedOperation {
            kind,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Partial,
        });
        let dynamic = classify(&DetectedOperation {
            kind,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Dynamic,
        });
        assert_eq!(
            known.risk, partial.risk,
            "Partial must not lower risk for {kind:?}"
        );
        assert_eq!(
            known.risk, dynamic.risk,
            "Dynamic must not lower risk for {kind:?}"
        );
    }
}

#[test]
fn code_execution_always_classifies_danger_regardless_of_certainty() {
    for certainty in [
        OperandCertainty::Known,
        OperandCertainty::Partial,
        OperandCertainty::Dynamic,
    ] {
        let c = classify(&DetectedOperation {
            kind: OperationKind::CodeExecution,
            modifiers: OperationModifiers::default(),
            certainty,
        });
        assert_eq!(
            c.risk,
            RiskLevel::Danger,
            "CodeExecution must stay Danger at {certainty:?}"
        );
    }
}

#[test]
fn classify_is_deterministic() {
    let a = classify(&op(OperationKind::DatabaseDestructive));
    let b = classify(&op(OperationKind::DatabaseDestructive));
    assert_eq!(a, b);
}

// ── safe_alt guidance is present for the high-impact operations ───────────

#[test]
fn classify_offers_safe_alt_for_recursive_delete_and_code_execution() {
    let del = classify(&op_with(
        OperationKind::FilesystemDelete,
        OperationModifiers {
            recursive: true,
            ..Default::default()
        },
    ));
    assert!(
        del.safe_alt.is_some(),
        "recursive delete needs a safer alternative"
    );

    let exec = classify(&op(OperationKind::CodeExecution));
    assert!(
        exec.safe_alt.is_some(),
        "code execution needs a safer alternative"
    );
}

// ── Match builder: language_match → MatchResult with LanguageRule evidence ─

#[test]
fn language_match_builds_matchresult_with_language_rule_evidence() {
    let operation = op(OperationKind::FilesystemDelete);
    let pv = provenance(&operation);
    let m: MatchResult = language_match(
        &operation,
        pv.clone(),
        "os.remove(path)",
        Some(HighlightRange { start: 0, end: 16 }),
    );

    assert_eq!(m.matched_text, "os.remove(path)");
    assert_eq!(
        m.highlight_range,
        Some(HighlightRange { start: 0, end: 16 })
    );
    assert_eq!(m.evidence.mechanism(), DetectionMechanism::LanguageRule);
    match &m.evidence {
        MatchEvidence::LanguageRule {
            source,
            operation,
            provenance,
        } => {
            assert_eq!(*source, DetectionSource::Builtin);
            assert_eq!(operation.kind, OperationKind::FilesystemDelete);
            assert_eq!(provenance, &pv);
        }
        other => panic!("expected LanguageRule evidence, got {other:?}"),
    }
}

#[test]
fn language_match_pattern_is_builtin_and_carries_classification() {
    let operation = op_with(
        OperationKind::FilesystemDelete,
        OperationModifiers {
            recursive: true,
            ..Default::default()
        },
    );
    let m = language_match(&operation, provenance(&operation), "shutil.rmtree(x)", None);

    assert_eq!(m.pattern.source, PatternSource::Builtin);
    assert_eq!(m.pattern.id.as_ref(), "LANG-FS-DEL-R");
    assert_eq!(m.pattern.category, Category::Filesystem);
    assert_eq!(m.pattern.risk, RiskLevel::Danger);
    assert!(!m.pattern.description.is_empty());
}

#[test]
fn language_match_rule_id_is_stable_across_calls() {
    // The detection rule id is a stable identifier (ADR-022 §10) — two matches
    // for the same operation kind + modifiers carry the same id, so audit and
    // override logic can key on it.
    let a = language_match(
        &op(OperationKind::CodeExecution),
        provenance(&op(OperationKind::CodeExecution)),
        "subprocess.run(x)",
        None,
    );
    let b = language_match(
        &op(OperationKind::CodeExecution),
        provenance(&op(OperationKind::CodeExecution)),
        "eval(x)",
        None,
    );
    assert_eq!(a.pattern.id, b.pattern.id);
    assert_eq!(a.pattern.id.as_ref(), "LANG-EXEC");
}
