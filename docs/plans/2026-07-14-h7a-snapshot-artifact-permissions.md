# H7a — Snapshot artifact permissions

## Status

Implemented and verified (TDD, 2026-07-15). Composes on top of the H6
containment seam.

## Finding

Database dumps and snapshot directories inherit process-umask defaults. Current
state:

- **PostgreSQL / MySQL** reserve dumps via `OpenOptions::create_new(true)` with
  no `.mode()` → umask default (typically `0644`, group/world-readable).
- **SQLite** uses `fs::copy(db_path, dump_path)` which on Unix copies the
  **source DB's permission bits** → the dump inherits the live DB mode (often
  `0644`).
- **Directories** are made with `create_dir_all` → umask default (`0755`,
  world-listable).
- **Supabase** already imports `PermissionsExt` but only *reads* mode; its
  manifest (`File::create`) and `artifacts_dir` are created with umask defaults.

Dumps may contain full database contents, secrets, or operator data and must not
be readable by unrelated local users.

## Design decisions (locked)

1. **Atomic mode-at-create, not create-then-chmod.** Use
   `OpenOptionsExt::mode(0o600)` / `DirBuilderExt::mode(0o700)` at creation to
   avoid a TOCTOU window where a secret dump is briefly world-readable. Explicit
   `.mode()` is a provable ceiling (`mode & ~umask ⊆ mode`) regardless of umask.
   - **SQLite** `fs::copy` cannot set mode (copies source perms) → replace with
     secure-open (`create_new` + `.mode(0o600)`) then stream the bytes.
   - Follow the create with an exact `set_permissions`/`fchmod` to `0600`/`0700`
     as a belt-and-suspenders guard against an exotic umask stripping owner-write
     (the pre-chmod window is always ⊆ the ceiling, so safe).
2. **Existing broad paths — tighten-or-reject by ownership:**
   - Snapshot store leaf we own (owner == current uid, not a symlink) and broader
     than contract → **tighten** to `0700` before writing.
   - Directory owned by another uid, a symlink, or unreadable metadata →
     **reject** (fail-closed).
   - Artifact *files* are always created fresh via `create_new` (collision →
     new suffix), so no existing-file tightening. The rollback **restore target**
     (`restore_path`, the live DB) is a user file — its permissions are never
     touched.
3. **Directory depth.** Every directory we create is made `0700` (recursive
   `DirBuilder` mode applies to all newly created components, incl. `~/.aegis` if
   we create it). Do **not** tighten pre-existing parents above the leaf —
   tighten-logic (decision 2) applies only to the snapshot store leaf. Narrowing
   an already-existing `~/.aegis` (also holds the audit log) is a separate
   surface, out of scope for H7a.
4. **Fail-closed before the sensitive write.** If secure-open fails, a
   post-create metadata check shows a mode broader than contract and tightening
   fails, or the directory is owned by another uid → error out **before**
   `pg_dump`/`mysqldump`/copy runs, so the secret never lands with bad perms.
   New typed error:
   ```rust
   SnapshotError::InsecureSnapshotPermissions {
       plugin: String,
       path: String,
       detail: String, // "directory owned by uid 1001", "mode 0644 could not be tightened"
   }
   ```
   Negative tests match the variant, not a string substring. On a
   permission-path failure after `create_new` already made the file, remove it
   (extends the existing PG/MySQL dump-failure cleanup).
5. **Non-Unix.** Secure-create helper is `#[cfg(unix)]` for the mode / chmod /
   owner-check logic; `#[cfg(not(unix))]` falls back to plain
   `create_new`/`create_dir_all` with a doc comment stating the permission
   contract does not apply and ACLs are out of scope. Permission tests are
   `#[cfg(unix)]`; add one platform-neutral round-trip test so the non-Unix
   branch cannot rot. Supported prod targets (Linux/macOS/WSL2) are all Unix.
6. **Centralization.** New `crates/aegis-snapshot/src/secure_fs.rs`, `pub(crate)`:
   ```rust
   pub(crate) fn create_store_dir(plugin: &'static str, path: &Path) -> Result<()>;      // 0700, tighten-or-reject leaf
   pub(crate) fn create_artifact_file(plugin: &'static str, path: &Path) -> Result<File>; // 0600, create_new
   ```
   Encapsulates the cfg(unix) logic, chmod guard, owner/symlink check, and
   `InsecureSnapshotPermissions`. Plugins call these instead of `create_dir_all`
   / `OpenOptions`.
   - **Composition with H6:** in reservation, resolve/verify the artifact path
     with the containment helper (H6) *first*, then create it via `secure_fs`
     (H7a). Order: containment → secure create.
   - **Supabase is in scope for H7a** (unlike H6, where its containment was
     already closed): apply `secure_fs` to its `artifacts_dir`, manifest write,
     and dump artifacts.
7. **Glossary.** Add one term to `CONTEXT.md` (§ Snapshot & Audit), e.g.
   **Owner-only artifact permissions** — the invariant that the `Snapshot store`
   and `Snapshot artifact`s are accessible only to the owner (`0700`/`0600`) on
   Unix. Builds on the H6 terms `Snapshot store` / `Snapshot artifact`.
8. **ADR-019** (after H6's ADR-018): restrictive permissions for filesystem
   snapshot artifacts — owner-only contract, atomic mode-at-create,
   tighten-or-reject, non-Unix no-op as a deliberate limitation. Update
   `docs/adr/README.md`.

## TDD seams

1. `snapshot()` creates an artifact whose Unix mode (`0600`) is asserted from
   filesystem metadata; store dir asserted `0700`.
2. Pre-existing broad store dir owned by us is tightened before the dump; broad
   dir owned by another uid / symlink is rejected with
   `InsecureSnapshotPermissions`.
3. Collision/retry (`create_new` suffix bump) preserves `0600`.
4. Injected mode-application failure prevents the sensitive write and cleans up.
5. Compatibility: SQLite dump no longer inherits a `0644` source-DB mode.

## Implementation sequence

1. Add failing mode tests for PostgreSQL and MySQL dump reservation (red).
2. Add `secure_fs` (cfg(unix) secure create + tighten-or-reject) +
   `InsecureSnapshotPermissions` (green).
3. Apply to directory creation and all local artifact reservations (PG, MySQL,
   SQLite copy→stream, Supabase manifest/artifacts).
4. Add SQLite/Supabase regressions and negative permission-error coverage.
5. Glossary term, ADR-019, CHANGELOG entry in the same change.

## Verification

- Focused snapshot permission tests on Unix
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
