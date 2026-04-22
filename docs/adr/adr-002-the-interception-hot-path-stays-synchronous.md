# ADR-002 — The interception hot path stays synchronous

## Status

Accepted

## Decision

Command parsing and classification are performance-sensitive and remain
synchronous by design.

## Why

- safe-command latency matters
- parser / scanner behavior is easier to reason about without async scheduling
- the project contract explicitly treats this path as benchmark-sensitive

## Implication

- async is acceptable for subprocess / snapshot work
- async is not introduced into `src/interceptor/`
