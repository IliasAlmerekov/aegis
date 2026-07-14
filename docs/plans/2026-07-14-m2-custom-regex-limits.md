# M2 — Custom regex resource limits

## Status

Draft — requires a finding-specific `grill-with-docs` session before TDD.

## Finding

Regexes loaded from user or project config have no explicit input-length,
compiled-size, or DFA-size budgets. A repository can therefore impose excessive
compile-time memory/CPU before Aegis reaches its normal command hot path.

## Scope

- Identify every untrusted regex compiler: custom scanner patterns, legacy
  allowlist patterns, and any policy/config validation path.
- Define one documented pattern-length cap and shared `RegexBuilder` size/DFA
  limits.
- Reject oversize or over-budget expressions during config validation/loading
  with layer/source context.
- Keep built-in regex construction separate if its trusted budgets differ.
- Do not introduce the `regex` crate into the Aho-Corasick quick first pass.

## TDD seams

- `aegis config validate` rejects a pattern over the length cap.
- A short expression that exceeds compiled limits fails closed with actionable
  context rather than panicking.
- Representative legitimate custom patterns continue to compile and match.
- Project-layer malicious config cannot silently drop the invalid rule and
  continue with weaker behavior.

## Implementation sequence

1. Add a failing config-validation length test.
2. Introduce shared constants and a bounded builder at the config/scanner seam.
3. Route every untrusted compiler through it.
4. Add compiled-budget and compatibility tests; benchmark only if hot-path work
   changes beyond startup/config load.

## Verification

- Focused config/scanner tests
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
