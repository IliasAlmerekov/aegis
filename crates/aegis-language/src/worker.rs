//! Minimal parse-only worker experiment (Iteration 0).
//!
//! Parses the inline source of detected targets with the matching Tree-sitter
//! grammar. It is **parse-only**: no filesystem access, no subprocess, no
//! daemon, no socket (ADR-022). For no-source commands it does not start at
//! all — [`analyze`] returns [`Outcome::NotStarted`] — and therefore performs
//! zero filesystem metadata calls.
//!
//! The bounded ephemeral worker process (length-bounded framing, crash/hang
//! isolation, typed degradation) lands in Iteration 3; this in-process helper
//! exists to prove the no-source contract and that the four foundation grammars
//! parse inline source on the host build.

use crate::language::{self, SourceLanguage};
use crate::router::{self, SourceTarget};

/// The outcome of a parse-only worker experiment on one command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The command exposed no analyzable source targets, so the worker did not
    /// start and performed no work — and no filesystem metadata calls.
    ///
    /// This is the only outcome a no-source command may produce (ADR-022
    /// Iteration 0 RED #3).
    NotStarted,
    /// The worker started and parsed the inline source of `targets` targets.
    ///
    /// `targets` counts successfully parsed bodies; a body that fails to parse
    /// is dropped rather than failing the whole experiment, because typed
    /// degradation reasons are not introduced until Iteration 1.
    Parsed { targets: usize },
}

/// Analyze `command` with the parse-only worker experiment.
///
/// No-source commands return [`Outcome::NotStarted`] without any filesystem
/// access. Commands with inline source are parsed in-process with the matching
/// Tree-sitter grammar.
#[must_use]
pub fn analyze(command: &str) -> Outcome {
    let targets = router::source_targets(command);
    if targets.is_empty() {
        return Outcome::NotStarted;
    }
    Outcome::Parsed {
        targets: count_parseable(targets),
    }
}

/// Count the targets whose inline body parses with its declared grammar.
fn count_parseable(targets: Vec<SourceTarget>) -> usize {
    targets
        .iter()
        .filter(|t| parses(t.language, &t.source))
        .count()
}

/// Parse `source` as `language`, returning whether it produced a tree.
fn parses(language: SourceLanguage, source: &str) -> bool {
    language::parse(language, source).is_ok()
}
