# ADR-008 — Installer is global-first and rejects removed mode controls

## Status

Accepted

## Decision

The convenience installer now has one supported path:

- global shell setup with managed RC-file integration

The old `AEGIS_SETUP_MODE` and `AEGIS_SKIP_SHELL_SETUP` controls are rejected
explicitly instead of being silently ignored.

## Current installer behavior

- validates shell / rc-file setup before downloading and installing the binary
- auto-attempts local `agent-setup.sh` only when a real local checkout is
  present
- reports hook setup honestly:
  - success
  - skip because no supported agent directories were detected
  - failure with next steps

## Implication

- “binary-only” workflows now use the manual / source-install guidance rather
  than a convenience-installer mode switch
