//! Monotonic merge tests for [`super::merge_analysis`] (plan Iteration 1 RED
//! #3): risk cannot decrease, Matches cannot disappear, and degradation is
//! carried without lowering risk. Split out of `analysis.rs` to stay under
//! the 800-line file budget (M5.1 quality gate).

use std::borrow::Cow;
use std::sync::Arc;

use super::*;
use crate::assessment::{Assessment, HighlightRange, MatchResult};
use crate::command::ParsedCommand;
use crate::pattern::{Category, Pattern, PatternSource};
use crate::risk::RiskLevel;

fn pattern(id: &str, risk: RiskLevel) -> Arc<Pattern> {
    Arc::new(Pattern {
        id: Cow::Owned(id.to_string()),
        category: Category::Filesystem,
        risk,
        pattern: Cow::Borrowed(""),
        description: Cow::Borrowed("test pattern"),
        safe_alt: None,
        justification: None,
        source: PatternSource::Builtin,
    })
}

/// A scanner-shaped (regex) `Match` for the baseline side.
fn baseline_match(id: &str, risk: RiskLevel) -> MatchResult {
    MatchResult {
        pattern: pattern(id, risk),
        matched_text: String::new(),
        highlight_range: None,
        evidence: MatchEvidence::RegexPattern {
            source: DetectionSource::Builtin,
        },
    }
}

/// A language-aware `Match` (carries `MatchEvidence::LanguageRule`).
fn language_match(id: &str, risk: RiskLevel) -> MatchResult {
    MatchResult {
        pattern: pattern(id, risk),
        matched_text: String::new(),
        highlight_range: None,
        evidence: MatchEvidence::LanguageRule {
            source: DetectionSource::Builtin,
            operation: DetectedOperation {
                kind: OperationKind::FilesystemDelete,
                modifiers: OperationModifiers::default(),
                certainty: OperandCertainty::Known,
            },
            provenance: AnalysisProvenance {
                language: Some("python".to_string()),
                source_origin: SourceOrigin::Inline,
                rule_id: Some(id.to_string()),
                operation: None,
                file_path: None,
                source_hash: None,
                span: None,
                certainty: OperandCertainty::Known,
                status: AnalysisStatus::Complete,
                degradation_reason: None,
            },
        },
    }
}

fn empty_command() -> ParsedCommand {
    ParsedCommand {
        program: Some("echo".to_string()),
        argv: Vec::new(),
        normalized: "echo".to_string(),
        inline_scripts: Vec::new(),
        raw: "echo".to_string(),
    }
}

fn baseline(risk: RiskLevel, matched: Vec<MatchResult>) -> Assessment {
    Assessment {
        risk,
        effect_opaque: false,
        matched,
        highlight_ranges: Vec::<HighlightRange>::new(),
        command: empty_command(),
        analysis: None,
    }
}

fn language(
    status: AnalysisStatus,
    matches: Vec<MatchResult>,
    reasons: Vec<DegradationReason>,
) -> LanguageAnalysisResult {
    LanguageAnalysisResult {
        status,
        matches,
        degradation_reasons: reasons,
    }
}

fn ids(a: &Assessment) -> Vec<String> {
    a.matched.iter().map(|m| m.pattern.id.to_string()).collect()
}

#[test]
fn merge_risk_never_decreases() {
    // Baseline Danger + lower-risk language match → stays Danger (≥ baseline).
    let b = baseline(
        RiskLevel::Danger,
        vec![baseline_match("FS-001", RiskLevel::Danger)],
    );
    let l = language(
        AnalysisStatus::Complete,
        vec![language_match("PY-001", RiskLevel::Warn)],
        vec![],
    );
    assert_eq!(merge_analysis(&b, &l).risk, RiskLevel::Danger);

    // Baseline Safe + Danger language match → rises to Danger (≥ baseline).
    let b = baseline(RiskLevel::Safe, vec![]);
    let l = language(
        AnalysisStatus::Complete,
        vec![language_match("PY-001", RiskLevel::Danger)],
        vec![],
    );
    let merged = merge_analysis(&b, &l);
    assert!(merged.risk >= RiskLevel::Safe);
    assert_eq!(merged.risk, RiskLevel::Danger);
}

#[test]
fn merge_retains_every_baseline_match() {
    let b = baseline(
        RiskLevel::Danger,
        vec![
            baseline_match("FS-001", RiskLevel::Danger),
            baseline_match("DB-001", RiskLevel::Danger),
        ],
    );
    let l = language(
        AnalysisStatus::Complete,
        vec![language_match("PY-001", RiskLevel::Warn)],
        vec![],
    );
    let merged = merge_analysis(&b, &l);
    let merged_ids = ids(&merged);
    // Every baseline Match is retained; the language Match is appended.
    assert!(merged_ids.contains(&"FS-001".to_string()));
    assert!(merged_ids.contains(&"DB-001".to_string()));
    assert!(merged_ids.contains(&"PY-001".to_string()));
    // Baseline order preserved, language appended after.
    assert_eq!(merged_ids, vec!["FS-001", "DB-001", "PY-001"]);
}

#[test]
fn merge_dedups_language_matches_sharing_a_baseline_id() {
    // A language Match with the same id as a baseline Match must not
    // duplicate or displace the baseline Match (monotonic: cannot disappear).
    let b = baseline(
        RiskLevel::Danger,
        vec![baseline_match("FS-001", RiskLevel::Danger)],
    );
    let l = language(
        AnalysisStatus::Complete,
        vec![language_match("FS-001", RiskLevel::Warn)],
        vec![],
    );
    let merged = merge_analysis(&b, &l);
    assert_eq!(ids(&merged), vec!["FS-001".to_string()]);
    // The baseline (Danger) Match is the one retained — risk stays Danger.
    assert_eq!(merged.risk, RiskLevel::Danger);
}

#[test]
fn merge_dedup_never_loses_a_higher_language_risk() {
    // Asymmetric direction of the id-collision case above: baseline Warn +
    // a same-id language Match at Danger. The dedup still retains only the
    // baseline Match (id collision), but risk must be the max across EVERY
    // language Match, retained or deduped-away — otherwise a more severe
    // language-detected risk silently vanishes behind a lower-risk baseline
    // Match sharing its id, contradicting this function's own "max of the
    // baseline risk and every language-match risk" doc contract.
    let b = baseline(
        RiskLevel::Warn,
        vec![baseline_match("FS-001", RiskLevel::Warn)],
    );
    let l = language(
        AnalysisStatus::Complete,
        vec![language_match("FS-001", RiskLevel::Danger)],
        vec![],
    );
    let merged = merge_analysis(&b, &l);
    assert_eq!(ids(&merged), vec!["FS-001".to_string()]);
    assert_eq!(merged.risk, RiskLevel::Danger);
}

#[test]
fn merge_degradation_is_carried_and_does_not_lower_risk() {
    // ADR-022 §5: degradation is orthogonal to RiskLevel, may coexist with
    // Safe, and never authorizes auto-execution. Baseline Safe + a Degraded
    // language result with no risky matches → risk stays Safe (not lowered,
    // not synthetically raised to Warn), and the degradation signal is
    // carried on `analysis` so the PolicyEngine can enforce no-auto-exec.
    let b = baseline(RiskLevel::Safe, vec![]);
    let l = language(
        AnalysisStatus::Degraded,
        vec![],
        vec![DegradationReason::DynamicSource],
    );
    let merged = merge_analysis(&b, &l);
    assert_eq!(
        merged.risk,
        RiskLevel::Safe,
        "degradation must not raise risk"
    );
    let analysis = merged
        .analysis
        .expect("degradation is carried onto analysis");
    assert_eq!(analysis.status, AnalysisStatus::Degraded);
    assert_eq!(
        analysis.degradation_reasons,
        vec![DegradationReason::DynamicSource]
    );
}

#[test]
fn merge_degradation_does_not_disappear() {
    // Monotonic: degradation cannot disappear. A degraded language result
    // yields a merged result that is still degraded even when baseline
    // already carried a Danger Match.
    let b = baseline(
        RiskLevel::Danger,
        vec![baseline_match("FS-001", RiskLevel::Danger)],
    );
    let l = language(
        AnalysisStatus::Degraded,
        vec![language_match("PY-001", RiskLevel::Danger)],
        vec![DegradationReason::LimitExceeded],
    );
    let merged = merge_analysis(&b, &l);
    assert_eq!(merged.risk, RiskLevel::Danger);
    let analysis = merged.analysis.expect("analysis carried");
    assert_eq!(analysis.status, AnalysisStatus::Degraded);
    assert_eq!(
        analysis.degradation_reasons,
        vec![DegradationReason::LimitExceeded]
    );
}

#[test]
fn merge_empty_language_yields_baseline_plus_not_applicable() {
    let b = baseline(
        RiskLevel::Danger,
        vec![baseline_match("FS-001", RiskLevel::Danger)],
    );
    let l = language(AnalysisStatus::NotApplicable, vec![], vec![]);
    let merged = merge_analysis(&b, &l);
    // Baseline fields are preserved…
    assert_eq!(merged.risk, b.risk);
    assert_eq!(ids(&merged), ids(&b));
    assert_eq!(merged.effect_opaque, b.effect_opaque);
    assert_eq!(merged.command, b.command);
    // …and the merge records that language analysis was not applicable.
    let analysis = merged.analysis.expect("merge always records analysis");
    assert_eq!(analysis.status, AnalysisStatus::NotApplicable);
    assert!(analysis.degradation_reasons.is_empty());
}

#[test]
fn merge_is_deterministic() {
    let b = baseline(
        RiskLevel::Warn,
        vec![baseline_match("GIT-001", RiskLevel::Warn)],
    );
    let l = language(
        AnalysisStatus::Complete,
        vec![language_match("PY-001", RiskLevel::Warn)],
        vec![],
    );
    let first = merge_analysis(&b, &l);
    let second = merge_analysis(&b, &l);
    // Same inputs → identical merged Assessment (matched order, risk, analysis).
    assert_eq!(first.risk, second.risk);
    assert_eq!(ids(&first), ids(&second));
    assert_eq!(first.analysis, second.analysis);
}
