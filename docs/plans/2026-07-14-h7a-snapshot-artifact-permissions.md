# H7a — Snapshot artifact permissions

## Status

Draft — requires a finding-specific `grill-with-docs` session after H6
establishes the trusted path seam and before TDD.

## Finding

Database dumps and snapshot directories can inherit process umask defaults.
Those artifacts may contain full database contents, secrets, or operator data and
must not be readable by unrelated local users.

## Scope

- Cover SQLite, PostgreSQL, MySQL, and Supabase local artifact directories/files.
- On Unix, create directories as `0700` and files as `0600` at reservation/open
  time; do not rely only on a later `set_permissions` repair.
- If an existing directory or file is broader than the contract, tighten it
  safely before writing or fail with an actionable error.
- Keep native Windows outside project scope; use conditional Unix APIs and
  document the behavior on supported macOS/Linux/WSL2 targets.
- Avoid a C-build dependency.

## TDD seams

- Public plugin `snapshot()` creates an artifact whose Unix mode is independently
  asserted from filesystem metadata.
- Pre-existing broad directories are tightened or rejected before a dump starts.
- Collision/retry behavior preserves restrictive modes.
- Failure to apply the required mode prevents a sensitive snapshot write.

## Implementation sequence

1. Add failing mode tests for PostgreSQL and MySQL dump reservation.
2. Add a small platform-gated secure-create helper in `aegis-snapshot`.
3. Apply it to directory creation and all local artifact reservations.
4. Add SQLite/Supabase regressions and negative permission-error coverage.

## Verification

- Focused snapshot permission tests on Unix
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
