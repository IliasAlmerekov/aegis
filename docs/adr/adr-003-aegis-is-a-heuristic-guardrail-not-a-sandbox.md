# ADR-003 — Aegis is a heuristic guardrail, not a sandbox

## Status

Accepted

## Decision

Aegis intentionally operates on raw command text and policy decisions before
the real shell runs.

## Why

- it is meant to reduce accidental damage, not provide OS isolation
- approved commands still run with the operator's normal privileges

## Implication

- docs must not describe Aegis as a hard security boundary
- limitations around encoded input, deferred execution, and shell/runtime
  expansion remain explicit non-goals
