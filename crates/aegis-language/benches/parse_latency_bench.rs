//! Iteration 0 GREEN measurement — parse latency per foundation grammar.
//!
//! ADR-022 §8 + the Iteration 0 GREEN list require *parse latency* to be
//! measured, not just the no-source path. This harness parses one
//! representative inline-source snippet per foundation grammar with the pinned
//! Tree-sitter runtime and records the per-grammar parse latency. It is a
//! measurement bench, not an asserting one — it parses real source so the
//! number reflects the cost the slow path pays when an inline interpreter
//! target is detected.
//!
//! The snippets are small but non-trivial (imports, a function, a loop, a
//! conditional) so the number is representative rather than a degenerate
//! single-statement parse.

use aegis_language::{SourceLanguage, parse};
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use std::time::Duration;

/// One representative inline-source snippet per foundation grammar. These are
/// realistic small programs, not single statements, so parse latency reflects a
/// plausible inline target rather than a best-case trivial parse.
const SNIPPETS: &[(SourceLanguage, &str, &str)] = &[
    (
        SourceLanguage::Python,
        "python",
        "import os\n\
         def walk(root):\n\
         \tfor name in os.listdir(root):\n\
         \t\tpath = os.path.join(root, name)\n\
         \t\tif os.path.isdir(path):\n\
         \t\t\tyield from walk(path)\n\
         \t\telse:\n\
         \t\t\tyield path\n",
    ),
    (
        SourceLanguage::JavaScript,
        "javascript",
        "function sum(xs) {\n\
         \tlet total = 0;\n\
         \tfor (const x of xs) {\n\
         \t\ttotal += x;\n\
         \t}\n\
         \treturn total;\n\
         }\n\
         console.log(sum([1, 2, 3]));\n",
    ),
    (
        SourceLanguage::TypeScript,
        "typescript",
        "function sum(xs: number[]): number {\n\
         \tlet total: number = 0;\n\
         \tfor (const x of xs) {\n\
         \t\ttotal += x;\n\
         \t}\n\
         \treturn total;\n\
         }\n\
         console.log(sum([1, 2, 3]));\n",
    ),
    (
        SourceLanguage::Bash,
        "bash",
        "set -euo pipefail\n\
         for f in \"$@\"; do\n\
         \tif [ -f \"$f\" ]; then\n\
         \t\techo \"file: $f\"\n\
         \tfi\n\
         done\n",
    ),
];

fn parse_latency_per_grammar(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_latency_per_grammar");
    group.measurement_time(Duration::from_secs(3));
    for &(language, label, source) in SNIPPETS {
        group.bench_with_input(BenchmarkId::new("parse", label), &source, |b, &source| {
            b.iter(|| {
                let tree = parse(language, black_box(source))
                    .expect("a foundation-grammar snippet must parse without error");
                assert!(
                    !tree.root_node().has_error(),
                    "snippet for {label} had parse errors"
                );
            })
        });
    }
    group.finish();
}

criterion_group!(benches, parse_latency_per_grammar);
criterion_main!(benches);
