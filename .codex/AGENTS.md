# AGENTS.md — Aegis Codex CLI Conventions

## Agent Configuration

Before starting any non-trivial task, read `CLAUDE.md` (repo root). It defines the
lead-orchestrator model, global constraints, and repository rules that apply to Codex
CLI sessions in this repository. The implemented agent roles live in `.codex/agents/`.

---

## Project Conventions

Always follow `CONVENTION.md` — it is the authoritative project-level contract for
architecture, style, security invariants, dependencies, testing, and release gates.
If rules conflict, follow the precedence order defined in `CONVENTION.md`.
This requirement is mandatory for all agents in the orchestration loop
(lead orchestrator, `red-tester`, `green-tester`, `reviewer`).

---

## Ubiquitous Language

`CONTEXT.md` (repo root) is the project's domain glossary — the single source of truth
for terminology shared by humans, agents, and code. Before naming a type, field, config
key, or audit field — or describing a concept in a PR or commit — use the exact canonical
term from `CONTEXT.md` and avoid the words under each term's `_Avoid_`. When a task
introduces or sharpens a domain term, update `CONTEXT.md` in the same change (via the
`domain-modeling` skill). Resolve any term that conflicts with the glossary (e.g. the
three meanings of "block") against `CONTEXT.md` before writing code.

---

## Execution Policy

All shell commands must go through `rtk`.

Examples:

```bash
rtk cargo build
rtk cargo test
rtk git status
rtk rg "RiskLevel" src
```

Never execute raw commands.

If Aegis denies a risky command, do not suggest bypassing the guardrail. Do not
recommend `! <command>` or equivalent escape syntax as a workaround. Hand off to
the operator using neutral language with an explicit risk explanation.

---

## Commit Style

Use short conventional commits. Never add `Co-Authored-By` trailers.

---

## Session Context

**At the start of every session:** read `PROJECT_STATE.md` to understand what was done
before and where the project stands. Do not skip this step on non-trivial tasks.

**After completing any significant change:** update `PROJECT_STATE.md`:
- Update "Last updated" date.
- Replace "What was done last session" with a concise summary of what changed this session.
- Update "Milestone status" rows whose status changed.
- Update "Open decisions / blockers" if any were resolved or new ones surfaced.

---

## Changelog Maintenance

After every feature, fix, or breaking change, prepend an entry under `## [Unreleased]`
in `CHANGELOG.md`:

- Use Keep a Changelog categories: `Added`, `Changed`, `Fixed`, `Removed`, `Security`.
- One line per change; reference the milestone (e.g. `M5.4`) or ADR (e.g. `ADR-011`)
  when applicable.
- When cutting a release, rename `[Unreleased]` to the version and date, then add a fresh
  empty `[Unreleased]` block above it.

---

## Architecture Decision Records

When making a significant architectural decision — new crate, change to a public API,
new plugin, performance trade-off, security model change, or intentional non-goal —
write an ADR in `docs/adr/`:

- Number sequentially: check existing files (`ls docs/adr/`) for the next free number.
- Filename: `adr-NNN-short-slug.md`.
- Required sections: **Status** (Accepted / Proposed / Deprecated), **Context**,
  **Decision**, **Consequences**.
- Keep it short — one page max.
- Update `docs/adr/README.md` index after adding a new ADR.

---

## Lead-Orchestrated Ticket Flow

Use the lead-agent `/implement` workflow. The implemented agents are defined in
`.codex/agents/` (Codex) and `.claude/agents/` (Claude); the pipeline is in
`.claude/commands/implement.md`:

1. Lead orchestrator accepts the ticket and frames the task.
2. `red-tester` writes failing tests that specify the behavior (or reproduce the bug).
3. `green-tester` writes the minimal implementation to make them pass.
4. `reviewer` performs correctness, regression, and security-regression review
   (its checklist covers bypass / fail-open / audit-integrity concerns).
5. On `CHANGES_REQUESTED`, the loop returns to `red-tester` with concrete findings,
   for a maximum of 3 iterations; then the lead agent takes over directly.

For a deeper, dedicated security pass on a finished change, the lead runs the
`code-review` skill (a parallel panel of specialist reviewers) before merge.

### State Machine

- `NEW -> RED -> GREEN -> REVIEW -> DONE`
- Fail transition: `REVIEW -> RED` (up to 3 iterations, then lead takeover).
- `BLOCKED` can be entered from any state and requires a human checkpoint.

### Handoff Contract

Each stage must return:

- Status: `RED TESTS WRITTEN` / `GREEN` / `APPROVED` / `CHANGES_REQUESTED` / `BLOCKED`
- Evidence: changed files and verification output summary
- Next owner: explicit next agent/stage

A ticket can close only after `reviewer` returns `APPROVED`.

---

## Project Overview

Aegis is a lightweight Rust CLI acting as a `$SHELL` proxy. It intercepts commands,
classifies risk, blocks unsafe operations, and requires explicit confirmation for risky
actions while preserving performance and correctness.

---

## Rust & Toolchain

- Edition: 2024
- Format with `rustfmt`
- Lint with `clippy` and resolve all warnings before merge
- Keep `src/main.rs` thin and push logic into modules

---

## Global Constraints for Codex CLI

- Read `CLAUDE.md` (repo root) before implementation work.
- See `.codex/agents/` for orchestration roles and `CONVENTION.md` for safety boundaries.
- Route every shell command through `rtk`.
- Do not run raw `cargo`, `git`, `rg`, `sed`, or other CLIs.
- Do not introduce `unsafe {}`.
- Do not add `unwrap()` / `expect()` in non-test code unless explicitly justified by startup contract.
- Preserve the interception/approval/snapshot/audit guarantees.
- Keep `src/interceptor/` synchronous and benchmark-sensitive.
- Do not modify dependency or CI policy files without explicit human approval.

---

## Security-Sensitive Paths

Any changes in these files require extra care and explicit validation:

- `src/main.rs`
- `crates/aegis-parser/` (shell tokenizer + PrefixPattern matching)
- `src/interceptor/parser/` (re-export shim over `aegis-parser`)
- `crates/aegis-scanner/` (Scanner, PatternSet, built-in patterns.toml)
- `src/interceptor/scanner.rs` (re-export shim over `aegis-scanner`)
- `src/interceptor/patterns.rs` (re-export shim over `aegis-scanner`)
- `crates/aegis-policy/` (PolicyEngine — allow/block decision)
- `src/decision/` (re-export shim over `aegis-policy`)
- `src/ui/confirm.rs`
- `crates/aegis-config/` (config model, loader, validation, amend)
- `src/config/` (re-export shim over `aegis-config`)
- `src/snapshot/mod.rs`
- `src/snapshot/git.rs`
- `src/snapshot/docker.rs`
- `src/audit/logger.rs`

---

## Verification Baseline

When relevant to a change, validate with:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
rtk cargo bench --bench scanner_bench
rtk cargo audit
rtk cargo deny check
```

---

## Orchestration References

For command-style routing and role mapping, see:

- `.claude/commands/implement.md` — the `/implement` pipeline definition.
- `.codex/agents/` — the `red-tester`, `green-tester`, and `reviewer` role prompts.
