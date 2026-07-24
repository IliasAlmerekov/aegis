pub(crate) use crate::block_screen::render_policy_block;
pub use crate::recovery_screen::{RecoveryPromptDecision, show_recovery_override_with_input};
pub(crate) use crate::shared::{build_highlighted_command, sorted_highlight_ranges_for_tests};
pub use crate::stdout_renderer::PromptDecision;
pub use crate::stdout_renderer::show_confirmation_with_input;
pub use crate::tty_renderer::tty_unavailable_decision;

use std::borrow::Cow;
use std::sync::Arc;

use aegis_config::{ConfigSourceLayer, Mode};
use aegis_explanation::{
    AllowlistExplanation, BlockReason, CommandExplanation, ExecutionContextExplanation,
    ExecutionTransport, PolicyAction, PolicyExplanation, PolicyRationale, ScanExplanation,
};
use aegis_parser::Parser;
use aegis_types::{
    AnalysisProvenance, AnalysisStatus, Assessment, ByteSpan, Category, DetectedOperation,
    DetectionSource, HighlightRange, MatchEvidence, MatchResult, OperandCertainty, OperationKind,
    OperationModifiers, Pattern, PatternSource, SourceOrigin,
};
use aegis_types::{RiskLevel, SnapshotRecord};

// ── helpers ───────────────────────────────────────────────────────────────

pub fn make_match(
    id: &'static str,
    risk: RiskLevel,
    pattern: &'static str,
    description: &'static str,
    safe_alt: Option<&'static str>,
) -> MatchResult {
    MatchResult {
        pattern: Arc::new(Pattern {
            id: Cow::Borrowed(id),
            category: Category::Filesystem,
            risk,
            pattern: Cow::Borrowed(pattern),
            description: Cow::Borrowed(description),
            safe_alt: safe_alt.map(Cow::Borrowed),
            source: PatternSource::Builtin,
            justification: None,
        }),
        matched_text: String::new(),
        highlight_range: None,
        evidence: MatchEvidence::RegexPattern {
            source: DetectionSource::Builtin,
        },
    }
}

pub fn make_match_with_justification(
    id: &'static str,
    risk: RiskLevel,
    pattern: &'static str,
    description: &'static str,
    safe_alt: Option<&'static str>,
    justification: Option<&'static str>,
) -> MatchResult {
    MatchResult {
        pattern: Arc::new(Pattern {
            id: Cow::Borrowed(id),
            category: Category::Filesystem,
            risk,
            pattern: Cow::Borrowed(pattern),
            description: Cow::Borrowed(description),
            safe_alt: safe_alt.map(Cow::Borrowed),
            source: PatternSource::Builtin,
            justification: justification.map(Cow::Borrowed),
        }),
        matched_text: String::new(),
        highlight_range: None,
        evidence: MatchEvidence::RegexPattern {
            source: DetectionSource::Builtin,
        },
    }
}

pub fn make_match_with_text(
    id: &'static str,
    risk: RiskLevel,
    pattern: &'static str,
    description: &'static str,
    matched_text: &'static str,
) -> MatchResult {
    MatchResult {
        pattern: Arc::new(Pattern {
            id: Cow::Borrowed(id),
            category: Category::Filesystem,
            risk,
            pattern: Cow::Borrowed(pattern),
            description: Cow::Borrowed(description),
            safe_alt: None,
            source: PatternSource::Builtin,
            justification: None,
        }),
        matched_text: matched_text.to_string(),
        highlight_range: None,
        evidence: MatchEvidence::RegexPattern {
            source: DetectionSource::Builtin,
        },
    }
}

pub fn make_match_with_text_and_range(
    id: &'static str,
    risk: RiskLevel,
    pattern: &'static str,
    description: &'static str,
    matched_text: &'static str,
    start: usize,
) -> MatchResult {
    MatchResult {
        pattern: Arc::new(Pattern {
            id: Cow::Borrowed(id),
            category: Category::Filesystem,
            risk,
            pattern: Cow::Borrowed(pattern),
            description: Cow::Borrowed(description),
            safe_alt: None,
            source: PatternSource::Builtin,
            justification: None,
        }),
        matched_text: matched_text.to_string(),
        highlight_range: Some(HighlightRange {
            start,
            end: start + matched_text.len(),
        }),
        evidence: MatchEvidence::RegexPattern {
            source: DetectionSource::Builtin,
        },
    }
}

pub fn make_assessment(cmd: &str, risk: RiskLevel, matches: Vec<MatchResult>) -> Assessment {
    Assessment {
        risk,
        effect_opaque: false,
        highlight_ranges: sorted_highlight_ranges_for_tests(cmd, &matches),
        matched: matches,
        command: Parser::parse(cmd),
        analysis: None,
    }
}

pub fn make_language_match(id: &'static str, risk: RiskLevel, matched_text: &str) -> MatchResult {
    let operation = DetectedOperation {
        kind: OperationKind::FilesystemDelete,
        modifiers: OperationModifiers::default(),
        certainty: OperandCertainty::Known,
    };
    MatchResult {
        pattern: Arc::new(Pattern {
            id: Cow::Borrowed(id),
            category: Category::Filesystem,
            risk,
            pattern: Cow::Borrowed(""),
            description: Cow::Borrowed("Language-aware filesystem deletion"),
            safe_alt: None,
            source: PatternSource::Builtin,
            justification: None,
        }),
        matched_text: matched_text.to_string(),
        highlight_range: None,
        evidence: MatchEvidence::LanguageRule {
            source: DetectionSource::Builtin,
            operation: operation.clone(),
            provenance: AnalysisProvenance {
                language: Some("python".to_string()),
                source_origin: SourceOrigin::ScriptFile,
                rule_id: Some(id.to_string()),
                operation: Some(operation),
                file_path: Some("danger.py".to_string()),
                source_hash: Some("abc123".to_string()),
                span: Some(ByteSpan {
                    line: 7,
                    column: 3,
                    byte_start: 42,
                    byte_end: 61,
                }),
                certainty: OperandCertainty::Known,
                status: AnalysisStatus::Degraded,
                degradation_reason: Some(aegis_types::DegradationReason::IncompleteSyntax),
            },
        },
    }
}

pub fn make_explanation(
    assessment: &Assessment,
    rationale: PolicyRationale,
    block_reason: Option<BlockReason>,
    allowlist_match: Option<AllowlistExplanation>,
) -> CommandExplanation {
    CommandExplanation {
        scan: ScanExplanation {
            highest_risk: assessment.risk,
            decision_source: assessment.decision_source(),
            basis: assessment.basis(),
            matched_patterns: Vec::new(),
        },
        policy: PolicyExplanation {
            action: match block_reason {
                Some(_) => PolicyAction::Block,
                None => PolicyAction::Prompt,
            },
            rationale,
            requires_confirmation: block_reason.is_none(),
            snapshots_required: assessment.risk == RiskLevel::Danger,
            allowlist_effective: false,
            block_reason,
        },
        context: ExecutionContextExplanation {
            mode: Mode::Protect,
            transport: ExecutionTransport::Shell,
            ci_detected: false,
            allowlist_match,
            applicable_snapshot_plugins: Vec::new(),
        },
        outcome: None,
    }
}

pub fn default_explanation_for_assessment(assessment: &Assessment) -> CommandExplanation {
    match assessment.risk {
        RiskLevel::Safe => make_explanation(assessment, PolicyRationale::SafeCommand, None, None),
        RiskLevel::Warn | RiskLevel::Danger => make_explanation(
            assessment,
            PolicyRationale::RequiresConfirmation,
            None,
            None,
        ),
        RiskLevel::Block => make_explanation(
            assessment,
            PolicyRationale::IntrinsicRiskBlock,
            Some(BlockReason::IntrinsicRiskBlock),
            None,
        ),
        _ => make_explanation(
            assessment,
            PolicyRationale::IntrinsicRiskBlock,
            Some(BlockReason::IntrinsicRiskBlock),
            None,
        ),
    }
}

/// Strip ANSI escape sequences from a string so we can do plain-text assertions.
pub fn strip_ansi(s: &str) -> String {
    let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    re.replace_all(s, "").to_string()
}

mod block_tests;
mod danger_tests;
mod decision_tests;
mod recovery_tests;
mod render_tests;
mod warn_tests;
