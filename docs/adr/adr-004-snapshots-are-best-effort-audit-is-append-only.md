# ADR-004 — Snapshots are best-effort; audit is append-only

## Status

Accepted

## Decision

Recovery and forensics are important, but they are not symmetric guarantees.

## Current contract

- snapshots are provider-based and best-effort
- rollback may still fail or conflict
- audit output remains append-only JSONL
- stronger tamper-evidence is available only when audit integrity mode is
  enabled

## Implication

- docs must describe rollback honestly
- audit integrity claims must stay tied to the configured integrity mode
