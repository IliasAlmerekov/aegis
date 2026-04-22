# ADR-010 — Full shell evaluation and deferred execution remain non-goals

## Status

Accepted

## Decision

This ADR is the important design limitation referenced by the threat model and
security policy.

Aegis does **not** aim to fully model:

- `eval "$(…)"`-style runtime assembly
- alias or shell-function expansion that changes behavior after raw command
  parsing
- a safe-looking write followed by a later dangerous invocation
- encoded / obfuscated payloads outside the implemented heuristic coverage
- shell semantics that only become visible after handing execution to the real
  shell

## Why

- those problems move Aegis toward becoming a full shell interpreter or
  sandbox, which is outside the project scope

## Implication

- reports based only on these known non-goals are documentation / roadmap
  inputs, not product vulnerabilities by themselves
