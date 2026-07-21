//! Shared language-aware operation classifier (ADR-022 §3, plan Iteration 5).
//!
//! This is the single place that maps a [`DetectedOperation`] — emitted by any
//! language adapter — into the existing Aegis `Category` / `RiskLevel` /
//! `Match` vocabulary. No adapter assigns a final `RiskLevel` directly or keeps
//! a private copy of this semantics (Iteration 5 REVIEW GATE); every adapter
//! routes through [`classify`].
//!
//! Invariants pinned by `classifier_tests`:
//! - The risk mapping is **certainty-independent**. A `Dynamic` operand never
//!   lowers risk below a `Known` one (ADR-022 §3/§7); certainty governs
//!   recursive enqueueing and degradation, which are the queue's concern.
//! - [`classify`] never returns [`RiskLevel::Block`]. Language-aware Matches are
//!   non-`Block` by ADR-022 §5; `Block` is reserved for intrinsic shell-level
//!   denials and remains unbypassable.
//! - [`OperationKind`] is matched exhaustively so adding a variant forces a
//!   classification here rather than silently falling through.

use std::borrow::Cow;
use std::sync::Arc;

use crate::analysis::{AnalysisProvenance, DetectedOperation, MatchEvidence};
use crate::pattern::{Pattern, PatternSource};
use crate::risk::RiskLevel;
use crate::{Category, DetectionSource, HighlightRange, MatchResult};

/// The shared classification of a [`DetectedOperation`] into the existing
/// Aegis vocabulary (ADR-022 §3).
///
/// `rule_id` is a stable detection rule identifier (ADR-022 §10) in the
/// `LANG-*` namespace, distinct from the shell scanner's `FS-001`-style ids.
/// `description` and `safe_alt` are built-in static strings: language-aware
/// rules are always built in (ADR-022 §4 — project config cannot define custom
/// Tree-sitter queries), so they never carry runtime strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    /// The semantic category the detected operation maps to.
    pub category: Category,
    /// The risk level the detected operation maps to.
    pub risk: RiskLevel,
    /// Stable detection rule id (`LANG-*`).
    pub rule_id: &'static str,
    /// Human-readable description of what was detected.
    pub description: &'static str,
    /// A safer alternative to suggest, if any.
    pub safe_alt: Option<&'static str>,
}

/// Classify a [`DetectedOperation`] into the shared Aegis vocabulary.
///
/// The risk mapping is **certainty-independent**: a `Dynamic` operand never
/// lowers risk below a `Known` one (ADR-022 §3/§7). Certainty governs whether a
/// recursive target is enqueued and whether degradation is recorded — that is
/// the queue's job, not this function's. This function never returns
/// [`RiskLevel::Block`]; language-aware Matches are non-`Block` (ADR-022 §5).
#[must_use]
pub fn classify(op: &DetectedOperation) -> Classification {
    use crate::analysis::OperationKind;

    let kind = op.kind;
    let mods = op.modifiers;

    match kind {
        OperationKind::FilesystemDelete => {
            let (rule_id, risk, description, safe_alt) = if mods.recursive && mods.forced {
                (
                    "LANG-FS-DEL-RF",
                    RiskLevel::Danger,
                    "Recursive, forced filesystem deletion detected in source",
                    Some("Remove a specific path with a bounded, non-recursive call"),
                )
            } else if mods.recursive {
                (
                    "LANG-FS-DEL-R",
                    RiskLevel::Danger,
                    "Recursive filesystem deletion detected in source",
                    Some("Remove a specific path with a bounded, non-recursive call"),
                )
            } else if mods.forced {
                (
                    "LANG-FS-DEL-F",
                    RiskLevel::Warn,
                    "Forced filesystem deletion detected in source",
                    Some("Remove the path without --force and confirm the target first"),
                )
            } else {
                (
                    "LANG-FS-DEL",
                    RiskLevel::Warn,
                    "Filesystem deletion detected in source",
                    Some("Confirm the exact path before deleting"),
                )
            };
            Classification {
                category: Category::Filesystem,
                risk,
                rule_id,
                description,
                safe_alt,
            }
        }
        OperationKind::FilesystemOverwrite => {
            let (rule_id, description) = if mods.destructive_mode {
                (
                    "LANG-FS-OVR-W",
                    "Destructive-mode file overwrite or truncation detected in source",
                )
            } else {
                ("LANG-FS-OVR", "File overwrite detected in source")
            };
            Classification {
                category: Category::Filesystem,
                risk: RiskLevel::Warn,
                rule_id,
                description,
                safe_alt: Some("Write to a new path or back up the existing file first"),
            }
        }
        OperationKind::PermissionOrOwnershipChange => Classification {
            category: Category::Filesystem,
            risk: RiskLevel::Danger,
            rule_id: "LANG-FS-CHMOD",
            description: "Permission or ownership change detected in source",
            safe_alt: Some("Apply the least-privileged mode to a specific path"),
        },
        OperationKind::DeviceOrCriticalWrite => Classification {
            category: Category::Filesystem,
            risk: RiskLevel::Danger,
            rule_id: "LANG-FS-DEV",
            description: "Write to a device file or critical-path target detected in source",
            safe_alt: Some("Avoid raw device writes; use the intended filesystem tool"),
        },
        OperationKind::DatabaseDestructive => Classification {
            category: Category::Database,
            risk: RiskLevel::Danger,
            rule_id: "LANG-DB-DEST",
            description: "Destructive database operation detected in source",
            safe_alt: Some("Run a reversible migration or back up the database first"),
        },
        OperationKind::CodeExecution => Classification {
            category: Category::Process,
            risk: RiskLevel::Danger,
            rule_id: "LANG-EXEC",
            description: "Process, shell, or eval execution sink detected in source",
            safe_alt: Some("Run a known, bounded command instead of arbitrary code"),
        },
        OperationKind::CloudDestructive => Classification {
            category: Category::Cloud,
            risk: RiskLevel::Danger,
            rule_id: "LANG-CLOUD-DEST",
            description: "Destructive cloud-provider API call detected in source",
            safe_alt: Some("Target a specific named resource and confirm before deletion"),
        },
        OperationKind::ContainerDestructive => Classification {
            category: Category::Docker,
            risk: RiskLevel::Danger,
            rule_id: "LANG-DOCKER-DEST",
            description: "Destructive container-management operation detected in source",
            safe_alt: Some("Remove a specific named container or image after confirmation"),
        },
        OperationKind::PackageDestructive => Classification {
            category: Category::Package,
            risk: RiskLevel::Warn,
            rule_id: "LANG-PKG-DEST",
            description: "Destructive package-manager operation detected in source",
            safe_alt: Some("Remove a specific named package after confirmation"),
        },
    }
}

/// Build a [`MatchResult`] carrying [`MatchEvidence::LanguageRule`] for a
/// detected operation and its metadata-only provenance.
///
/// The resulting [`Pattern`] is always built in ([`PatternSource::Builtin`])
/// with the stable `LANG-*` rule id, the classified category and risk, and an
/// empty pattern string (language rules have no regex). The evidence carries
/// the detected operation and provenance; the caller supplies the matched
/// source text and optional highlight range.
#[must_use]
pub fn language_match(
    op: &DetectedOperation,
    provenance: AnalysisProvenance,
    matched_text: &str,
    highlight_range: Option<HighlightRange>,
) -> MatchResult {
    let class = classify(op);
    let pattern = Arc::new(Pattern {
        id: Cow::Borrowed(class.rule_id),
        category: class.category,
        risk: class.risk,
        pattern: Cow::Borrowed(""),
        description: Cow::Borrowed(class.description),
        safe_alt: class.safe_alt.map(Cow::Borrowed),
        justification: None,
        source: PatternSource::Builtin,
    });
    MatchResult {
        pattern,
        matched_text: matched_text.to_string(),
        highlight_range,
        evidence: MatchEvidence::LanguageRule {
            source: DetectionSource::Builtin,
            operation: op.clone(),
            provenance,
        },
    }
}
