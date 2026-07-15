# ADR-020 — Audit artifacts are owner-only and use no-follow opens on Unix

## Status

Accepted

## Context

The active Audit log, its companion lock, and rotated segments contain a record
of security decisions. Ordinary `OpenOptions`, `File::open`, `File::create`, and
recursive directory creation inherit the process umask and follow a symlink in
the final path component. Rotation also mutates several archive names, so
discovering an unsafe slot only after earlier renames or removals can damage the
existing archive order before the operation fails.

Audit paths may be configured beneath a caller-owned directory such as a
working tree or `/var/log`. Aegis cannot treat every pre-existing parent as its
own state directory or safely change that parent's permissions.

## Decision

On Unix, directories Aegis creates while materializing an Audit log path use
mode `0700`. A pre-existing immediate parent must be a real directory but keeps
its existing owner and mode; H7b also leaves a pre-existing shared `~/.aegis`
directory unchanged. New active logs, lock files, rotated segments, and gzip
staging artifacts use mode `0600`.

Every audit-artifact open uses `O_NOFOLLOW | O_CLOEXEC`. The opened descriptor's
metadata must describe a regular file owned by the effective uid. An owned file
with broader permissions is tightened to exactly `0600` through that descriptor
and rechecked. A symlink, non-regular object, other-owner file, or failed
hardening/recheck returns `AuditError::InsecureAuditArtifact`. Append, shared
reads, queries, integrity verification, tail-hash discovery, and rotation use
the same policy, so an unsafe segment fails the whole operation instead of
being skipped. Rust does not expose the effective uid directly, so
`aegis-audit` uses the direct `libc::geteuid` binding.

Rotation validates the active log, every plain and gzip archive slot it can
read, remove, or rename, the retention destination, and the deterministic gzip
staging slot before its first archive mutation. Compressed rotation writes to a
fresh adjacent `audit.jsonl.1.gz.tmp` artifact and commits it by rename before
removing the active log. A safe stale staging artifact is tightened and removed
after preflight; an unsafe one aborts preflight. Compression or commit failure
removes staging best-effort and preserves the active log.

On non-Unix platforms the audit crate retains ordinary compatible opens and
rotation behavior. It makes no POSIX mode, owner, ACL, no-follow, or Windows
reparse-point guarantee there; native Windows hardening remains outside the 1.0
scope.

## Consequences

- Supported Unix audit artifacts are not created with group/world access, and
  existing owner-owned broad artifacts migrate to the owner-only mode.
- Target symlinks and unsafe managed rotation slots fail closed without
  weakening append-only or whole-query behavior.
- Custom-path parents remain caller-owned. Separate path-based directory and
  rotation operations still have parent-entry substitution races, including
  the possibility of processes locking different inodes in a writable parent.
  Operators should place custom logs in a dedicated owner-only directory.
- Gzip staging avoids exposing a partial final archive, but this decision adds
  no `fsync` or power-loss durability guarantee and is not filesystem
  confinement.
