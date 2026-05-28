//! Audit layer: append-only JSONL log with optional hash-chain integrity.

pub mod logger;

pub use logger::{
    AuditEntry, AuditIntegrityReport, AuditIntegrityStatus, AuditLogger, AuditQuery,
    AuditRotationPolicy, AuditSnapshot, AuditSummary, AuditTimestamp, Decision, DecisionEntry,
    MatchedPattern, WatchEntry,
};
