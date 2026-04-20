# Changelog

This changelog records the release-documentation state for Aegis. It is intended
to stay aligned with the repository's current docs, release workflow, and
installer behavior.

## v0.5.0

### Highlights

- **Managed agent-hook install flow**: the current release line now documents
  and ships the local-checkout-only installation path for Claude Code and Codex
  hook payloads, including their shared toggle helper behavior.
- **Global toggle + CI posture clarified**: the `aegis on`, `aegis off`, and
  `aegis status` flow remains part of the public current line, with docs and
  tests aligned around the default-on CI enforcement contract and explicit
  `AEGIS_CI` override semantics.
- **Installer and hook hardening**: deprecated installer controls stay rejected,
  uninstall cleanup removes installed hook payloads and registrations together,
  and hook fallback paths remain best-effort without silently weakening the main
  guardrail contract.
- **Release and architecture docs refreshed**: architecture, install,
  troubleshooting, and release-readiness docs were updated to describe the
  global-first installer and current release workflow honestly.

### Documentation and contracts

- `docs/releases/current-line.md` now tracks the `0.5.0` release line.
- `docs/releases/v1.0.0.md` now references `0.5.0` as the current pre-1.0
  crate version when describing the future `v1.0.0` target.
- The release documentation continues to describe Aegis as a heuristic shell
  guardrail rather than a sandbox or hard security boundary.

---

## v0.4.0

### Highlights

- **Global-first installer flow**: the convenience installer no longer prompts
  for Global / Local / Binary setup modes. It validates shell support up front,
  performs the managed global shell setup path, and prints explicit follow-up
  guidance.
- **Dynamic on/off toggle**: Aegis now exposes `aegis on`, `aegis off`, and
  `aegis status` backed by the global `~/.aegis/disabled` flag.
- **Zero-noise disabled mode**: outside CI, disabled shell-wrapper and
  supported hook usage behave as though Aegis were absent for ordinary command
  flow while still preserving the explicit toggle history.
- **CI override contract**: detected CI environments keep enforcement active by
  default, while `AEGIS_CI` can explicitly override CI detection in either
  direction.
- **Shared hook toggle helper**: Claude Code and Codex hook installations now
  share the managed helper path `~/.aegis/lib/toggle-state.sh`, with fail-safe
  fallback behavior if that helper is missing.
- **Honest install / uninstall behavior**: local hook setup is auto-attempted
  only from a real local checkout, and uninstall now removes both installed
  hook payloads and their JSON registrations.

### Documentation and contracts

- `README.md` now documents the global-first installer, the removed
  `AEGIS_SETUP_MODE` / `AEGIS_SKIP_SHELL_SETUP` controls, and the verified
  disabled / CI-override behavior.
- Added `docs/architecture-decisions.md` to capture the current architecture,
  documented non-goals, toggle / CI decisions, and fuzzing guidance referenced
  by contributor and security docs.
- Troubleshooting and release docs now describe the current installer and hook
  setup behavior instead of the removed interactive setup-mode flow.

---

## v0.3.0

### Highlights

- **Interactive installer with three setup modes**: the install script now asks
  the user to choose Global, Local (project-only), or Binary-only setup.
- **Local mode**: creates `.aegis/enter.sh` in the project directory and
  immediately launches a protected shell — no manual activation needed.
- **ASCII banner**: the installer displays an Aegis banner on startup.
- **Simplified README**: rewritten for clarity with a "Why Aegis exists" section
  explaining the motivation (vibe coders, full-permission agents, accidental
  data loss). Installation is now a single command with an interactive prompt.
- **`AEGIS_SETUP_MODE` env var**: allows CI and scripts to select the setup mode
  non-interactively (`global`, `local`, or `binary`).
- **Supabase snapshot provider**: Aegis can now snapshot and rollback Supabase
  databases before dangerous commands, alongside existing Git, Docker, MySQL,
  and PostgreSQL providers.

### Internal

- Updated contract tests to match the new README structure.
- Fixed duplicate `#[test]` attribute in installer tests.
- Added three new installer integration tests covering all setup modes.

---

## v0.2.0

Release documentation for the current pre-1.0 line tracked by `Cargo.toml`
version `0.2.0`.

### Highlights documented for this release line

- The release workflow is configured to produce GitHub Release artifacts for four targets:
  - `x86_64-unknown-linux-gnu`
  - `aarch64-unknown-linux-gnu`
  - `x86_64-apple-darwin`
  - `aarch64-apple-darwin`
- Each binary is produced with a matching `.sha256` sidecar.
- The install path is configured to verify the downloaded checksum before writing to `BINDIR`.
- The current docs state the supported platform matrix and the known
  limitations of the heuristic guardrail model.
- Troubleshooting and recovery guidance exists for install, checksum, and
  rollback failures.

### What is not claimed

- No SBOM is published by the current release workflow.
- No provenance metadata or attestations are generated or attached by the
  current release workflow.
- This release documentation does not claim byte-for-byte reproducible builds
  across all environments.

### Reference docs

- [Current release line](docs/releases/current-line.md)
- [Planned v1.0.0 release summary](docs/releases/v1.0.0.md)
- [Release and CI guarantees](docs/ci.md)
- [Platform support](docs/platform-support.md)
- [Threat model](docs/threat-model.md)
- [Troubleshooting and recovery](docs/troubleshooting.md)
