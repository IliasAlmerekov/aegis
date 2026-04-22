# ADR-006 — CI detection has an explicit override contract

## Status

Accepted

## Decision

CI detection is centralized in `src/runtime_gate.rs`.

## Current precedence

1. `AEGIS_CI` explicit override
   - truthy (`1`, `true`, `yes`) forces CI behavior
   - falsy (`0`, `false`, `no`) forces non-CI behavior
2. well-known CI environment variables (`CI`, `GITHUB_ACTIONS`, `GITLAB_CI`,
   etc.)
3. non-empty `JENKINS_URL`

## Why

- CI semantics must be consistent across shell-wrapper mode, watch mode,
  status, and hook integrations

## Implication

- docs should say that detected CI environments enforce by default, while
  `AEGIS_CI` can explicitly override CI detection
