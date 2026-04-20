# Aegis current release line (v0.5.0)

This document summarizes the live pre-1.0 release line described by
`Cargo.toml` version `0.5.0`.

It is the current public / MVP posture, not the future `v1.0.0` target.

## Summary

Aegis on the current line is a heuristic shell-proxy guardrail that:

- intercepts shell commands before execution;
- classifies commands into `Safe`, `Warn`, `Danger`, or `Block`;
- requires confirmation for risky commands;
- hard-blocks `Block` commands;
- supports a global on/off toggle via `aegis on`, `aegis off`, and `aegis status`;
- keeps detected CI environments enforcing by default, with explicit `AEGIS_CI` override semantics;
- writes audit data and supports best-effort snapshots for dangerous actions
  when configured.

This current-line summary does **not** claim sandboxing, a hard security
boundary, or release properties not already documented elsewhere.

## Current release-line notes

- Current crate version: `0.5.0`
- The convenience installer is global-first and rejects the removed
  `AEGIS_SETUP_MODE` / `AEGIS_SKIP_SHELL_SETUP` controls.
- Claude Code / Codex hook installation is attempted automatically only from a
  real local checkout with supported agent directories present.
- Supported platforms are documented in [Platform support](../platform-support.md).
- Release and CI behavior are documented in [CI and Release Guarantees](../ci.md).
- Launch checklists and the verification-first/manual install path are
  documented in [Release readiness](../release-readiness.md).
- Threat and limitation language is documented in [Threat model](../threat-model.md).

## What is not claimed

- no claim that a `v1.0.0` release has already been published;
- no claim that the release-readiness checklist has already been completed;
- no claim of SBOM, provenance, or attestation coverage unless the release
  workflow adds it;
- no claim of byte-for-byte reproducible builds across all environments.

## Forward-looking reference

The planned `v1.0.0` target is tracked separately in
[docs/releases/v1.0.0.md](v1.0.0.md).
