# H6 — Snapshot path containment

## Status

Draft — requires a finding-specific `grill-with-docs` session before TDD.

## Finding

Filesystem snapshot plugins validate spelling (`absolute`, `..`) but do not
uniformly prove that the artifact used by rollback or deletion is contained in
the trusted snapshot root. A malicious or corrupted audit `snapshot_id`, symlink,
or sibling-prefix path can therefore redirect a destructive filesystem action.

## Scope

- Inventory every plugin whose opaque `snapshot_id` resolves to a local path:
  SQLite, PostgreSQL, MySQL, and Supabase artifacts; include cleanup paths.
- Centralize the containment rule in `aegis-snapshot` where it can be shared
  without depending on the root binary.
- Validate the canonical existing parent and reject symlink escape before open,
  overwrite, delete, or rename.
- Treat a missing, malformed, or unresolvable artifact as a typed rollback error.
- Preserve non-path snapshot IDs (Git refs/hashes, Docker identifiers).

## TDD seams

1. Public plugin `rollback(snapshot_id)` is the behavior seam.
2. Positive: a snapshot created by the plugin restores normally.
3. Negative: `..`, absolute paths, sibling-prefix roots, symlink-to-outside, and
   audit-derived malformed IDs fail before the outside target changes.
4. Compatibility: legacy legitimate IDs still resolve.

## Implementation sequence

1. Add one failing escape regression to the SQLite seam.
2. Extract the smallest shared containment helper with a typed error.
3. Apply it vertically to PostgreSQL/MySQL and then Supabase cleanup/rollback.
4. Review create-time reservation together with H7a permissions, but do not
   combine the tasks: containment and confidentiality have separate closure
   criteria.

## Verification

- Focused `aegis-snapshot` tests for every filesystem plugin
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
