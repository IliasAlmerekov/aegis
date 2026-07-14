# H7b — Audit file and directory hardening

## Status

Draft — requires a finding-specific `grill-with-docs` session after H7a
establishes the secure-create idiom and before TDD.

## Finding

Audit directory, active log, rotated segments, and lock-file opens rely on default
filesystem modes and ordinary path-following. Because audit is a security
artifact, a broad mode or symlink target can expose or redirect append operations.

## Scope

- Cover the audit directory, active JSONL, rotated segments, and `.lock` file.
- On supported Unix targets, create directories `0700` and files `0600`.
- Reject symlink targets at the actual open/rotation seam. Prefer atomic
  no-follow/open flags where the standard library/platform extension supports
  them; document residual parent-component races honestly.
- Preserve file locking, append-only behavior, rotation ordering, and integrity
  chaining.
- Any hardening failure on the interception path remains fail closed.

## TDD seams

1. `AuditLogger::append` creates owner-only directory/log/lock artifacts.
2. Append to a symlinked active log fails and leaves the target untouched.
3. Rotation refuses a symlinked destination and never loses the active log.
4. Existing legitimate audit logs remain appendable after safe mode tightening.

## Implementation sequence

1. Add the active-log symlink failure regression.
2. Introduce the minimal secure audit open helper under platform gates.
3. Route lock and rotation opens through the same policy.
4. Add metadata-mode and backward-compatibility tests.

## Verification

- Focused `aegis-audit` append/rotation/integrity tests
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
