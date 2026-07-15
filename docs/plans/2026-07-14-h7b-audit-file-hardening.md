# H7b — Audit artifact and directory hardening

## Status

Implemented and verified locally via TDD and review/re-review (2026-07-15).
H7a's landed Unix secure-create idiom was refreshed and ADR-020 assigned; H7b
remains open only until the required PR CI checks complete.

## Finding

Audit directory, active log, rotated segments, and lock-file opens rely on default
filesystem modes and ordinary path-following. Because audit is a security
artifact, a broad mode or symlink target can expose or redirect append operations.

## Scope

- Cover directories Aegis creates while materializing the audit path, the active
  JSONL, rotated segments, and `.lock` file.
- On supported Unix targets, create new directories `0700` and Audit artifacts
  `0600`.
- Do not chmod a pre-existing immediate parent: a custom audit path may live in
  a caller-owned working directory, `/var/log`, `/tmp`, or another shared
  container. Validate that the immediate parent is a real directory and reject
  a symlink at that component, but do not reclassify it as Aegis-owned.
- Do not tighten an already-existing default `~/.aegis` directory in H7b. It is
  shared Aegis state rather than an audit-only directory; a whole-state-directory
  permission contract is a separate finding if required.
- Reject symlink targets at the actual open/rotation seam. Prefer atomic
  no-follow/open flags where the standard library/platform extension supports
  them; document residual parent-component races honestly.
- Preserve file locking, append-only behavior, rotation ordering, and integrity
  chaining.
- Any hardening failure on the interception path remains fail closed.

## TDD seams

1. `AuditLogger::append` creates new parent components as `0700` and creates the
   log/lock as `0600`; it leaves a legitimate pre-existing parent mode unchanged.
2. Existing owned `0644` active/lock/archive files tighten to `0600`; simulated
   other-owner metadata, symlinks, and non-regular objects return
   `InsecureAuditArtifact` without touching their targets.
3. Query and integrity verification reject unsafe plain/gzip segments rather
   than returning a partial audit view.
4. Rotation preflight refuses an unsafe object in any managed slot before
   mutating an archive; the archive set remains byte-for-byte in its prior
   ordering.
5. Gzip failure removes staging best-effort, leaves the active log intact, and
   exposes no final partial archive; a safe stale staging file is recoverable.
6. Existing legitimate audit logs remain appendable after mode tightening, and
   a platform-neutral append/rotation round trip covers the non-Unix branch.

## Design decisions (locked)

1. **Audit directory boundary.** Only parent components Aegis creates while
   materializing the configured `Audit log` path are `Audit directory` objects
   and receive `0700`. A pre-existing immediate parent remains caller-owned and
   is validated but never chmodded by H7b. The owner-only contract applies to
   the active log, lock, and newly created compressed archives themselves.
2. **Existing audit artifacts — tighten-if-owned, otherwise reject.** On Unix,
   an existing regular active log, lock, or rotated segment owned by the current
   effective uid is tightened to exactly `0600` through the same no-follow file
   descriptor used for validation, then its metadata is rechecked. A symlink,
   non-regular file, different owner (including when Aegis runs as root), or a
   failed mode change/recheck is rejected fail closed. Do not use a separate
   path-based `chmod`, which would reopen a check/use substitution window.
3. **Whole-rotation preflight before mutation.** While holding the exclusive
   audit lock, validate and, where permitted, tighten the active log, retention
   destination, every numbered/plain-or-gzip segment the rotation can touch,
   and the new compressed destination before the first remove, rename, or
   create. Any unsafe object aborts the rotation without changing the archive
   set. Preserve the existing rotation order after preflight.
4. **Compressed rotation stages before commit.** Write gzip output to a new
   owner-only staging file opened with no-follow plus `create_new`. Remove the
   active log only after compression completes and the staging file has been
   successfully committed to its final archive name. A compression or commit
   failure must leave the active log intact. Path substitution between preflight
   and later path-based rename remains a documented parent/directory race rather
   than a claimed sandbox-grade guarantee.
5. **Non-Unix compatibility fallback.** The owner, numeric-mode, and atomic
   no-follow guarantees are `#[cfg(unix)]` and cover the supported production
   targets (Linux, macOS, and WSL2). `#[cfg(not(unix))]` retains ordinary
   filesystem opens so the library compiles and append/rotation behavior remains
   testable, but makes no `0700`/`0600`, owner, ACL, or reparse-point guarantee.
   Native Windows hardening remains outside the 1.0 product scope; do not turn
   the fallback into an unconditional deny.
6. **Typed policy refusal.** Add
   `AuditError::InsecureAuditArtifact { path: String, detail: String }` for an
   unsafe immediate parent, symlink, non-regular object, owner mismatch, failed
   mode hardening/recheck, or unsafe rotation slot. Keep ordinary open, write,
   compression, and rename failures under `AuditError::Io`. Negative tests match
   the typed variant and path rather than brittle display strings.
7. **Target-level no-follow, not parent confinement.** Unix opens reject a
   symlink in the `Audit artifact` itself atomically, and the immediate parent
   must be a real directory when checked. H7b still permits a pre-existing
   group/world-writable or differently owned parent, so an actor who can rename
   its entries retains races between separate open/rename/remove calls and can
   split processes across different lock inodes. Document this limit and
   recommend a dedicated owner-only directory for custom audit paths. A full
   component-by-component dirfd walk and `*at`-based rotation is separate
   architectural hardening, not H7b closure scope.
8. **Managed deterministic gzip staging.** Compressed rotation uses the adjacent
   managed slot `audit.jsonl.1.gz.tmp`, created owner-only with no-follow plus
   `create_new`. An owned regular stale staging file from an interrupted
   rotation is validated/tightened during preflight, then removed as the first
   controlled path mutation only after the entire preflight succeeds; an unsafe
   staging object aborts before archive mutation. Ordinary compression failure
   removes staging best-effort and preserves the active log. Commit the
   completed staging file to the final archive before removing the active log.
   H7b adds no `fsync` or power-loss durability guarantee.
9. **One secure-open policy for every audit operation.** Append, rotation,
   queries, integrity verification, and tail/hash discovery open the active log,
   lock, plain segments, and gzip segments through the same Unix validation and
   hardening seam. An unsafe segment fails the whole operation rather than being
   skipped and weakening the visible chain. Owned broad artifacts may therefore
   be tightened by a read operation. If the audit parent does not exist, a pure
   query may still return an empty result without creating the directory tree.
10. **Audit-local filesystem seam.** Add
    `crates/aegis-audit/src/secure_fs.rs`; do not create a shared crate or make
    `aegis-audit` depend on `aegis-snapshot`. Reuse the H7a idiom, not its API,
    because Audit artifacts are repeatedly reopened and tightened rather than only
    reserved with `create_new`. Add a direct Unix `libc` dependency for
    `O_NOFOLLOW | O_CLOEXEC` and `geteuid`; bind metadata and mode changes to the
    opened `File`. Keep intent-specific open/parent helpers there, while archive
    naming, retention, and ordering remain in `rotation.rs`.
11. **Unprivileged test strategy.** Use real Unix filesystem tests for modes,
    tightening, symlink/non-regular rejection, unchanged pre-existing parent
    mode, target preservation, and mutation-free rotation preflight. Exercise an
    owner mismatch through a pure internal metadata-policy function with an
    explicit expected uid rather than requiring `chown` or root. Narrow
    `#[cfg(test)]` seams may inject mode-hardening and gzip failures without
    widening the production API. Tests match `InsecureAuditArtifact` and its
    path. Replace the existing source-text assertions that pin
    `create_dir_all` and its old race comment with observable behavior tests.
12. **Architectural record.** Add the next available ADR (expected ADR-020
    after H7a's ADR-019) for the Unix owner-only contract, target-level
    no-follow, tighten-or-reject migration, rotation preflight/staging, and the
    deliberate parent-race/non-Unix/crash-durability limits. Index it in
    `docs/adr/README.md`; do not reserve the number until H7a has landed.

## Implementation sequence

1. **Refresh after H7a.** Confirm its landed dependency/error/test idioms, assign
   the next ADR number, and keep H7b in its own slice. Do not copy any H7a review
   defect into the audit helper.
2. **Red — append seam.** Replace the source-text `create_dir_all` assertions
   with behavior tests for new `0700` directories, `0600` active/lock files,
   unchanged existing parent mode, owned-file tightening, and active/lock
   symlink rejection.
3. **Green — secure filesystem core.** Add `InsecureAuditArtifact`, the
   audit-local `secure_fs` module, direct Unix `libc`, descriptor-bound
   validation/hardening, and the documented non-Unix fallback. Preserve append
   mode and the current lock acquisition contract.
4. **Red/green — reads and integrity.** Route active/plain/gzip reads, tail hash
   discovery, queries, and integrity verification through secure opens. Prove an
   unsafe segment fails the whole operation and an absent parent remains an
   empty read without filesystem creation.
5. **Red/green — rotation preflight.** Validate every managed source,
   destination, retention slot, and staging slot under the exclusive lock before
   mutation. Preserve the existing retention and oldest-to-newest query order.
6. **Red/green — staged gzip commit.** Create `audit.jsonl.1.gz.tmp` securely,
   inject compression failure, recover a safe stale stage, reject an unsafe
   stage, commit the final archive, then remove the active log.
7. **Docs and record.** Add/index the next ADR, update `docs/threat-model.md`
   with the Unix contract and explicit parent/non-Unix/durability limits, and
   synchronize `CHANGELOG.md`, `PROJECT_STATE.md`, `CONTEXT.md`, and `TASKS.md`
   only after implementation and all required gates pass.

## Verification

- Focused `aegis-audit` append/rotation/integrity tests
- Unix mode/symlink/rotation-preflight regressions
- Platform-neutral append/rotation compatibility test
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`

No scanner benchmark is required because H7b does not touch the synchronous
scanner hot path.
