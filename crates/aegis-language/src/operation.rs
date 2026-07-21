//! Self-contained detected-operation vocabulary that language adapters emit
//! (ADR-022 §3).
//!
//! `aegis-language` is a workspace leaf and may **not** depend on `aegis-types`
//! (ADR-022 §4, pinned by `tests/aegis_language_boundary.rs`), so the operation
//! vocabulary an adapter *produces* lives here rather than reusing
//! `aegis_types::analysis`. The root `aegis` crate maps these types into
//! `aegis_types::analysis::DetectedOperation` and runs the shared classifier
//! (`aegis_types::analysis::classifier::classify`); no adapter assigns a final
//! `RiskLevel` directly (Iteration 5 REVIEW GATE).
//!
//! This is a deliberate, boundary-forced parallel of `aegis_types::analysis`:
//! `OperationKind`, `OperationModifiers`, `OperandCertainty`, `DetectedOperation`
//! mirror the `aegis-types` enums one-for-one. The duplication is structural —
//! the two crates cannot share the type — and is pinned by a conversion test in
//! the root `aegis` crate's pipeline tests, which assert every variant maps.
//!
//! The types carry no `serde`/`schemars` derives: this is the in-process adapter
//! output, not an audit-persisted record. Audit persistence goes through the
//! `aegis-types` provenance path after the root mapping. Keeping `serde` out
//! preserves the crate's stated dependency invariant (Tree-sitter runtime + four
//! grammars + `thiserror` only — see `tests/aegis_language_boundary.rs`).

use crate::language::SourceLanguage;

/// The kind of destructive effect or execution sink a language adapter detected
/// (ADR-022 §3 initial scope).
///
/// Mirrors `aegis_types::analysis::OperationKind` one-for-one. `#[non_exhaustive]`
/// so adapters may surface finer-grained kinds without breaking the root mapping.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationKind {
    /// Recursive or single filesystem deletion (`os.remove`, …).
    FilesystemDelete,
    /// Overwrite or truncation of an existing file (`open('w')`, …).
    FilesystemOverwrite,
    /// A dangerous permission or ownership change (`os.chown`, …).
    PermissionOrOwnershipChange,
    /// A write to a device file or other critical-path target.
    DeviceOrCriticalWrite,
    /// A destructive database operation.
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

/// Modifiers that refine an [`OperationKind`] (ADR-022 §3). Mirrors
/// `aegis_types::analysis::OperationModifiers`. All flags default to `false`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct OperationModifiers {
    /// The operation is recursive (`shutil.rmtree`, …).
    pub recursive: bool,
    /// The operation is forced.
    pub forced: bool,
    /// The operation is in an explicitly destructive mode (e.g. a truncating
    /// `open` flag).
    pub destructive_mode: bool,
}

/// How completely a `Detected operation`'s operand is known to static analysis
/// (ADR-022 §3). Mirrors `aegis_types::analysis::OperandCertainty`.
///
/// Ordered by *decreasing* certainty: `Known < Partial < Dynamic`. A `Dynamic`
/// operand is never evidence of safety (ADR-022 §3, §7).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OperandCertainty {
    /// The operand is a literal recoverable from the source.
    Known,
    /// The operand is partially resolved (e.g. an alias or adjacent literal).
    Partial,
    /// The operand is computed, imported, or otherwise not statically recoverable.
    Dynamic,
}

/// A concrete byte span inside analyzed source (ADR-022 §10). Mirrors
/// `aegis_types::analysis::ByteSpan`. Carries position only — never source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// A literal payload statically recovered from an execution sink, to be enqueued
/// as a bounded recursive analysis target (ADR-022 §7).
///
/// `language` is the payload's *own* language (cross-language: a Python `eval`
/// of a JavaScript literal enqueues a JavaScript target). `source` is the
/// recovered literal body; `span` locates the payload in the parent source for
/// provenance. The root crate wraps this in a `QueueTarget` at `parent_depth+1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedTarget {
    /// The language the payload should be parsed as.
    pub language: SourceLanguage,
    /// The recovered literal payload body.
    pub source: String,
    /// Where the payload was recovered in the parent source.
    pub span: ByteSpan,
}

/// A language-neutral operation detected from source syntax (ADR-022 §3).
///
/// Mirrors `aegis_types::analysis::DetectedOperation` plus an adapter-local
/// `span` (the `aegis-types` type is span-less; the span lives on its
/// `AnalysisProvenance`) and an optional [`NestedTarget`] payload for execution
/// sinks. The root mapping moves `span` into `AnalysisProvenance` and feeds
/// `payload` to the recursive sink invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedOperation {
    /// What effect or execution sink was detected.
    pub kind: OperationKind,
    /// Modifiers refining the kind.
    pub modifiers: OperationModifiers,
    /// How completely the operand is known to static analysis.
    pub certainty: OperandCertainty,
    /// Where the operation appears in the source.
    pub span: ByteSpan,
    /// For a `CodeExecution` sink: the statically recovered literal payload, if
    /// any. `None` for non-execution ops and for dynamic/encoded payloads.
    pub payload: Option<NestedTarget>,
}

/// The output of one language adapter over one source target.
///
/// `operations` are the detected destructive effects / execution sinks;
/// `parse_errors` is the count of Tree-sitter `ERROR` nodes in the parse (a
/// nonzero count means the source was malformed, which the root mapping records
/// as `DegradationReason::IncompleteSyntax`). The parent owns status/degradation
/// aggregation and recursive enqueueing (ADR-022 §2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdapterResult {
    /// Detected operations, in source order.
    pub operations: Vec<DetectedOperation>,
    /// Number of Tree-sitter `ERROR` nodes in the parse (0 = clean).
    pub parse_errors: u32,
}
