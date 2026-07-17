//! Language-aware analysis data model (ADR-022 §4).
//!
//! This module holds the **zero-I/O shared types** that let the scanner model
//! represent all detection mechanisms — regex `Pattern`, `Token-prefix rule`,
//! and Language-aware rule — through one common contract, plus the typed
//! evidence, status, and degradation vocabulary that language adapters emit.
//!
//! Constraints (plan Iteration 1 REVIEW GATE):
//! - Pure data only. No filesystem access, no subprocess, no Tree-sitter
//!   types, and no dependency arrow from this crate to any parser crate.
//! - These types carry no source body, full snippet, variable value, or AST.
//!   Provenance persists metadata only (ADR-022 §10).
//!
//! Behavior is unchanged by introducing this module: the existing scanner
//! `Assessment` and `MatchResult` are not yet refactored onto this model (that
//! is a later slice). The types here are the foundation language-aware
//! analysis will populate.

use serde::{Deserialize, Serialize};

/// Which detection mechanism produced a `Match`.
///
/// The three concrete mechanisms of the common Detection rule contract
/// (ADR-022 §4): regex `Pattern`, `Token-prefix rule`, and Language-aware
/// rule. A `Match` always identifies exactly one mechanism.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DetectionMechanism {
    /// A regex `Pattern` matched anywhere in the normalized command.
    RegexPattern,
    /// A `Token-prefix rule` matched the command's `Effective program` + token prefix.
    TokenPrefixRule,
    /// A built-in Language-aware rule matched via structural source analysis.
    LanguageRule,
}

/// Whether a detection rule is built into Aegis or came from user config.
///
/// Distinct from `crate::PatternSource` (which lives on the legacy `Pattern`
/// struct): this is the common per-`Match` "built in or custom" flag ADR-022 §4
/// requires every `Match` to carry.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DetectionSource {
    /// Shipped with Aegis.
    Builtin,
    /// Loaded from user configuration.
    Custom,
}

impl From<crate::PatternSource> for DetectionSource {
    fn from(source: crate::PatternSource) -> Self {
        match source {
            crate::PatternSource::Builtin => DetectionSource::Builtin,
            crate::PatternSource::Custom => DetectionSource::Custom,
        }
    }
}

/// How completely a `Detected operation`'s operand is known to static analysis
/// (ADR-022 §3).
///
/// Ordered by *decreasing* certainty: `Known < Partial < Dynamic`. A `Dynamic`
/// operand is never treated as evidence of safety — it records Analysis
/// degradation in addition to the visible operation (ADR-022 §3, §7).
#[non_exhaustive]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Deserialize,
    Serialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum OperandCertainty {
    /// The operand is a literal recoverable from the source.
    Known,
    /// The operand is partially resolved (e.g. an alias or adjacent literal).
    Partial,
    /// The operand is computed, imported, or otherwise not statically recoverable.
    Dynamic,
}

/// The state of language-aware analysis for one target (ADR-022 §4).
///
/// Ordered by *increasing* degradation: `NotApplicable < Complete < Degraded`,
/// so `max` of a set of statuses is the worst (most degraded) one — the
/// invariant the monotonic merge (plan Iteration 1 RED #3) relies on.
#[non_exhaustive]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Deserialize,
    Serialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStatus {
    /// No language-aware analysis applies to this target (no analyzable source).
    NotApplicable,
    /// Analysis ran to completion with no degradation.
    Complete,
    /// Analysis ran but produced typed degradation; prior results are retained.
    Degraded,
}

/// Why language-aware analysis degraded for a target (ADR-022 §4).
///
/// Variants mirror the seven degradation buckets ADR-022 §4 enumerates. The
/// enum is `#[non_exhaustive]` so adapters may surface finer-grained reasons
/// without breaking serialization consumers.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DegradationReason {
    /// The grammar for the target language is unsupported or unavailable.
    GrammarUnavailable,
    /// The source could not be fully parsed (malformed or incomplete syntax).
    IncompleteSyntax,
    /// The source was unsafe or unavailable to read (symlink, FIFO, permissions, …).
    UnsafeSource,
    /// The source used an unsupported encoding (invalid UTF-8, UTF-16, …).
    UnsupportedEncoding,
    /// A size, file-count, recursion-depth, or timeout limit was exceeded.
    LimitExceeded,
    /// The source or working directory was dynamic and could not be resolved.
    DynamicSource,
    /// The analysis worker or its protocol failed (crash, timeout, bad frame).
    WorkerFailure,
}

/// The kind of destructive effect or execution sink a language-aware rule
/// detected (ADR-022 §3 initial scope).
///
/// `CodeExecution` is the canonical kind for a recognized process, shell, or
/// eval sink: it always emits a `CodeExecution` Match regardless of payload
/// certainty (ADR-022 §3, §5). The enum is `#[non_exhaustive]` so adapters may
/// add operation kinds without breaking serialization consumers. It carries
/// no inherent severity ordering — the shared classifier maps kind + modifiers
/// + certainty to `Category`/`RiskLevel`, not this enum's declaration order.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    /// Recursive or single filesystem deletion (`os.remove`, `fs.rmSync`, …).
    FilesystemDelete,
    /// Overwrite or truncation of an existing file (`open('w')`, `truncate`, …).
    FilesystemOverwrite,
    /// A dangerous permission or ownership change (`chmod 000`, `os.chown`, …).
    PermissionOrOwnershipChange,
    /// A write to a device file or other critical-path target.
    DeviceOrCriticalWrite,
    /// A destructive database operation (`DROP TABLE`, `DELETE` without where, …).
    DatabaseDestructive,
    /// A recognized process, shell, or eval sink (`subprocess.run`, `eval`, …).
    CodeExecution,
    /// A destructive cloud-provider API call.
    CloudDestructive,
    /// A destructive container-management operation.
    ContainerDestructive,
    /// A destructive package-manager operation.
    PackageDestructive,
}

/// Modifiers that refine an [`OperationKind`] (ADR-022 §3).
///
/// All flags default to `false`; an operation carries only the modifiers that
/// apply to it.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize, Serialize, schemars::JsonSchema,
)]
pub struct OperationModifiers {
    /// The operation is recursive (`rm -r`, `shutil.rmtree`, …).
    pub recursive: bool,
    /// The operation is forced (`rm -f`, `--force`, …).
    pub forced: bool,
    /// The operation is in an explicitly destructive mode (e.g. a destructive
    /// open flag or overwrite mode).
    pub destructive_mode: bool,
}

/// A language-neutral operation detected from source syntax (ADR-022 §3).
///
/// Each adapter emits `DetectedOperation`s rather than assigning `RiskLevel`
/// directly from an API spelling. A shared classifier maps `kind`, `modifiers`,
/// and `certainty` into the existing `Category`, `RiskLevel`, explanation,
/// safer alternative, and `Match` vocabulary. A `Dynamic` operand never
/// authorizes treating the operation as safe (ADR-022 §3, §7).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize, schemars::JsonSchema)]
pub struct DetectedOperation {
    /// What effect or execution sink was detected.
    pub kind: OperationKind,
    /// Modifiers refining the kind (recursive, forced, destructive mode).
    pub modifiers: OperationModifiers,
    /// How completely the operand is known to static analysis.
    pub certainty: OperandCertainty,
}

/// Where a language-aware analysis target's source came from (ADR-022 §6).
///
/// Drives routing and recovery: only `Inline` / `Heredoc` / `ScriptFile` / `Stdin`
/// / `Pipe` shapes can become analysis targets, and each carries different
/// Effect-opaque / Required-recovery consequences.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceOrigin {
    /// An inline interpreter body (`-c` / `-e`).
    Inline,
    /// A heredoc body supplied to the command.
    Heredoc,
    /// A script file named in argv.
    ScriptFile,
    /// Interpreter stdin.
    Stdin,
    /// A pipe-to-shell sink.
    Pipe,
}

/// A concrete byte span inside analyzed source (ADR-022 §10).
///
/// Carries line/column for human display and byte offsets for mapping back to
/// the original bytes. It references position only — it never carries the
/// source text itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ByteSpan {
    /// 1-based line number.
    pub line: u32,
    /// 1-based column number (in bytes).
    pub column: u32,
    /// Inclusive start byte offset within the source.
    pub byte_start: usize,
    /// Exclusive end byte offset within the source.
    pub byte_end: usize,
}

/// Metadata recording where a language-aware result came from (ADR-022 §10).
///
/// Provenance persists metadata only. It MUST NOT carry script contents, full
/// snippets, imported source, variable values, or syntax trees — the TUI may
/// render a short in-memory snippet, but provenance never does (ADR-022 §10).
/// The `privacy_test_*` tests below pin that invariant at the serialization
/// boundary so a later field addition cannot silently leak source.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AnalysisProvenance {
    /// The detected language (e.g. `"python"`), when applicable.
    pub language: Option<String>,
    /// Where the analyzed source came from.
    pub source_origin: SourceOrigin,
    /// The stable detection rule ID that fired, when applicable.
    pub rule_id: Option<String>,
    /// The detected operation, when a language-aware rule matched.
    pub operation: Option<DetectedOperation>,
    /// The analyzed file path, when the target was a `ScriptFile`.
    pub file_path: Option<String>,
    /// A hex digest of the original analyzed bytes (never the bytes).
    pub source_hash: Option<String>,
    /// The span in the source the result refers to, when localized.
    pub span: Option<ByteSpan>,
    /// The operand certainty of the result.
    pub certainty: OperandCertainty,
    /// The analysis status for this target.
    pub status: AnalysisStatus,
    /// The degradation reason, when `status` is `Degraded`.
    pub degradation_reason: Option<DegradationReason>,
}

/// Per-target language-aware analysis result (ADR-022 §4).
///
/// One `TargetAnalysis` per analysis target. `status` is ordered by increasing
/// degradation (`NotApplicable < Complete < Degraded`), so the worst target
/// drives the merged `Assessment` status. `degradation_reasons` carries zero or
/// more typed reasons; an empty vector is valid only when `status` is not
/// `Degraded`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
pub struct TargetAnalysis {
    /// The analysis status for this target.
    pub status: AnalysisStatus,
    /// Typed degradation reasons (non-empty iff `status == Degraded`).
    pub degradation_reasons: Vec<DegradationReason>,
    /// Provenance for this target, when analysis ran.
    pub provenance: Option<AnalysisProvenance>,
}

/// Typed evidence carried by every `Match` (ADR-022 §4).
///
/// The variant encodes the detection mechanism, so an impossible combination
/// (e.g. a regex `Pattern` carrying a `DetectedOperation`) is unconstructable.
/// Only `LanguageRule` carries a `DetectedOperation` and `AnalysisProvenance`;
/// regex `Pattern` and `Token-prefix rule` matches carry just their source.
/// Every variant also records whether the rule is built in or custom.
///
/// `DetectionMechanism` remains the lightweight standalone tag for consumers
/// that need only the mechanism (audit projection, deterministic sort);
/// [`MatchEvidence::mechanism`] projects this enum to that tag.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum MatchEvidence {
    /// A regex `Pattern` matched anywhere in the normalized command.
    RegexPattern {
        /// Whether the pattern is built in or custom.
        source: DetectionSource,
    },
    /// A `Token-prefix rule` matched the command's `Effective program` + prefix.
    TokenPrefixRule {
        /// Whether the rule is built in or custom.
        source: DetectionSource,
    },
    /// A built-in Language-aware rule matched via structural source analysis.
    LanguageRule {
        /// Whether the rule is built in (Language-aware rules are always built
        /// in — project config cannot define custom Tree-sitter queries,
        /// ADR-022 §4 — but the field is retained so the common per-Match
        /// "built in or custom" contract is uniform).
        source: DetectionSource,
        /// The detected operation that fired.
        operation: DetectedOperation,
        /// Where the result came from (metadata only — no source body).
        provenance: AnalysisProvenance,
    },
}

impl MatchEvidence {
    /// Project this evidence to the lightweight mechanism tag.
    pub fn mechanism(&self) -> DetectionMechanism {
        match self {
            MatchEvidence::RegexPattern { .. } => DetectionMechanism::RegexPattern,
            MatchEvidence::TokenPrefixRule { .. } => DetectionMechanism::TokenPrefixRule,
            MatchEvidence::LanguageRule { .. } => DetectionMechanism::LanguageRule,
        }
    }

    /// Whether the rule that fired is built in or custom.
    pub fn source(&self) -> DetectionSource {
        match self {
            MatchEvidence::RegexPattern { source }
            | MatchEvidence::TokenPrefixRule { source }
            | MatchEvidence::LanguageRule { source, .. } => *source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operand_certainty_orders_known_partial_dynamic() {
        // Decreasing certainty: Known < Partial < Dynamic. max() is the
        // least-certain (Dynamic), which the merge must never treat as safe.
        assert!(OperandCertainty::Known < OperandCertainty::Partial);
        assert!(OperandCertainty::Partial < OperandCertainty::Dynamic);
        assert_eq!(
            *[
                OperandCertainty::Dynamic,
                OperandCertainty::Known,
                OperandCertainty::Partial,
            ]
            .iter()
            .max()
            .unwrap(),
            OperandCertainty::Dynamic,
        );
    }

    #[test]
    fn analysis_status_orders_not_applicable_complete_degraded() {
        // Increasing degradation: NotApplicable < Complete < Degraded. max()
        // of any set is the worst status — the merge invariant.
        assert!(AnalysisStatus::NotApplicable < AnalysisStatus::Complete);
        assert!(AnalysisStatus::Complete < AnalysisStatus::Degraded);
        assert_eq!(
            *[
                AnalysisStatus::Degraded,
                AnalysisStatus::NotApplicable,
                AnalysisStatus::Complete,
            ]
            .iter()
            .max()
            .unwrap(),
            AnalysisStatus::Degraded,
        );
    }

    #[test]
    fn detection_mechanism_round_trips_through_serde() {
        for variant in [
            DetectionMechanism::RegexPattern,
            DetectionMechanism::TokenPrefixRule,
            DetectionMechanism::LanguageRule,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: DetectionMechanism = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
        assert_eq!(
            serde_json::to_string(&DetectionMechanism::TokenPrefixRule).unwrap(),
            "\"token_prefix_rule\"",
        );
    }

    #[test]
    fn detection_source_round_trips_through_serde() {
        assert_eq!(
            serde_json::to_string(&DetectionSource::Builtin).unwrap(),
            "\"builtin\"",
        );
        assert_eq!(
            serde_json::to_string(&DetectionSource::Custom).unwrap(),
            "\"custom\"",
        );
        for variant in [DetectionSource::Builtin, DetectionSource::Custom] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: DetectionSource = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn degradation_reason_round_trips_each_bucket() {
        let all = [
            DegradationReason::GrammarUnavailable,
            DegradationReason::IncompleteSyntax,
            DegradationReason::UnsafeSource,
            DegradationReason::UnsupportedEncoding,
            DegradationReason::LimitExceeded,
            DegradationReason::DynamicSource,
            DegradationReason::WorkerFailure,
        ];
        for variant in all {
            let json = serde_json::to_string(&variant).unwrap();
            let back: DegradationReason = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
        // Spot-check the snake_case tag of two distinct buckets.
        assert_eq!(
            serde_json::to_string(&DegradationReason::LimitExceeded).unwrap(),
            "\"limit_exceeded\"",
        );
        assert_eq!(
            serde_json::to_string(&DegradationReason::WorkerFailure).unwrap(),
            "\"worker_failure\"",
        );
    }

    #[test]
    fn operation_kind_round_trips_through_serde() {
        let all = [
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
        for variant in all {
            let json = serde_json::to_string(&variant).unwrap();
            let back: OperationKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
        assert_eq!(
            serde_json::to_string(&OperationKind::CodeExecution).unwrap(),
            "\"code_execution\"",
        );
    }

    #[test]
    fn operation_modifiers_default_all_false_and_round_trip() {
        let mods = OperationModifiers::default();
        assert!(!mods.recursive);
        assert!(!mods.forced);
        assert!(!mods.destructive_mode);

        let mods = OperationModifiers {
            recursive: true,
            forced: false,
            destructive_mode: true,
        };
        let json = serde_json::to_string(&mods).unwrap();
        let back: OperationModifiers = serde_json::from_str(&json).unwrap();
        assert_eq!(back, mods);
        assert!(json.contains("\"recursive\":true"));
        assert!(json.contains("\"destructive_mode\":true"));
        // Derived `Deserialize` has no `#[serde(default)]`, so all three fields
        // are required and serialized — including `forced: false`.
        assert!(json.contains("\"forced\":false"));
    }

    #[test]
    fn detected_operation_round_trips_and_preserves_certainty() {
        let op = DetectedOperation {
            kind: OperationKind::FilesystemDelete,
            modifiers: OperationModifiers {
                recursive: true,
                forced: true,
                destructive_mode: false,
            },
            certainty: OperandCertainty::Known,
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: DetectedOperation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, op);
        assert_eq!(back.certainty, OperandCertainty::Known);
        assert_eq!(back.kind, OperationKind::FilesystemDelete);
        assert!(back.modifiers.recursive && back.modifiers.forced);
    }

    #[test]
    fn detected_operation_with_dynamic_certainty_is_not_known() {
        // A Dynamic operand is never evidence of safety (ADR-022 §3, §7). The
        // type system does not encode that invariant, but the certainty must
        // round-trip as `Dynamic` — the classifier/merge layer enforces the
        // never-safe rule, and this test pins the data contract it depends on.
        let op = DetectedOperation {
            kind: OperationKind::CodeExecution,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Dynamic,
        };
        let back: DetectedOperation =
            serde_json::from_str(&serde_json::to_string(&op).unwrap()).unwrap();
        assert_eq!(back.certainty, OperandCertainty::Dynamic);
        assert_ne!(back.certainty, OperandCertainty::Known);
    }

    #[test]
    fn source_origin_round_trips_through_serde() {
        for variant in [
            SourceOrigin::Inline,
            SourceOrigin::Heredoc,
            SourceOrigin::ScriptFile,
            SourceOrigin::Stdin,
            SourceOrigin::Pipe,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let back: SourceOrigin = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
        assert_eq!(
            serde_json::to_string(&SourceOrigin::ScriptFile).unwrap(),
            "\"script_file\"",
        );
    }

    #[test]
    fn byte_span_round_trips_through_serde() {
        let span = ByteSpan {
            line: 3,
            column: 5,
            byte_start: 42,
            byte_end: 48,
        };
        let json = serde_json::to_string(&span).unwrap();
        let back: ByteSpan = serde_json::from_str(&json).unwrap();
        assert_eq!(back, span);
        assert!(json.contains("\"byte_start\":42"));
        assert!(json.contains("\"byte_end\":48"));
    }

    #[test]
    fn analysis_provenance_round_trips_with_metadata_only() {
        let provenance = AnalysisProvenance {
            language: Some("python".to_string()),
            source_origin: SourceOrigin::Inline,
            rule_id: Some("PY-001".to_string()),
            operation: Some(DetectedOperation {
                kind: OperationKind::FilesystemDelete,
                modifiers: OperationModifiers {
                    recursive: false,
                    forced: false,
                    destructive_mode: false,
                },
                certainty: OperandCertainty::Known,
            }),
            file_path: None,
            source_hash: Some("deadbeef".to_string()),
            span: Some(ByteSpan {
                line: 1,
                column: 1,
                byte_start: 0,
                byte_end: 10,
            }),
            certainty: OperandCertainty::Known,
            status: AnalysisStatus::Complete,
            degradation_reason: None,
        };
        let json = serde_json::to_string(&provenance).unwrap();
        let back: AnalysisProvenance = serde_json::from_str(&json).unwrap();
        assert_eq!(back, provenance);
        assert_eq!(back.language.as_deref(), Some("python"));
    }

    #[test]
    fn analysis_provenance_serialized_form_carries_no_source_body_or_snippet() {
        // ADR-022 §10: provenance must not persist script contents, full
        // snippets, imported source, variable values, or syntax trees. Pin
        // that at the serialization boundary so a later field cannot leak
        // source silently. The expected key set is the independent source of
        // truth (the ADR's allow-list), not a re-derivation of the struct.
        let provenance = AnalysisProvenance {
            language: Some("python".to_string()),
            source_origin: SourceOrigin::ScriptFile,
            rule_id: Some("PY-002".to_string()),
            operation: None,
            file_path: Some("/tmp/x.py".to_string()),
            source_hash: Some("abc123".to_string()),
            span: None,
            certainty: OperandCertainty::Partial,
            status: AnalysisStatus::Degraded,
            degradation_reason: Some(DegradationReason::DynamicSource),
        };
        let json = serde_json::to_string(&provenance).unwrap();
        let obj: serde_json::Value = serde_json::from_str(&json).unwrap();
        let keys: Vec<&str> = obj
            .as_object()
            .expect("provenance serializes to a JSON object")
            .keys()
            .map(String::as_str)
            .collect();
        let forbidden = [
            "body",
            "snippet",
            "source",
            "source_body",
            "source_contents",
            "contents",
            "text",
            "ast",
            "syntax_tree",
            "value",
            "variable_value",
            "imported_source",
        ];
        for key in forbidden {
            assert!(
                !keys.contains(&key),
                "provenance leaked forbidden source-bearing key {key:?} in {keys:?}",
            );
        }
        // The path and hash ARE allowed (metadata, not contents).
        assert!(keys.contains(&"file_path"));
        assert!(keys.contains(&"source_hash"));
    }

    #[test]
    fn target_analysis_round_trips_and_status_orders_toward_degraded() {
        let complete = TargetAnalysis {
            status: AnalysisStatus::Complete,
            degradation_reasons: Vec::new(),
            provenance: None,
        };
        let degraded = TargetAnalysis {
            status: AnalysisStatus::Degraded,
            degradation_reasons: vec![DegradationReason::LimitExceeded],
            provenance: None,
        };
        // The merge takes the worst status; Degraded beats Complete.
        assert_eq!(
            complete.status.max(degraded.status),
            AnalysisStatus::Degraded,
        );
        // Round-trip preserves the typed reasons.
        let json = serde_json::to_string(&degraded).unwrap();
        let back: TargetAnalysis = serde_json::from_str(&json).unwrap();
        assert_eq!(back, degraded);
        assert_eq!(
            back.degradation_reasons,
            vec![DegradationReason::LimitExceeded]
        );
    }

    #[test]
    fn match_evidence_regex_round_trips_and_projects_to_mechanism() {
        let evidence = MatchEvidence::RegexPattern {
            source: DetectionSource::Builtin,
        };
        let json = serde_json::to_string(&evidence).unwrap();
        let back: MatchEvidence = serde_json::from_str(&json).unwrap();
        assert_eq!(back, evidence);
        assert_eq!(evidence.mechanism(), DetectionMechanism::RegexPattern);
        assert_eq!(evidence.source(), DetectionSource::Builtin);
        // Tagged shape: { "kind": "regex_pattern", "source": "builtin" }.
        // The discriminator key is the generic "kind" (consistent with
        // `AssessmentBasis`); the domain term lives in the variant value.
        assert!(json.contains("\"kind\":\"regex_pattern\""));
        assert!(json.contains("\"source\":\"builtin\""));
    }

    #[test]
    fn match_evidence_token_prefix_round_trips() {
        let evidence = MatchEvidence::TokenPrefixRule {
            source: DetectionSource::Custom,
        };
        let json = serde_json::to_string(&evidence).unwrap();
        let back: MatchEvidence = serde_json::from_str(&json).unwrap();
        assert_eq!(back, evidence);
        assert_eq!(evidence.mechanism(), DetectionMechanism::TokenPrefixRule);
        assert_eq!(evidence.source(), DetectionSource::Custom);
    }

    #[test]
    fn match_evidence_language_carries_operation_and_provenance() {
        // Only LanguageRule carries operation + provenance (ADR-022 §4). The
        // enum shape makes a regex match carrying an operation unconstructable.
        let operation = DetectedOperation {
            kind: OperationKind::CodeExecution,
            modifiers: OperationModifiers::default(),
            certainty: OperandCertainty::Dynamic,
        };
        let provenance = AnalysisProvenance {
            language: Some("javascript".to_string()),
            source_origin: SourceOrigin::Inline,
            rule_id: Some("JS-001".to_string()),
            operation: Some(operation.clone()),
            file_path: None,
            source_hash: Some("feedface".to_string()),
            span: None,
            certainty: OperandCertainty::Dynamic,
            status: AnalysisStatus::Degraded,
            degradation_reason: Some(DegradationReason::DynamicSource),
        };
        let evidence = MatchEvidence::LanguageRule {
            source: DetectionSource::Builtin,
            operation: operation.clone(),
            provenance: provenance.clone(),
        };
        let json = serde_json::to_string(&evidence).unwrap();
        let back: MatchEvidence = serde_json::from_str(&json).unwrap();
        assert_eq!(back, evidence);
        assert_eq!(evidence.mechanism(), DetectionMechanism::LanguageRule);
        assert_eq!(evidence.source(), DetectionSource::Builtin);
        // A Dynamic code-execution sink still records its operation in
        // evidence (ADR-022 §3): uncertainty never hides the visible sink.
        assert!(json.contains("\"kind\":\"language_rule\""));
        assert!(json.contains("\"kind\":\"code_execution\""));
        assert!(json.contains("\"certainty\":\"dynamic\""));
    }
}
