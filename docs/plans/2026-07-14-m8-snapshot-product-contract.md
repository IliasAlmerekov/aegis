# M8 — Snapshot product-contract alignment

## Status

Draft — requires a finding-specific `grill-with-docs` session before a docs/UX
TDD slice. This plan deliberately rejects building a general backup system for
1.0.

## Finding

A Git `Snapshot` uses a pre-execution stash. It preserves the working-tree delta
that exists at snapshot time; it does not capture a later command's deletion of
clean tracked files. Other plugins cover only their own domains, and a command
may have no applicable plugin. Wording that promises “undo the dangerous action”
or universal rollback is false.

## Product boundary

- `Snapshot`: best-effort pre-execution capture produced by an applicable plugin.
- `Rollback`: restore the state that the plugin captured, not reverse arbitrary
  post-snapshot effects.
- Required recovery under ADR-016 can fail closed, but that does not make an
  individual plugin a complete backup.
- Targeted file copies, filesystem snapshots, and universal backup discovery are
  out of scope.

## Scope

1. Inventory README, TUI/explanations, CLI help, glossary, threat model, config
   docs, release docs, and landing copy for undo/backup/complete-recovery claims.
2. State plugin applicability and the captured-state boundary at the decision
   point where users see snapshot information.
3. Surface “no applicable snapshot plugin” accurately; coordinate with H9 when
   recovery is required and with M9 for the rollback command path.
4. Keep snapshot mechanics and serialized audit compatibility unchanged unless a
   user-visible status is missing.

## TDD seams

- Documentation-contract tests reject universal undo/backup claims.
- Explanation/TUI tests name the plugin and pre-execution captured state.
- No-plugin flows state that no snapshot was created.
- Existing snapshot/rollback integration tests remain green.

## Verification

- Focused docs/explanation/snapshot tests
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
