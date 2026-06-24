# Changelog

All notable changes to Aegis are documented here.  
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) · Versioning: [SemVer](https://semver.org/).

**Agent instructions:** prepend a new entry under `[Unreleased]` after every feature,
fix, or breaking change. Use categories: `Added`, `Changed`, `Fixed`, `Removed`, `Security`.
Reference the ADR number when an architectural decision was made (e.g. `(ADR-011)`).

---

## [Unreleased]

### Security

- Claude Code interception no longer depends on `aegis` being on the hook-exec PATH: `aegis install-hooks --claude-code` (and `--all`) now materializes an absolute, jq-free shim at `~/.claude/hooks/aegis-pre-tool-use.sh` and registers its absolute path in `settings.json`, at parity with the Codex hook (ADR-012).

### Fixed

- `aegis install-hooks --claude-code` now migrates away every aegis-managed legacy Bash registration — the bare `aegis hook` command and the legacy `aegis-rewrite.sh` file — to the absolute shim while preserving unrelated user hooks (including commands that merely mention `aegis`); reinstall is idempotent (ADR-012).
- The shared `aegis hook` deny response now emits a top-level `reason` mirroring `hookSpecificOutput.permissionDecisionReason`, so the deny message is visible in both Claude Code (top-level `reason`) and Codex (`permissionDecisionReason`) (ADR-012).
- `scripts/uninstall.sh` now removes the absolute Claude hook shim and prunes its `PreToolUse` `Bash` registration, alongside the existing legacy `aegis hook` / `aegis-rewrite.sh` cleanup (ADR-012).
- `scripts/uninstall.sh` normalizes a trailing slash on `$HOME` before building prune paths so they match the absolute path the Rust installer registers via `std::path::absolute` / `Path::join` (which never emits a doubled separator); root `/` is preserved (ADR-012).
- `scripts/hooks/claude-code.sh` now ends with a trailing newline (POSIX text-file convention), and the ADR-012 "byte-identical except header" wording was corrected to "behaviorally identical; only agent-specific comments differ" since the two shims cross-reference each other by name (ADR-012).
- Closed the C2 `$IFS` command-obfuscation bypass by normalizing unquoted literal `$IFS` / `${IFS}` as shell separators during tokenization, so destructive forms such as `rm$IFS-rf$IFS/`, `rm${IFS}-rf${IFS}/`, and `dd${IFS}of=/dev/sda` classify correctly across direct, nested-shell, heredoc, and process-substitution paths; quoted, escaped, and non-IFS variable forms stay opaque.
- Restored fail-closed hook test coverage for non-object `tool_input` payloads and centralized production POSIX shell quoting for setup-shell/Codex hook generation (ADR-011).
- `aegis setup-shell` now accepts scoped npm install paths (e.g. `@iliasalmerekov/aegis`); paths are POSIX single-quote escaped in the managed rc block instead of rejected, and errors name whether the real shell path or the Aegis binary path was invalid (ADR-011).
- Codex `SessionStart` hook now emits guidance under `additionalContext` instead of the invalid `context` field, fixing `hook returned invalid session start JSON output` (ADR-011).

### Changed

- The Claude Code `PreToolUse` hook is now a jq-free shim that `exec`s the Rust `aegis hook` (byte-identical to the Codex shim except for its header), replacing the legacy jq-based `aegis-rewrite.sh` script; `install::mod` now shares `write_executable`, `resolved_aegis_bin`, and `combine_outcomes` between the Claude and Codex installers instead of duplicating them (ADR-012).
- Codex `PreToolUse` hook now transparently rewrites unwrapped Bash commands through `aegis --command` (`permissionDecision: "allow"` + `updatedInput`) by delegating to the Rust `aegis hook`, instead of denying and relying on the model to retry. This removes the `jq`/`python3` runtime dependency from the Codex hook (ADR-011).
- The Rust `aegis hook` rewrite now fails closed on commands that begin with the bare `aegis` word but are not a canonical `aegis --command '<...>'` wrapper, and passes canonical wrappers through untouched (ADR-011).
- Installed Codex pre-tool-use hook embeds a shell-quoted absolute Aegis binary path so it works under a minimal hook-exec PATH; an explicit `AEGIS_BIN` still overrides it (ADR-011).

### Added

- npm postinstall best-effort agent hook setup: runs `aegis install-hooks --all` when `~/.claude` or `~/.codex` already exists, prints next steps otherwise, never creates agent directories, and never fails the npm install (opt out with `AEGIS_NPM_SKIP_HOOKS=1`) (ADR-011).

### Changed

- Simplified `README.md` to a minimal public contract (What / Why / Install / How it works) with a visible threat-model link and an honest heuristic-not-a-sandbox statement (M6 docs gate).
- Aligned landing page copy with the current install flow while keeping the existing design (3D shield and section layout unchanged): installer/Homebrew/npm/Cargo, `aegis setup-shell` opt-in, `v0.5.8`, and honest audit wording (append-only; tamper-evident when hash-chain integrity is enabled) replacing the prior overclaim (M6).
- Prepare release metadata for v0.5.8 after the v0.5.7 release build hit the stale `ldd` static-link verification path (M3.2).

### Removed

- Non-production landing source artifacts not used by the runtime: `landing/pencil.pen`, `landing/DESIGN.md`, `landing/tokens.json`, and unused image assets (`landing/images/Hitem3d-1781772057946.glb`, `landing/images/generated-1781681175337.png`) (M6).
- `test_q` stray compiled ELF binary from the repo root (M6).

### Added

- `aegis setup-shell` — explicit opt-in command for shell hook installation (ADR-009)
- Supply-chain gates: `cargo audit` + `cargo deny check` both green in CI (M5.4)
- npm wrapper package with native binary download per platform (M3.4)
- npm checksum updater script for release automation
- GitHub Releases CI: static musl targets for Linux (M3.2)
- Homebrew formula/tap with formula updater (M3.3)
- GitHub Releases with `.sha256` sidecars (M3.5)
- Fuzz corpus CI job at ≥ 100 000 iterations per target (M5.2)
- Snapshot/rollback integration tests in CI (M5.3)

### Fixed

- Fixed C1 uppercase scanner bypass by compiling built-in regex patterns case-insensitively while preserving custom regex case sensitivity.
- Render the README hero GIF through standard Markdown image syntax so GitHub treats it like other animated demos (M6 docs gate).
- Ignore `/test_q` at the repo root so the stray compiled ELF cannot be re-committed (M6).
- Release CI: verify static Linux binaries via `readelf` (ELF headers) instead of `ldd`; fixes false failures on musl `static-pie` (x86_64) and cross-compiled `aarch64` binaries (M3.2)
- `setup-shell`: block symlink recursion and rc injection
- Gate starlark-policy dependency — closed supply-chain lint warnings
- Follow GitHub release redirects in npm installer
- Keep npm package contents minimal

### Security

- `setup-shell` rejects symlink loops and prevents injection into shell rc files

---

## v0.5.6

### Highlights

- **Sandbox bypass is an audit event** (ROADMAP 6.4): every executed command
  now records a `sandbox_status` field in the audit log — `active` (a sandbox
  profile was applied), `unavailable` (a configured sandbox could not be applied
  and the command ran unconfined — a bypass), or `not_configured`. When a bypass
  occurs, Aegis also emits a `WARN` on the `aegis::sandbox` target. Setting
  `sandbox.required = true` still turns unavailability into a hard block.

### Documentation and contracts

- Audit log entries gain the canonical `sandbox_status` field. The legacy
  `sandbox_active` boolean is still written (mirrored from the status) and read,
  so existing log readers and older logs remain compatible.

---

## v0.5.3

### Highlights

- **Binary-first hook installation**: the documented release-install flow now
  describes Claude Code / Codex hook setup as running through the installed
  `aegis` binary when supported agent directories are already present, instead
  of depending on a local repository checkout.
- **Honest skip behavior**: current docs now state that automatic hook setup
  only updates agent directories that already exist and skips missing
  `~/.claude` / `~/.codex` directories without seeding them.
- **Single follow-up command**: README, troubleshooting, and release docs now
  point users at `aegis install-hooks --all` as the supported explicit rerun
  command after agent directories appear later.

### Documentation and contracts

- `README.md` now documents automatic hook setup as a binary-first flow and
  replaces local-checkout-only follow-up guidance with `aegis install-hooks --all`.
- `docs/troubleshooting.md` now explains the skip reason in terms of missing
  agent directories and tells users to rerun `aegis install-hooks --all`.
- `docs/releases/current-line.md` and `docs/releases/v1.0.0.md` now describe
  the binary-first auto-attempt path, skip semantics, and explicit follow-up
  command honestly.

---

## v0.5.1

### Highlights

- **Keyword scanner regression test hardening**: the `keywords.rs` source-slice
  helper used by the hot-path regression test now stops at the actual `mod
tests` boundary instead of relying on a naive split. This keeps literal
  `chars.next().unwrap()` strings inside test-only helpers from causing false
  positives against production-code assertions.
- **Release metadata bumped to 0.5.1**: `Cargo.toml` and `Cargo.lock` now track
  the `0.5.1` crate version for the current release line.
- **Tracker cleanup**: repository-local `REVIEW.md` and `TODO.md` were removed
  from the release tree so the tagged state reflects the current curated docs
  set more closely.

### Documentation and contracts

- `CHANGELOG.md` now tracks the `v0.5.1` release line.
- `docs/releases/current-line.md` now tracks the `0.5.1` release line.
- `docs/releases/v1.0.0.md` now references `0.5.1` as the current pre-1.0
  crate version when describing the future `v1.0.0` target.
- The release documentation continues to describe Aegis as a heuristic shell
  guardrail rather than a sandbox or hard security boundary.

---

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
