# H6 — Snapshot path containment

## Status

Design locked (grill-with-docs, 2026-07-15). Ready for TDD.

## Finding

Filesystem snapshot plugins validate spelling (`absolute`, `..`) but do not
uniformly prove that the artifact used by rollback or deletion is contained in
the trusted snapshot store. A malicious or corrupted audit `snapshot_id`,
symlink, or sibling-prefix path can therefore redirect a destructive filesystem
action.

The SQLite hole is wider than the finding states: `rollback` runs
`fs::copy(dump_path, original_path)` where **both** paths come from the
(untrusted) `snapshot_id`. The attacker controls the read source *and* the write
destination; a well-formed absolute path without `..` (e.g.
`/home/victim/.ssh/authorized_keys`) passes the current lexical
`validate_snapshot_path`. `delete` has the same lexical-only gate before
`fs::remove_file` → arbitrary file deletion.

Supabase (`supabase/runtime/rollback.rs::resolve_db_artifact_path`) already does
containment correctly and is the reference behaviour.

## Design decisions (locked)

1. **Scope.** H6 closes *both* holes but with two distinct rules:
   - the **snapshot artifact** (the `fs::copy` source and the `delete` target)
     must be proven contained under its snapshot store (path containment);
   - the SQLite **restore destination** (`original_path`) is **not** trusted
     from `snapshot_id` at all — the plugin already knows its live DB path
     (`self.db_path` / `resolve_db_path`). The destination is taken from plugin
     configuration; the encoded original is at most a cross-check, never the
     write address.
2. **Trusted root = the plugin's own `self.snapshots_dir`**, canonicalized once.
   Not the global `resolve_snapshots_dir()` default (breaks temp-dir tests and
   custom stores). Matches Supabase's per-bundle `bundle_root`.
3. **Containment algorithm** (mirror Supabase, proven):
   1. canonicalize the store (catches a symlinked root);
   2. lexical fail-closed: reject absolute-outside / `..` / empty before any I/O;
   3. canonicalize the artifact **parent** and require
      `starts_with(store_canonical)` — catches symlink-escape even when the file
      does not exist;
   4. if the artifact exists, canonicalize it and re-check `starts_with`
      (catches a symlinked artifact);
   5. missing artifact → typed error (rollback: `RollbackDumpNotFound`; delete:
      idempotent no-op as today).
   Helper takes an **absolute** candidate (SQLite/PG/MySQL encode absolute
   artifact paths, unlike Supabase's relative one).
4. **Typed error.** New variant
   `SnapshotError::PathEscapesSnapshotStore { plugin, store, candidate }`.
   Negative tests match the variant, not a `Snapshot(String)` substring.
5. **Domain terms** added to `CONTEXT.md` (§ Snapshot & Audit):
   - **Snapshot store** — the trusted directory a plugin writes to / reads from
     (generalizes `snapshots_dir` and Supabase `bundle_root`).
     _Avoid_: snapshots dir, bundle root, snapshot root.
   - **Snapshot artifact** — the concrete file under the store that a
     `snapshot_id` addresses. _Avoid_: dump, blob.
   - **Path containment** — the invariant that a resolved `Snapshot artifact` is
     provably beneath its `Snapshot store`, including after symlink resolution.
6. **Legacy compatibility.** Containment is enforced uniformly, including legacy
   IDs. "Legitimate legacy" = artifact resolves inside the current store (still
   round-trips). A legacy ID pointing outside the store is now fail-closed —
   indistinguishable from forged. SQLite legacy rollback also writes to
   `self.db_path` (not the ID-encoded original): an explicit, safer behaviour
   change → CHANGELOG + ADR.
7. **Centralization.** New `crates/aegis-snapshot/src/containment.rs`:
   ```rust
   pub(crate) fn contain_artifact(
       plugin: &'static str,
       store: &Path,
       candidate: &Path,
   ) -> Result<PathBuf> // canonical, proven-under-store; else PathEscapesSnapshotStore
   ```
   SQLite/PG/MySQL replace their `validate_snapshot_path` with a call to this
   helper in `rollback`/`delete` (after decode, before `fs::copy`/`remove_file`).
   **Supabase is not touched in H6** — its containment is already closed and
   tested; migrating it onto the shared helper is a separate refactor.
8. **ADR.** Write `docs/adr/adr-018-*.md` (path containment for filesystem
   snapshot plugins): store = trusted root, artifact provably inside, destination
   never sourced from `snapshot_id`. Update `docs/adr/README.md`.

## TDD seams

1. Public plugin `rollback(snapshot_id)` / `delete(snapshot_id)` are the seams.
2. Positive: a snapshot created by the plugin restores/deletes normally.
3. Negative (fail before the outside target changes): `..`, absolute-outside,
   sibling-prefix store, symlink-to-outside (parent and artifact), and
   audit-derived forged IDs → `PathEscapesSnapshotStore`.
4. Compatibility: legacy IDs whose artifact is inside the store still resolve.

## Implementation sequence

1. Add one failing escape regression to the SQLite seam (red).
2. Extract `containment::contain_artifact` + `PathEscapesSnapshotStore` (green).
3. Apply vertically to PostgreSQL then MySQL.
4. Fix SQLite destination to use `self.db_path`, not the ID-encoded original.
5. Add glossary terms, ADR-018, CHANGELOG entry in the same change.

## Verification

- Focused `aegis-snapshot` tests for every filesystem plugin
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
