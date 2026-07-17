//! Iteration 0 RED #3 — benchmark harness for the no-source contract.
//!
//! A no-source command must not start the language worker (ADR-022; plan
//! Iteration 0 RED #3). This criterion harness measures the no-source path of
//! [`aegis_language::worker::analyze`] over a corpus of no-source commands and
//! asserts each one yields [`Outcome::NotStarted`]. If a regression ever makes
//! a no-source command start the worker, the assertion panics inside the
//! iteration body, the bench exits non-zero, and the CI perf job fails.
//!
//! The contract is also pinned by a fast `#[test]` in `tests/no_source.rs`
//! (runs in `cargo test --workspace` on every PR); this harness additionally
//! keeps the no-source path on the performance-regression wall so a slow path
//! that quietly grew filesystem metadata calls is caught before main.

use aegis_language::worker::{Outcome, analyze};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::time::Duration;

// The no-source corpus is shared verbatim with `tests/no_source.rs` (a
// `tests/common/` subdirectory file, not its own test target) so the bench and
// the contract test cannot drift. `include!` inlines the `pub const NO_SOURCE`.
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/common/no_source_corpus.rs"
));

fn no_source_does_not_start_worker(c: &mut Criterion) {
    c.bench_function("no_source_does_not_start_worker", |b| {
        b.iter(|| {
            for cmd in NO_SOURCE {
                let outcome = analyze(black_box(*cmd));
                assert_eq!(
                    outcome,
                    Outcome::NotStarted,
                    "no-source command `{cmd}` started the language worker — \
                     regression of ADR-022 Iteration 0 RED #3 (the no-source \
                     path must not spawn a worker or perform filesystem metadata \
                     calls)"
                );
            }
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().measurement_time(Duration::from_secs(3));
    targets = no_source_does_not_start_worker
}
criterion_main!(benches);
