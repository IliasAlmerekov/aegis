# ADR-004 — Snapshots are best-effort; audit is append-only

## Status

Accepted

## Decision

Recovery and forensics are important, but they are not symmetric guarantees.

## Current contract

- snapshots are provider-based and best-effort
- rollback may still fail or conflict
- audit output remains append-only JSONL
- the optional audit integrity chain detects corruption and inconsistent edits;
  it has no keyed or external anchor

## Implication

- docs must describe rollback honestly
- audit integrity claims must stay tied to the configured integrity mode
