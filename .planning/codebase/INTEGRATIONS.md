# Aegis Integrations Scan

Last refreshed: 2026-04-12
Focus: tech+arch

## User-facing runtime surfaces

### 1. Shell-wrapper execution

- Main user path remains `aegis -c '<cmd>'`.
- `src/main.rs` resolves the real shell from:
  1. `AEGIS_REAL_SHELL`
  2. `SHELL` when it does not point back to Aegis
  3. `/bin/sh` fallback
- `scripts/install.sh` writes a managed block into `~/.bashrc` or `~/.zshrc`.

### 2. Watch mode

- `aegis watch` consumes NDJSON frames from stdin and emits NDJSON results to stdout.
- Implemented in `src/watch.rs`.
- Confirmation for watch-mode execution goes through `/dev/tty`, so this is intentionally Unix-oriented.

### 3. Evaluation-only JSON

- `aegis -c '<cmd>' --output json`
- Exposes machine-readable decision data without executing the command.
- Includes risk, decision, matched patterns, allowlist info, CI state, and snapshot plan.

### 4. Config integration

Effective config layers are wired as:

- built-in defaults
- `~/.config/aegis/config.toml`
- project `.aegis.toml`

Operational commands:

- `aegis config init`
- `aegis config show`
- `aegis config validate`

## External tool integrations

### Git snapshots

Implemented in `src/snapshot/git.rs`:

- applicability detection via Git repo checks
- pre-danger snapshot via stash
- rollback via stash restoration

The repo includes dedicated coverage for subdirs, worktrees, untracked files, and rollback conflict paths.

### Docker snapshots

Implemented in `src/snapshot/docker.rs`:

- live container discovery
- metadata capture
- snapshot via `docker commit`
- rollback via stop/remove/run reconstruction

Supported scope modes are configurable and documented in code.

### Local filesystem / audit

- Audit root: `~/.aegis/audit.jsonl`
- Rotation archives supported
- gzip archive support present
- tamper-evident hash chaining supported
- companion lock-file logic exists for multi-process safety

## CI / release integrations

### CI workflow

`.github/workflows/ci.yml` currently runs:

- fmt
- clippy
- tests
- `cargo audit`
- `cargo deny check bans licenses sources`
- release build job
- scanner benchmark job + policy check

### Release workflow

`.github/workflows/release.yml` currently:

- builds Linux + macOS assets
- publishes `.sha256` sidecars
- marks prereleases automatically for `-rc`, `-beta`, `-alpha`
- uploads artifacts to a GitHub Release

## Public-repo hygiene signals

- `LICENSE`, `CONTRIBUTING.md`, and `CODE_OF_CONDUCT.md` are present.
- README now includes:
  - security model
  - limitations
  - platform claims
- `docs/platform-support.md` exists and matches the Unix-only positioning.
- `fuzz/` now exists in the repo, consistent with the ADR update.

## Integration gaps that still matter for release confidence

### Installer verification gap

- Release workflow generates checksum files.
- `scripts/install.sh` downloads only the binary asset and does not fetch or verify the matching `.sha256`.
- The primary README install flow is still `curl ... | sh`.

### Threat-model documentation gap

- `docs/platform-support.md` exists.
- `README.md` has a limitations section.
- But `docs/threat-model.md` is still absent.

This matters because `CONVENTION.md` still treats threat model + limitations documentation as part of stronger production claims.

### CI/doc drift

- `docs/ci.md` says pinned `cargo-deny` is `0.19.1`.
- `.github/workflows/ci.yml` currently sets `CARGO_DENY_VERSION: 0.18.2`.

### Crates.io release caveat

- `rtk cargo publish --dry-run --allow-dirty` succeeded.
- The dry-run also warned that `aegis@0.1.0` already exists on the crates.io index.

That is not a blocker for a GitHub release, but it is a real caveat if the intended “first release” includes crates.io publication under the current name/version.

## Bottom line

Integration coverage is now strong enough for a real public `0.1.0` GitHub release and day-to-day usage on Linux/macOS.
The remaining weakness is no longer missing plumbing; it is mostly missing verification/positioning polish around installer trust and threat-model documentation.
