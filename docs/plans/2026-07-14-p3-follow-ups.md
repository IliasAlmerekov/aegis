# P3 — Consolidated low-priority follow-up plan

## Status

Deferred until the ordered 1.0 blockers are closed or new evidence promotes an
item. Each item still requires its own grill/TDD slice before implementation.

## P3-1 — SQLite snapshot creation TOCTOU

Replace separated existence-check/copy behavior with atomic destination
reservation (`create_new` or equivalent). Test collision and outside-target
non-modification. Coordinate with H6/H7a helpers rather than inventing a second
path/permission policy.

## P3-2 — Backslash-newline tokenization

Build a small truth table from real supported shell behavior, then add parser and
scanner examples. Keep full shell evaluation outside scope per ADR-010.

## P3-3 — Parameterized IFS expansion

Grill whether `${IFS:-x}` / `${IFS:+x}` merit another bounded literal normalizer
or remain effect-opaque. Runtime `IFS=` state tracking is not implied by C2 and
must not be added without revisiting ADR-010.

## P3-4 — Renderer fallback

Replace implicit wildcard approval with exhaustive matching or a fail-closed
fallback. Add a compile-time/source contract or focused test that a new risk value
cannot auto-approve.

## P3-5 — Sandbox status TOCTOU

Derive `SandboxStatus` from the confinement operation actually used for
execution, then audit that result. Coordinate terminology and protocol behavior
with M1.

## P3-6 — Current-directory fallback

Remove ambiguous `.` fallback on `current_dir()` error from snapshot planning.
Test that the command does not execute with a snapshot planned against an unknown
directory.

## P3-7 — Optional Starlark advisories

Keep `deny.toml` exceptions scoped to the opt-in dependency chain and record
upstream status. Remove/replace only when a supported migration exists; do not
weaken default `cargo audit`/`cargo deny` gates.

## P3-8 — Destructive SQL follow-ups

Evaluate comment separators, `DROP VIEW`, `DROP INDEX`, and the `dropdb` client as
separate rule slices. Embedded SQL follows ADR-015; `dropdb` is program-led and
follows ADR-014. A full SQL parser remains out of scope.

## Verification for any promoted slice

- Focused RED → GREEN behavior test at the public seam
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
- benchmark when the scanner hot path changes
