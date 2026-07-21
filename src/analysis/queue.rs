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
use aegis_types::DegradationReason;
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
/// UTF-8 source body the parent will send to the worker; `source_hash` is the
/// hex SHA-256 of `source.as_bytes()`. This matches `source_reader`'s hash for
/// sources without a UTF-8 BOM, so an inline target and a BOM-free script-file
/// target over the same body collapse. `source_reader::read_script_file` hashes
/// the raw file bytes *before* stripping a BOM, so a BOM-prefixed script file
/// will not collapse with an inline target built from the post-strip body; the
/// queue is internally consistent either way (it always hashes the post-strip
/// `String`), so in-session dedup is unaffected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueTarget {
    /// The language the source should be parsed as.
    pub language: SourceLanguage,
    /// The source body to analyze.
    pub source: String,
    /// Recursion depth (root = 0).
    pub depth: u32,
    /// Hex SHA-256 of `source`.
    pub source_hash: String,
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
        }
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
