//! Parent-owned recursive analysis work queue (ADR-022 §7, plan Iteration 5
//! Slice 2).
//!
//! The parent owns recursive routing (ADR-022 §2). When an adapter detects a
//! literal process/eval payload, that payload becomes a new analysis target;
//! this queue is the bounded, deduplicated work queue those nested targets
//! enter. Targets are deduplicated by `(language, source_hash)` so a cycle or
//! a repeated target is analyzed once. Depth, target count, aggregate bytes,
//! and the session deadline are capped; exceeding any cap records
//! [`DegradationReason::LimitExceeded`] (ADR-022 §4) while preserving the
//! Matches already produced (ADR-022 §7).
//!
//! The queue is a pure in-memory control structure: it holds source bytes only
//! up to the aggregate-byte ceiling (≤ 1 MiB by default) and performs no I/O,
//! no worker spawn, and no filesystem access.

use std::collections::HashSet;
use std::time::Instant;

use aegis_language::SourceLanguage;
use aegis_types::{DegradationReason, SourceOrigin};
use sha2::{Digest, Sha256};

/// Hard ceilings for the recursive analysis work queue (ADR-022 §7).
///
/// `deadline` is caller-set (the parent owns the total language-analysis time
/// budget via `tokio::time::timeout`); it is `None` in [`QueueBudget::L1_DEFAULT`]
/// and the parent supplies a concrete `Instant` at queue construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueBudget {
    /// Maximum recursion depth (ADR-022 §7 ceiling: 8).
    pub max_depth: u32,
    /// Maximum number of distinct accepted targets.
    pub max_targets: usize,
    /// Maximum aggregate source bytes across all accepted targets.
    pub max_aggregate_bytes: usize,
    /// Optional wall-clock deadline for the whole analysis session.
    pub deadline: Option<Instant>,
}

impl QueueBudget {
    /// ADR-022 §7 pre-1.0 ceilings. `deadline` is `None` — the parent sets it.
    ///
    /// `max_targets` (16) exceeds `max_depth` (8) so a linear depth-8 chain
    /// (9 targets) is bounded by depth, the semantically meaningful recursion
    /// guard, rather than by count.
    pub const L1_DEFAULT: QueueBudget = QueueBudget {
        max_depth: 8,
        max_targets: 16,
        max_aggregate_bytes: 1024 * 1024,
        deadline: None,
    };
}

/// One recursive analysis target awaiting worker analysis.
///
/// Deduplicated by `(language, source_hash)` (ADR-022 §7). `source` is the
/// UTF-8 source body the parent sends to the worker. Inline targets hash
/// `source.as_bytes()`; script-file targets replace that value with the
/// source-reader hash over original bytes. A BOM-prefixed script therefore
/// remains distinct from an otherwise identical inline body, while provenance
/// retains the required original-byte hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueTarget {
    /// The language the source should be parsed as.
    pub language: SourceLanguage,
    /// The source body to analyze.
    pub source: String,
    /// Recursion depth (root = 0).
    pub depth: u32,
    /// Hex SHA-256 used for deduplication and provenance: computed from
    /// `source` for inline targets, replaced by the original-byte hash for
    /// script-file targets.
    pub source_hash: String,
    /// Where the source entered analysis.
    pub source_origin: SourceOrigin,
    /// Script path metadata when the source came from a file.
    pub file_path: Option<String>,
    /// Leading original bytes stripped before parsing (currently UTF-8 BOM).
    pub source_byte_offset: usize,
}

impl QueueTarget {
    /// Construct a target, computing its hex SHA-256 `source_hash`.
    #[must_use]
    pub fn new(language: SourceLanguage, source: String, depth: u32) -> Self {
        let source_hash = format!("{:x}", Sha256::digest(source.as_bytes()));
        Self {
            language,
            source,
            depth,
            source_hash,
            source_origin: SourceOrigin::Inline,
            file_path: None,
            source_byte_offset: 0,
        }
    }

    /// Attach privacy-safe top-level source provenance.
    #[must_use]
    pub fn with_provenance(
        mut self,
        source_origin: SourceOrigin,
        file_path: Option<String>,
    ) -> Self {
        self.source_origin = source_origin;
        self.file_path = file_path;
        self
    }

    /// Preserve a source-reader hash computed over the original file bytes.
    #[must_use]
    pub fn with_source_hash(mut self, source_hash: Option<String>) -> Self {
        if let Some(source_hash) = source_hash {
            self.source_hash = source_hash;
        }
        self
    }

    /// Record the original-byte offset stripped before parsing.
    #[must_use]
    pub fn with_source_byte_offset(mut self, source_byte_offset: usize) -> Self {
        self.source_byte_offset = source_byte_offset;
        self
    }
}

/// The outcome of pushing a target onto the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// The target was accepted for analysis.
    Accepted,
    /// The target was already seen (same language + hash) — skipped, not
    /// degradation.
    DuplicateSkipped,
    /// The target exceeded the recursion-depth ceiling.
    DepthExceeded,
    /// The target exceeded the target-count ceiling.
    CountExceeded,
    /// The target exceeded the aggregate-byte ceiling.
    BytesExceeded,
    /// The session deadline has passed.
    DeadlineExceeded,
}

impl PushOutcome {
    /// Whether this outcome accepted the target.
    #[must_use]
    pub fn is_accepted(self) -> bool {
        matches!(self, PushOutcome::Accepted)
    }

    /// The typed degradation reason this outcome records, if any.
    ///
    /// Every budget cap maps to [`DegradationReason::LimitExceeded`]
    /// (ADR-022 §4 — "a size, file-count, recursion-depth, or timeout limit was
    /// exceeded"). [`PushOutcome::Accepted`] and [`PushOutcome::DuplicateSkipped`]
    /// record no degradation: acceptance is normal flow, and a duplicate was
    /// already analyzed.
    #[must_use]
    pub fn degradation_reason(self) -> Option<DegradationReason> {
        match self {
            PushOutcome::DepthExceeded
            | PushOutcome::CountExceeded
            | PushOutcome::BytesExceeded
            | PushOutcome::DeadlineExceeded => Some(DegradationReason::LimitExceeded),
            PushOutcome::Accepted | PushOutcome::DuplicateSkipped => None,
        }
    }
}

/// A parent-owned, deduplicated, budget-bounded recursive analysis work queue.
pub struct AnalysisQueue {
    budget: QueueBudget,
    seen: HashSet<(&'static str, String)>,
    pending: Vec<QueueTarget>,
    accepted_count: usize,
    accepted_bytes: usize,
}

impl AnalysisQueue {
    /// Construct a new empty queue with the given budget.
    #[must_use]
    pub fn new(budget: QueueBudget) -> Self {
        AnalysisQueue {
            budget,
            seen: HashSet::new(),
            pending: Vec::new(),
            accepted_count: 0,
            accepted_bytes: 0,
        }
    }

    /// Push a target. Deduplicates by `(language, source_hash)` and enforces
    /// the depth, count, aggregate-byte, and deadline budgets.
    ///
    /// A duplicate is always skipped (it is already-analyzed, never new work),
    /// even after the deadline has passed. After a duplicate, the remaining
    /// checks run in order: deadline, depth, count, bytes.
    pub fn push(&mut self, target: QueueTarget) -> PushOutcome {
        let key = (target.language.id(), target.source_hash.clone());
        if self.seen.contains(&key) {
            return PushOutcome::DuplicateSkipped;
        }
        if self
            .budget
            .deadline
            .is_some_and(|deadline| Instant::now() > deadline)
        {
            return PushOutcome::DeadlineExceeded;
        }
        if target.depth > self.budget.max_depth {
            return PushOutcome::DepthExceeded;
        }
        if self.accepted_count >= self.budget.max_targets {
            return PushOutcome::CountExceeded;
        }
        let new_bytes = self.accepted_bytes + target.source.len();
        if new_bytes > self.budget.max_aggregate_bytes {
            return PushOutcome::BytesExceeded;
        }

        self.seen.insert(key);
        self.accepted_count += 1;
        self.accepted_bytes = new_bytes;
        self.pending.push(target);
        PushOutcome::Accepted
    }

    /// Pop the next target to analyze (LIFO — depth-first recursion).
    pub fn pop(&mut self) -> Option<QueueTarget> {
        self.pending.pop()
    }

    /// Whether no targets remain pending.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Number of targets accepted so far (excludes duplicates and rejections).
    #[must_use]
    pub fn accepted_count(&self) -> usize {
        self.accepted_count
    }

    /// Aggregate source bytes across accepted targets.
    #[must_use]
    pub fn accepted_bytes(&self) -> usize {
        self.accepted_bytes
    }
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
