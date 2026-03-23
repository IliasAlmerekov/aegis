---
name: add-pattern
description: Add a new detection pattern to the Aegis scanner — creates the pattern entry, registers it, and adds positive/negative test cases.
allowed_tools: ["Bash", "Read", "Write", "Edit", "Grep", "Glob"]
---

# /add-pattern

Use this workflow when adding a new detection pattern to `src/interceptor/patterns.rs`.

## Goal

Add a well-tested, benchmarked pattern to the Aegis scanner without introducing false positives or regressions.

## Common Files

- `src/interceptor/patterns.rs` — BuiltinPattern definitions
- `src/interceptor/scanner.rs` — assess() logic
- `tests/fixtures/commands.toml` — fixture test cases
- `benches/scanner_bench.rs` — criterion benchmarks

## Suggested Sequence

1. **Define the pattern** — determine `id`, `category`, `risk`, Aho-Corasick keyword(s), and optional regex for precision.
2. **Write evals first** — add at least 2 positive and 2 negative fixture cases to `tests/fixtures/commands.toml` before touching `patterns.rs`.
3. **Implement** — add `BuiltinPattern` entry in `patterns.rs`. Use `&'static str`. Follow existing ID format (`FS-001`, `GIT-003`).
4. **Verify match** — `rtk cargo test -- interceptor` — all new cases must pass.
5. **Check for false positives** — run the full fixture suite: `rtk cargo test`.
6. **Benchmark** — `rtk cargo criterion` — ensure safe-path latency stays under 2ms.
7. **Clippy clean** — `rtk cargo clippy -- -D warnings`.

## Eval Checklist

Before merging, confirm:

- [ ] Pattern fires on all positive fixtures (no false negatives)
- [ ] Pattern is silent on all negative fixtures (no false positives)
- [ ] `cargo test` green across the full suite
- [ ] `cargo criterion` safe-path p99 < 2ms
- [ ] `cargo clippy` zero warnings

## Typical Commit Signal

```
feat(patterns): add <CATEGORY>-<NNN> — <short description>
```

## Notes

- Aho-Corasick is the fast first pass — keep the keyword cheap (no regex in AC).
- If precision requires regex, add it as a secondary filter in `scanner.rs`, compiled via `LazyLock`.
- Do not use `once_cell` — use `std::sync::LazyLock`.
- Pattern IDs are part of the public audit log contract — never reuse a retired ID.
