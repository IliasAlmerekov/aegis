# ADR-018 — Filesystem snapshot artifacts stay contained in their snapshot store

## Status

Accepted

## Context

Filesystem snapshot identifiers record artifact paths. Lexical validation alone
cannot prove that a decoded path remains in the plugin-owned store: an absolute
outside path, sibling-prefix path, or symlink can redirect rollback reads or
artifact deletion. SQLite also recorded the live restore destination in the
identifier, allowing a forged identifier to select a write destination.

## Decision

For SQLite, PostgreSQL, and MySQL, the plugin's own configured **Snapshot
store** is the trusted root. Before rollback or deletion, the artifact path is
checked by canonicalizing the store, validating the absolute candidate
lexically, canonicalizing its parent, and—when it exists—canonicalizing the
artifact. Each resolved path must remain beneath the canonical store; otherwise
the operation fails with `SnapshotError::PathEscapesSnapshotStore`.

SQLite restores only to its configured database path. The original path encoded
in a snapshot identifier is not a write destination. Supabase keeps its
existing bundle-root containment implementation; migrating it to the shared
helper is outside this decision.

## Consequences

- Forged, traversal, symlink-escape, and sibling-prefix artifact paths fail
  closed before rollback or deletion can act outside the snapshot store.
- Missing artifacts remain an idempotent deletion no-op only when their parent
  is proven inside an existing snapshot store.
- Legacy identifiers whose artifacts are in the current store still restore.
  PostgreSQL and MySQL use the current configured connection target because
  their legacy identifiers did not record one; SQLite always targets the
  configured live database path.
- Snapshots remain best-effort; this decision constrains where filesystem
  artifact operations may act and does not turn them into a general backup.
