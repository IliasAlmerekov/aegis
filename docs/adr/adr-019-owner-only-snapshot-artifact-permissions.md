# ADR-019 — Snapshot artifacts use owner-only permissions on Unix

## Status

Accepted

## Context

Snapshot stores, database dumps, Supabase bundles, and manifests contain
operator data and can contain credentials. Ordinary directory/file creation
uses the process umask; SQLite copying also inherits the live database's mode.
Those defaults can expose snapshot contents to another local user before a
rollback or deletion ever occurs.

## Decision

On Unix, Aegis creates every new Snapshot store and bundle directory with mode
`0700` and every new Snapshot artifact with mode `0600`. The requested modes
are supplied atomically at creation and then reasserted from metadata. SQLite
streams into a fresh secured artifact instead of using `fs::copy`; PostgreSQL,
MySQL, and Supabase reserve a secured artifact before invoking an external dump
tool, then reassert its mode afterward.

Before a sensitive write, an existing Snapshot store leaf is rejected when it
is a symlink, not a directory, or owned by another uid. An owner-owned store
with broader permissions is tightened to `0700`. Permission hardening failures
return `SnapshotError::InsecureSnapshotPermissions`; fresh artifact failures
remove the reserved file. Existing parents and live restore targets remain
caller-owned and are not modified. Rust exposes stored uid metadata but not the
effective uid, so the snapshot crate uses the direct `libc::geteuid` binding;
it is already in the dependency graph and does not require a native build step.

On non-Unix platforms the helpers retain ordinary fresh-create behavior but do
not claim a POSIX mode contract; ACL handling and native-Windows support remain
out of scope.

## Consequences

- Snapshot artifacts on supported Unix targets are no longer readable or
  listable by unrelated local users through their direct store paths.
- An unsafe store fails closed before Aegis writes a new sensitive artifact,
  except an owner-owned broad leaf that can safely be narrowed.
- Snapshot creation remains best-effort and this decision does not make
  Aegis a filesystem sandbox or a general backup system.
