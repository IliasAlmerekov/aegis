# Aegis architecture decisions

This document captures the current architecture-level decisions, documented
non-goals, and verification guidance that shape the Aegis codebase.

It is the companion to:

- `README.md` — user-facing behavior and install flow
- `docs/threat-model.md` — security posture, mitigations, and residual risks
- `CONVENTION.md` — project-level contract for architecture, style, and release gates

## Current architecture snapshot

Aegis is a single-crate Rust CLI that acts as a shell-proxy guardrail.

The current runtime architecture is split across a small set of focused modules:

- `src/main.rs` — CLI argument parsing and top-level orchestration only
- `src/interceptor/parser.rs` — shell parsing and segmentation
- `src/interceptor/scanner.rs` — synchronous risk classification
- `src/interceptor/patterns.rs` — built-in and user pattern loading
- `src/runtime_gate.rs` — Rust-side CI detection contract
- `src/toggle.rs` — global on/off toggle state rooted at `~/.aegis/disabled`
- `src/watch.rs` — NDJSON watch-mode control loop
- `src/snapshot/` — best-effort Git / Docker snapshot providers
- `src/audit/logger.rs` — append-only JSONL audit writer and integrity handling
- `scripts/hooks/` — Claude / Codex hook payloads and the shared shell-side toggle helper template
- `scripts/install.sh` / `scripts/uninstall.sh` — convenience installer and managed cleanup

At a high level, the current product contract is:

1. intercept commands before execution
2. classify them as `Safe`, `Warn`, `Danger`, or `Block`
3. require confirmation or hard-block according to policy
4. record the decision in the audit log
5. optionally snapshot before dangerous execution when configured

The global toggle and hook integrations extend that flow, but do not replace the
core classification / approval pipeline when enforcement is active.

## ADR-001 — Keep the CLI entrypoint thin

`src/main.rs` is intentionally orchestration-only.

Why:

- it keeps the CLI surface readable
- it reduces the risk of mixing policy, parsing, and execution details together
- it makes security-sensitive behavior easier to review in focused modules

Implication:

- business logic belongs in focused modules such as `toggle`, `runtime_gate`,
  `watch`, `runtime`, `decision`, `interceptor`, and `snapshot`

## ADR-002 — The interception hot path stays synchronous

Command parsing and classification are performance-sensitive and remain
synchronous by design.

Why:

- safe-command latency matters
- parser / scanner behavior is easier to reason about without async scheduling
- the project contract explicitly treats this path as benchmark-sensitive

Implication:

- async is acceptable for subprocess / snapshot work
- async is not introduced into `src/interceptor/`

## ADR-003 — Aegis is a heuristic guardrail, not a sandbox

Aegis intentionally operates on raw command text and policy decisions before the
real shell runs.

Why:

- it is meant to reduce accidental damage, not provide OS isolation
- approved commands still run with the operator's normal privileges

Implication:

- docs must not describe Aegis as a hard security boundary
- limitations around encoded input, deferred execution, and shell/runtime
  expansion remain explicit non-goals

## ADR-004 — Snapshots are best-effort; audit is append-only

Recovery and forensics are important, but they are not symmetric guarantees.

Current contract:

- snapshots are provider-based and best-effort
- rollback may still fail or conflict
- audit output remains append-only JSONL
- stronger tamper-evidence is available only when audit integrity mode is enabled

Implication:

- docs must describe rollback honestly
- audit integrity claims must stay tied to the configured integrity mode

## ADR-005 — Global toggle at command boundaries

The current full-disable model uses a single global flag file:

- `~/.aegis/disabled`

Rust-side behavior:

- `src/toggle.rs` owns disabled-flag path resolution and status helpers
- `aegis on` removes the flag
- `aegis off` creates or refreshes the flag
- toggle commands audit best-effort and do not roll back the state change when
  audit writing fails

Command-boundary semantics:

- shell-wrapper mode snapshots toggle state once before enforcement-related I/O
- watch mode snapshots toggle state once per command-boundary gate
- disabled local mode is intentionally quiet for ordinary shell / supported hook usage

Why:

- a single metadata existence check is cheap and predictable
- command-boundary semantics are easier to reason about than mid-command mutation

Implication:

- toggle detection must not rely on TOML parsing or heavy config reloads
- malformed parent paths should not silently imply disabled mode

## ADR-006 — CI detection has an explicit override contract

CI detection is centralized in `src/runtime_gate.rs`.

Current precedence:

1. `AEGIS_CI` explicit override
   - truthy (`1`, `true`, `yes`) forces CI behavior
   - falsy (`0`, `false`, `no`) forces non-CI behavior
2. well-known CI environment variables (`CI`, `GITHUB_ACTIONS`, `GITLAB_CI`, etc.)
3. non-empty `JENKINS_URL`

Why:

- CI semantics must be consistent across shell-wrapper mode, watch mode, status,
  and hook integrations

Implication:

- docs should say that detected CI environments enforce by default, while
  `AEGIS_CI` can explicitly override CI detection

## ADR-007 — Shell hooks share one managed helper, but must not fail open

Installed Claude / Codex hooks use the shared helper path:

- `~/.aegis/lib/toggle-state.sh`

The repository template lives at:

- `scripts/hooks/toggle-state.sh`

However, the installed hooks also embed minimal fallback CI / disabled-state
logic so helper loss does not create a fail-open enforcement bypass.

Why:

- shared logic keeps the normal path consistent
- inline fallback prevents helper-missing regressions from silently disabling
  Codex / Claude enforcement behavior

Implication:

- any semantic change to hook-side toggle detection must keep the shared helper
  and inline fallback behavior aligned

## ADR-008 — Installer is global-first and rejects removed mode controls

The convenience installer now has one supported path:

- global shell setup with managed RC-file integration

The old `AEGIS_SETUP_MODE` and `AEGIS_SKIP_SHELL_SETUP` controls are rejected
explicitly instead of being silently ignored.

Current installer behavior:

- validates shell / rc-file setup before downloading and installing the binary
- auto-attempts local `agent-setup.sh` only when a real local checkout is present
- reports hook setup honestly:
  - success
  - skip because no supported agent directories were detected
  - failure with next steps

Implication:

- “binary-only” workflows now use the manual / source-install guidance rather
  than a convenience-installer mode switch

## ADR-010 — Full shell evaluation and deferred execution remain non-goals

This ADR is the important design limitation referenced by the threat model and
security policy.

Aegis does **not** aim to fully model:

- `eval "$(…)"`-style runtime assembly
- alias or shell-function expansion that changes behavior after raw command parsing
- a safe-looking write followed by a later dangerous invocation
- encoded / obfuscated payloads outside the implemented heuristic coverage
- shell semantics that only become visible after handing execution to the real shell

Why:

- those problems move Aegis toward becoming a full shell interpreter or sandbox,
  which is outside the project scope

Implication:

- reports based only on these known non-goals are documentation / roadmap inputs,
  not product vulnerabilities by themselves

## Fuzzing and verification guidance

Current fuzz targets live under:

- `fuzz/fuzz_targets/parser.rs`
- `fuzz/fuzz_targets/scanner.rs`

They are the preferred stress path for parser / scanner edge cases, alongside:

- focused regression tests in `tests/`
- `cargo bench --bench scanner_bench` for hot-path changes

When a change affects parsing, scanning, or command-boundary behavior, the
expected verification posture is:

- regression tests for positive and negative cases
- clippy / fmt / full test suite where relevant
- benchmark evidence for hot-path-sensitive changes
- fuzz target maintenance rather than documentation-only confidence

## References

- `README.md`
- `docs/threat-model.md`
- `docs/config-schema.md`
- `docs/release-readiness.md`
- `tests/toggle_cli.rs`
- `tests/agent_hooks.rs`
- `tests/installer_flow.rs`
- `tests/full_pipeline.rs`
