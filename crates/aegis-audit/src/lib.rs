//! Append-only JSONL audit log with optional SHA-256 hash-chain integrity for Aegis.
//!
//! The central type is [`AuditLogger`], which appends [`AuditEntry`] records to
//! `~/.aegis/audit.jsonl`. Each entry is a single JSON line. Entries can be
//! read back via [`AuditLogger::read_all`] / [`AuditLogger::query`], and the
//! integrity chain can be verified with [`AuditLogger::verify_integrity`].

pub mod error;
pub mod logger;
mod secure_fs;
pub use error::AuditError;
pub use logger::{
    AuditEntry, AuditIntegrityReport, AuditIntegrityStatus, AuditLogger, AuditQuery,
    AuditRotationPolicy, AuditSnapshot, AuditSummary, AuditTimestamp, Decision, DecisionEntry,
    MatchedPattern, WatchEntry,
};
