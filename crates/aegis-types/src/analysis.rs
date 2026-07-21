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

/// Aggregate language-aware analysis result for one intercepted command, the
/// input to [`merge_analysis`] (ADR-022 §1, §4).
///
/// Carries the overall analysis `status`, the language-aware `Match`es (each
/// with `MatchEvidence::LanguageRule`), and the typed degradation `reasons`. The
/// parent process builds this from worker results (Iterations 3–8); for
/// Iteration 1 it is exercised with synthetic inputs.
///
/// Not `Eq`: `matches` holds `MatchResult`, which is not `Eq` (its `Arc<Pattern>`
/// is not `Eq`). Compare by `status` / `degradation_reasons` or by the merged
/// `Assessment` instead.
#[derive(Debug, Clone)]
pub struct LanguageAnalysisResult {
    /// Overall analysis status (worst per-target status).
    pub status: AnalysisStatus,
    /// Language-aware `Match`es produced by the adapters.
    pub matches: Vec<crate::MatchResult>,
    /// Typed degradation reasons (non-empty iff `status == Degraded`).
    pub degradation_reasons: Vec<DegradationReason>,
}

/// Summary of language-aware analysis carried on a merged `Assessment`
/// (ADR-022 §1: "merges all analysis results into one Assessment").
///
/// `status` + `degradation_reasons` are orthogonal to `RiskLevel` (ADR-022 §5):
/// degradation may coexist with `Safe` and never authorizes auto-execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AnalysisSummary {
    /// Overall language-aware analysis status.
    pub status: AnalysisStatus,
    /// Typed degradation reasons (non-empty iff `status == Degraded`).
    pub degradation_reasons: Vec<DegradationReason>,
}

/// Merge the baseline scanner `Assessment` with a `LanguageAnalysisResult` into
/// one `Assessment` (ADR-022 §1, plan Iteration 1 RED #3).
///
/// Monotonic invariants (this function is the single place that enforces them):
/// - **risk never decreases**: the merged `RiskLevel` is the max of the
///   baseline risk and every language-match risk, so it is `>= baseline.risk`.
/// - **Matches never disappear**: every baseline `Match` is retained; language
///   `Match`es are appended, deduplicated by pattern id against the baseline
///   set so a duplicate id does not displace the baseline `Match`.
/// - **degradation is carried, not lowering risk**: the language `status` and
///   `degradation_reasons` flow onto `Assessment.analysis`. A degraded result
///   may coexist with `Safe` (ADR-022 §5); this function records the
///   degradation and never lowers risk to authorize auto-execution — the
///   enforcement of "no auto-execution" is the PolicyEngine's job (Iteration 9),
///   but the merge guarantees the signal is present and risk is not dropped.
///
/// `effect_opaque`, `highlight_ranges`, and `command` come from the baseline
/// (language source spans live on each language `Match`, not on the
/// command-span `highlight_ranges`).
pub fn merge_analysis(
    baseline: &crate::Assessment,
    language: &LanguageAnalysisResult,
) -> crate::Assessment {
    let mut merged_matched = baseline.matched.clone();
    let baseline_ids: std::collections::HashSet<&str> = baseline
        .matched
        .iter()
        .map(|m| m.pattern.id.as_ref())
        .collect();
    for m in &language.matches {
        if !baseline_ids.contains(m.pattern.id.as_ref()) {
            merged_matched.push(m.clone());
        }
    }

    // Risk is the max over the baseline risk, every retained Match, AND every
    // language Match — including ones deduped away above. A same-id language
    // Match is dropped from `matched` (the baseline Match is retained per the
    // "Matches never disappear" invariant), but its risk must still count:
    // otherwise a more severe language-detected risk could silently vanish
    // behind a lower-risk baseline Match that happens to share its id.
    let risk = merged_matched
        .iter()
        .map(|m| m.pattern.risk)
        .chain(language.matches.iter().map(|m| m.pattern.risk))
        .max()
        .map(|mx| mx.max(baseline.risk))
        .unwrap_or(baseline.risk);

    crate::Assessment {
        risk,
        effect_opaque: baseline.effect_opaque,
        matched: merged_matched,
        highlight_ranges: baseline.highlight_ranges.clone(),
        command: baseline.command.clone(),
        analysis: Some(AnalysisSummary {
            status: language.status,
            degradation_reasons: language.degradation_reasons.clone(),
        }),
    }
}

// Test modules live in `analysis/` (split out of this file to stay under the
// 800-line file-size budget — M5.1 quality gate).
#[cfg(test)]
mod merge_tests;
#[cfg(test)]
mod tests;

/// Shared language-aware operation classifier (plan Iteration 5, Slice 1).
pub mod classifier;

#[cfg(test)]
mod classifier_tests;
