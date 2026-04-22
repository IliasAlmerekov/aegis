# ADR-001 — Keep the CLI entrypoint thin

## Status

Accepted

## Decision

`src/main.rs` is intentionally orchestration-only.

## Why

- it keeps the CLI surface readable
- it reduces the risk of mixing policy, parsing, and execution details together
- it makes security-sensitive behavior easier to review in focused modules

## Implication

- business logic belongs in focused modules such as `toggle`, `runtime_gate`,
  `watch`, `runtime`, `decision`, `interceptor`, and `snapshot`
