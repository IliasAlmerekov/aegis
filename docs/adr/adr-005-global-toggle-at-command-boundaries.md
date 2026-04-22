# ADR-005 — Global toggle at command boundaries

## Status

Accepted

## Decision

The current full-disable model uses a single global flag file:

- `~/.aegis/disabled`

## Rust-side behavior

- `src/toggle.rs` owns disabled-flag path resolution and status helpers
- `aegis on` removes the flag
- `aegis off` creates or refreshes the flag
- toggle commands audit best-effort and do not roll back the state change when
  audit writing fails

## Command-boundary semantics

- shell-wrapper mode snapshots toggle state once before enforcement-related I/O
- watch mode snapshots toggle state once per command-boundary gate
- disabled local mode is intentionally quiet for ordinary shell / supported
  hook usage

## Why

- a single metadata existence check is cheap and predictable
- command-boundary semantics are easier to reason about than mid-command
  mutation

## Implication

- toggle detection must not rely on TOML parsing or heavy config reloads
- malformed parent paths should not silently imply disabled mode
