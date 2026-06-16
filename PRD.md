# PRD — Aegis 1.0

**Status:** approved specification for the 1.0 production release
**Document version:** 1.0
**Date:** 2026-06-15

---

## 1. Product Overview

Aegis is a lightweight Rust CLI that acts as a `$SHELL` proxy. It sits between an
AI agent and the real shell, intercepts every shell command, classifies it by
risk level, and requires human confirmation before destructive operations run.

**Problem.** AI agents (Claude Code, Codex, Cursor) run shell commands fast and
with full permissions. A single bad command deletes files, resets a repository,
drops a database, or pushes something dangerous. The developer has no control
point between the agent's intent and the irreversible action.

**Promise (one-liner).** Aegis is the last barrier between an AI agent and a
dangerous command: safe commands run instantly, risky ones require confirmation,
the worst are always blocked, and approved commands can run inside an OS sandbox.

---

## 2. Positioning

Aegis 1.0 is positioned as a **heuristic guardrail with an optional OS sandbox**.

- Base layer — a fast heuristic text filter for commands (token scanner).
- Reinforcing layer — an optional OS sandbox (bubblewrap + Landlock on Linux,
  Seatbelt on macOS) that restricts an approved command's capabilities at the OS
  level.

**Honest boundaries (must be reflected in the product and docs).** Aegis is
**not** a replacement for a sandbox as a hard security boundary and **not** a
guarantee against malicious code. The heuristic layer does not catch:

- obfuscated or encoded commands,
- `eval "$(...)"` and commands assembled at runtime,
- indirect execution (write a script first, run it later).

These limitations are documented in `docs/threat-model.md` and must be visible
in the README. The OS sandbox reduces blast radius, but its availability depends
on platform and environment (see §8).

---

## 3. Target Audience

**Primary (1.0 focus).** Individual developers running AI agents locally — from
"vibe coders" to experienced engineers who give the agent full shell access.
Priorities for this audience:

- one-command install that works out of the box (zero-config by default),
- no friction on the safe hot path (< 2 ms),
- clear prompts and learnability (repeated decisions are not re-asked).

**Secondary (post-1.0, considered architecturally but out of release scope).**
Teams and organizations: shared policies, centralized audit for compliance. The
architecture (`[[rules]]`, audit trail, config layering) must not foreclose this
path, but team features are not shipped in 1.0.

---

## 4. User Scenarios

1. **Intercept a dangerous command.** The agent runs `rm -rf ./src`. Aegis
   recognizes `Danger`, shows a TUI dialog with `justification`, and offers
   `[A]llow / [D]eny / [Always allow] / [Always deny]`.
2. **Block the worst.** The agent runs `rm -rf /`. Aegis always refuses, with no
   confirmation option.
3. **Learning via persistence.** The user picks "Always allow" — the decision is
   automatically written to config as a typed rule; the same command is no longer
   re-prompted.
4. **Snapshot and rollback.** Before a destructive command, Aegis takes a
   best-effort snapshot via a configured provider; if needed, the user rolls back
   with `aegis rollback '<snapshot-id>'`.
5. **Sandboxed execution.** An approved command runs inside an OS sandbox with a
   read-only view of the filesystem except for explicitly allowed write paths.
6. **Audit.** Every decision and every execution is written to an append-only
   JSONL log; the user reviews history and verifies the chain integrity
   (`aegis audit --verify-integrity`).
7. **Temporary disable.** `aegis off` / `aegis on` / `aegis status`; in a
   detected CI environment, policy stays enforced by default.

---

## 5. Functional Requirements 1.0

### 5.1 Interception and classification

- Intercept every shell command in the `$SHELL` proxy role and in `aegis -c` mode.
- Classify by `RiskLevel`: `Safe`, `Warn`, `Danger`, `Block` (the order is
  semantically ordered by severity and does not change).
- Token scanner as the single source of truth: tokenize → `ParsedCommand` →
  token-level matching via `MultiMap<program, PrefixRule>` (O(1) lookup of the
  relevant rules). The raw string is used only for display and audit.
- Support for `Alts` (semantic flag equivalents in one rule), `justification`,
  `match_examples` / `not_match_examples`.
- Built-in rules (≥70) are validated against their own examples in debug/tests.

### 5.2 Policy DSL

- **Typed TOML DSL** (`[[rules]]`): `pattern` with `Alts`, `decision`
  (`allow`/`prompt`/`block`), `justification`, `match_examples`,
  `not_match_examples`, a `when` clause (environment-conditional decision). Rules
  are validated at load time; an invalid rule is a startup error with a
  human-readable message and line numbers.
- **Starlark DSL** (`~/.aegis/policy.star`) as an opt-in power-user feature:
  `prefix_rule(...)` and `on_command(cmd)`. Starlark is evaluated at startup and
  compiled into the same `MultiMap<program, PrefixRule>`; Starlark never runs on
  the hot path.

### 5.3 Decision persistence

- "Always allow" appends an `[[allow]]` rule to the active config; "Always deny"
  appends a `[[block]]` rule.
- The command is tokenized into a prefix (program + meaningful flags; variable
  arguments are stripped).
- Deduplication on write: a duplicate is skipped silently; a conflict (same
  pattern, different decision) emits a warning with the existing rule's location.
- The scanner cache is invalidated; the new rule takes effect immediately.
- The legacy `allowlist` is migrated to `[[allow]]` with a deprecation warning.

### 5.4 Snapshot / rollback

- `SnapshotPlugin` trait (async, via `async-trait`) + 6 providers: Git, Docker,
  PostgreSQL, MySQL/MariaDB, SQLite, Supabase.
- Snapshot is best-effort before `Danger`-level commands. It is taken **only when
  the command is approved (`Allow`)** — never for `Block`ed commands — and runs
  **before** the (optionally sandboxed) execution, so a rollback target exists
  regardless of sandbox availability.
- `aegis rollback '<snapshot-id>'` restores state. `aegis snapshot list`
  enumerates available snapshots with their `snapshot_id`, provider, and creation
  time so the opaque `cwd+hash` id is discoverable.
- **Lifecycle:** snapshots are subject to a configurable retention policy
  (by count and/or age) under `[snapshot]`; `aegis snapshot prune` removes
  snapshots beyond the retention bound. Retention applies across providers
  (git stashes, Docker images, SQLite/PostgreSQL/MySQL dumps) to bound unlimited
  growth.
- No blocking I/O in async context (`tokio::time::sleep`, no `spawn_blocking`
  workarounds).

### 5.5 Sandbox

- **Linux:** bubblewrap (namespace sandbox, read-only filesystem except
  `allow_write`) + Landlock (LSM) for defense in depth.
- **macOS:** Seatbelt via `/usr/bin/sandbox-exec` with a `.sbpl` profile.
- **Windows:** not supported. On Windows, Aegis runs only inside WSL2, where it
  behaves as a Linux environment and uses the Linux sandbox.
- `[sandbox]` config: `enabled`, `allow_write`, `allow_network`, `required`.
- **Bypass is an audit event:** if the sandbox cannot be applied, the log records
  `sandbox_status = "unavailable"` and a `WARN` is emitted on the
  `aegis::sandbox` target. With `sandbox.required = true`, unavailability is a
  hard block.

### 5.6 Audit log

- Append-only JSONL at `~/.aegis/audit.jsonl`; the file is only appended to.
- Each entry is an `AuditEntry` (typed enum: `Decision` / `Watch`).
- A `sandbox_status` field (`active` / `unavailable` / `not_configured`); the
  legacy `sandbox_active` boolean is mirrored for backward compatibility.
- Tamper detection: SHA-256 hash chain, mode `ChainSha256` enabled **by default**
  (opt-out, not opt-in).
- **Concurrent writes:** appends are serialized with an advisory file lock
  (`flock`) so parallel Aegis processes (multiple agent sessions) cannot interleave
  entries and break the hash chain. The lock is held only for the duration of a
  single append.
- Any audit write failure is a hard error with a non-zero exit code, regardless
  of `verbose`.
- The log format is part of the public contract from 1.0.

### 5.7 Toggle and CI contract

- `aegis on` / `aegis off` / `aegis status` (global `~/.aegis/disabled` flag).
- In disabled mode outside CI, Aegis behaves as if it is not installed
  (zero-noise) while preserving the toggle history.
- In a detected CI environment, policy stays enforced by default; `AEGIS_CI`
  explicitly overrides CI detection in either direction.

### 5.8 Agent integrations

- **First-class:** Claude Code and Codex — via hook integration
  (`aegis install-hooks --all`), including the shared toggle helper.
- Other agents (Cursor and anything that respects `$SHELL`) work via `$SHELL` on
  a best-effort basis; documented, but not first-class.
- Hook installation is binary-first: it updates existing `~/.claude` / `~/.codex`
  directories and skips missing ones without creating them.

### 5.9 Configuration

- TOML: `~/.config/aegis/config.toml` (global) and `.aegis.toml` (per-project),
  with layered merge.
- All fields are optional with defaults via `#[serde(default)]`; backward
  compatibility with existing config files is not broken.
- `aegis config init|show|validate`; a JSON schema is generated from the type for
  editor autocompletion.

---

## 6. Non-Functional Requirements

- **Performance:** safe hot path < 2 ms (p99). Any change to `scanner`/`parser`
  is benchmarked with `cargo criterion`; regressions are not allowed.
- **Parsing correctness:** the parser is a security-critical input; fuzzing is
  mandatory (parser, scanner, heredoc unwrapping).
- **Dependency security:** `cargo audit` and `cargo deny check` pass with zero
  findings; permissive licenses only (MIT/Apache-2.0/ISC); no duplicate core
  crates and no banned crates.
- **Portability:** no dependencies with a C build step; a statically portable
  binary.
- **Architecture:** edition 2024, MSRV `1.80`, no file in `src/` exceeds 800 LoC,
  the crate dependency DAG is enforced by `tests/architecture_boundaries.rs`.
- **Code:** no `.unwrap()`/`.expect()` in production paths; typed errors
  (`thiserror`) in libraries, `anyhow` in bin glue; libraries never write to
  stdout (only `tracing`).

---

## 7. Distribution

Officially supported 1.0 channels:

1. **curl | sh** — convenience installer (global-first), verifying the checksum
   before writing the binary.
2. **GitHub Releases** — prebuilt binaries with `.sha256` sidecars for all
   supported targets.
3. **Homebrew** — official formula/tap for macOS and Linux.
4. **npm** — a wrapper package that downloads and installs the platform binary
   (for the audience used to `npm i -g`).
5. **cargo install** — build from source as a fallback for platforms without a
   prebuilt binary.

---

## 8. Platforms

| Platform               | Shell proxy  | Sandbox                         |
| ---------------------- | ------------ | ------------------------------- |
| Linux x86_64           | ✅           | bubblewrap + Landlock           |
| Linux aarch64          | ✅           | bubblewrap + Landlock           |
| macOS arm64            | ✅           | Seatbelt (`sandbox-exec`)       |
| macOS x86_64           | ✅           | Seatbelt (`sandbox-exec`)       |
| Windows (WSL2)         | ✅ (Linux)   | bubblewrap + Landlock (Linux)   |

- Native Windows is **not** supported. Native Windows shells (PowerShell,
  cmd.exe) do not work; Aegis runs on Windows only inside WSL2, where it is a
  Linux environment.
- Automatic shell setup recognizes `bash` and `zsh`; others via `AEGIS_SHELL_RC`.
- Sandbox unavailability on a platform/environment is an audit event (§5.5), not
  a silent skip.

---

## 9. Success Metrics

### Technical (quality)

- **Zero false negatives** on the bypass corpus
  (`tests/fixtures/security_bypass_corpus.toml`): no dangerous command from the
  corpus is classified as `Safe`.
- **Hot path < 2 ms (p99)** on safe commands, confirmed by `cargo criterion`.
- **0 CVEs** in dependencies (`cargo audit`) and a clean `cargo deny check`.
- **Green CI on all supported platforms:** Linux (x86_64/aarch64) and macOS
  (arm64/x86_64). Windows is covered transitively via the Linux target (WSL2).

### Security (impact)

- **Dangerous-pattern coverage:** every built-in pattern has ≥1 positive and ≥1
  negative test; all `RiskLevel` variants are covered both ways.
- **Share of commands executed in the sandbox** (from the audit log) when the
  sandbox is enabled.
- **Prevented incidents:** the count of `Block` and `Deny` on `Danger` commands
  in the audit log as an indicator of real protection.

---

## 10. Release Readiness Criteria 1.0 (Definition of Done)

- [ ] README and docs accurately describe all features through Phase 6.
- [ ] Convenience installer documented and tested (`curl | sh`).
- [ ] Homebrew formula/tap published and tested.
- [ ] npm wrapper published and installs the correct platform binary.
- [ ] Release workflow exercised on a real tag; artifacts include `.sha256`
      sidecars.
- [ ] CI includes ARM cross-compilation (`aarch64-unknown-linux-musl`).
- [ ] Threat model and known limitations visible on the README.
- [ ] Typed TOML Policy DSL (5.2) implemented; invalid rules produce a startup
      error with line numbers; hot path shows no regression.
- [ ] Sandbox works and is tested on `ubuntu-latest` and `macos-latest`; a
      command writing outside allowed paths is killed by the sandbox; audit
      records the applied profile/status for every execution.
- [ ] Snapshot/rollback integration tests run in CI against real Docker/SQLite
      daemons.
- [ ] Fuzz corpus in CI at ≥ 100 000 iterations per target.
- [ ] `cargo audit` and `cargo deny check` both pass with zero findings.
- [ ] CHANGELOG.md updated for every release.

---

## 11. Out of Scope for 1.0 (Non-Goals)

- Team/centralized mode: shared server-side policies, multi-user management,
  centralized audit collection.
- Native Windows shells (PowerShell, cmd.exe) — WSL2 only.
- Protection against obfuscation, encoding, and runtime `eval` of commands —
  beyond the heuristic model (documented in the threat model).
- SBOM, provenance metadata, and attestations in the release workflow.
- A guarantee of byte-for-byte reproducible builds across all environments.
- First-class integration with agents other than Claude Code and Codex (others
  go through `$SHELL`, best-effort).
