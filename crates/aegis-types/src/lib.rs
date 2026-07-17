#![deny(missing_docs)]

//! Core data types shared across the Aegis crates.
//!
//! This crate is the foundation of the dependency DAG (Phase 4 of the
//! roadmap): it carries the pure data vocabulary — risk levels, the unified
//! pattern representation, and the human decision outcome — with no
//! dependencies on any other Aegis crate. Logic that *produces* these types
//! (the scanner, parser, policy engine, audit logger) lives in the crates that
//! depend on this one.

mod analysis;
mod assessment;
mod command;
mod decision;
mod pattern;
mod policy;
mod recovery;
mod risk;
mod sandbox;
mod snapshot;

pub use analysis::{
    AnalysisProvenance, AnalysisStatus, ByteSpan, DegradationReason, DetectedOperation,
    DetectionMechanism, DetectionSource, MatchEvidence, OperandCertainty, OperationKind,
    OperationModifiers, SourceOrigin, TargetAnalysis,
};
pub use assessment::{Assessment, AssessmentBasis, DecisionSource, HighlightRange, MatchResult};
pub use command::{InlineScript, ParsedCommand};
pub use decision::Decision;
pub use pattern::{Category, Pattern, PatternSource, PatternToken, PrefixPattern};
pub use policy::{AllowlistOverrideLevel, CiPolicy, Mode, PolicyRuleDecision, SnapshotPolicy};
pub use recovery::RecoveryDegradation;
pub use risk::RiskLevel;
pub use sandbox::SandboxStatus;
pub use snapshot::SnapshotRecord;

#[cfg(test)]
mod tests {
    /// SnapshotRecord must be re-exported from aegis_types so that aegis-tui
    /// can import it without depending on the root aegis crate.
    /// This test fails until SnapshotRecord is added to this crate and
    /// re-exported from lib.rs.
    #[test]
    fn test_snapshot_record_exported_from_aegis_types() {
        use crate::SnapshotRecord;
        let record = SnapshotRecord {
            plugin: "git",
            snapshot_id: "stash-ref-abc123".to_string(),
        };
        assert_eq!(record.plugin, "git");
    }
}
