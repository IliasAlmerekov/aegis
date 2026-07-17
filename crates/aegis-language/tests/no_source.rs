//! Iteration 0 RED #3 — no-source commands must not start the worker.
//!
//! Aegis's shell `Scanner` stays the hot path. Language-aware analysis is an
//! *additive slow path*: a command that exposes no analyzable source (no inline
//! interpreter script, no script file this prototype inspects) must not cause
//! the parse-only worker experiment to start, and must trigger zero filesystem
//! metadata calls. The worker experiment is parse-only and takes source by
//! `&str`, so it has no filesystem code path at all; this test pins the
//! observable half of that contract — for a corpus of no-source commands,
//! [`aegis_language::worker::analyze`] returns [`Outcome::NotStarted`] and
//! [`aegis_language::router::source_targets`] returns an empty vector.
//!
//! See ADR-022 and `docs/plans/2026-07-16-language-aware-analysis.md`
//! Iteration 0 RED #3.

use aegis_language::router::source_targets;
use aegis_language::worker::{Outcome, analyze};

// The no-source corpus is shared verbatim with `benches/no_source_bench.rs` so
// the test and the bench cannot drift.
#[path = "common/no_source_corpus.rs"]
mod no_source_corpus;
use no_source_corpus::NO_SOURCE;

#[test]
fn no_source_commands_yield_no_targets() {
    for cmd in NO_SOURCE {
        assert_eq!(
            source_targets(cmd),
            Vec::new(),
            "no-source command `{cmd}` must not expose analyzable source targets"
        );
    }
}

#[test]
fn no_source_commands_do_not_start_the_worker() {
    for cmd in NO_SOURCE {
        assert_eq!(
            analyze(cmd),
            Outcome::NotStarted,
            "no-source command `{cmd}` must not start the language worker"
        );
    }
}

#[test]
fn inline_interpreter_commands_do_start_the_worker() {
    // The positive control: commands that DO expose inline analyzable source
    // must produce targets and must NOT report NotStarted. This keeps the
    // no-source contract honest — it proves `NotStarted` is not the only
    // outcome the worker can return.
    let inline: &[(&str, &str)] = &[
        ("python3 -c \"import os\"", "python"),
        ("python -c 'print(1)'", "python"),
        ("bash -c \"echo hi\"", "bash"),
        ("sh -c 'ls -la'", "bash"),
        ("node -e \"console.log(1)\"", "javascript"),
    ];

    for (cmd, lang) in inline {
        let targets = source_targets(cmd);
        assert!(
            !targets.is_empty(),
            "inline command `{cmd}` must yield a target"
        );
        assert_eq!(
            targets[0].language.id(),
            *lang,
            "inline command `{cmd}` must map to language `{lang}`"
        );
        assert_ne!(
            analyze(cmd),
            Outcome::NotStarted,
            "inline command `{cmd}` must start the worker (not NotStarted)"
        );
    }
}
