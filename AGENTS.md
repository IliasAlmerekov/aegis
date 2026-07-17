# AGENTS.md — Aegis (Codex instructions)

@RTK.md

## Project Overview

Aegis is a lightweight Rust CLI that acts as a `$SHELL` proxy for AI coding
agents (Claude Code, Codex). It intercepts every command an agent tries to
run, classifies it (`Safe` / `Warn` / `Danger` / `Block`), and requires human
confirmation before anything destructive executes. It is a heuristic
guardrail, not a sandbox — see `docs/adr/adr-003-aegis-is-a-heuristic-guardrail-not-a-sandbox.md`.
It must stay fast (< 2 ms on the safe-command hot path), correct, and minimal.

The binary lives at the workspace root; the actual logic is split across
focused library crates under `crates/` (parser, scanner, policy engine,
config, snapshot backends, audit log, TUI). See `ARCHITECTURE.md` for the
current structural contract and `docs/adr/` for why it looks this way.

## Session Context — mandatory before starting a task

**Before any non-trivial task**, read in this order:

1. [`PROJECT_STATE.md`](PROJECT_STATE.md) — what changed last session, current
   milestone status, open blockers. Do not skip this step.
2. [`CONVENTION.md`](CONVENTION.md) — authoritative contract for style,
   architecture, security invariants, dependency rules, testing, and release
   gates. When it conflicts with other documents, use the precedence order
   defined inside `CONVENTION.md` (security invariants → CI-enforced rules →
   `CONVENTION.md` → contributor guidance).
3. [`CONTEXT.md`](CONTEXT.md) — the project's domain glossary. Use its
   canonical terms, and avoid the words listed under each term's `_Avoid_`, in
   code, commits, and PR descriptions.
4. [`TASKS.md`](TASKS.md) — the open security-finding backlog blocking 1.0
   (P0/P1/P2), with a `[ ]`/`[x]` status per item.

**After completing any significant change**, update:

- `CHANGELOG.md` — prepend one line under `## [Unreleased]` (Keep a Changelog
  categories: `Added`, `Changed`, `Fixed`, `Removed`, `Security`), referencing
  the ADR or milestone ID when applicable.
- `PROJECT_STATE.md` — update "Last session" with what changed and what was
  verified, any `Milestone status` rows that changed, and `Open decisions /
  blockers` if something was resolved or newly surfaced. Keep it terse —
  history belongs in git and `CHANGELOG.md`.
- `TASKS.md` — flip the relevant `[ ]` to `[x]` only if the task closes a
  tracked finding and verification passed.
- `CONTEXT.md` — if the task introduces or sharpens a domain term, record it in
  the same change.

Do not fill these files in before the task is actually done and verified.

## Agent Configuration

Before starting any non-trivial task, use the global skills installed under
`~/.agents/skills/` (symlinked into `~/.codex/skills/`) in this order:

1. **`grill-me`** (or **`grill-with-docs`** when a PRD/spec already exists) —
   interview the task to a shared understanding before writing a plan. Plans for
   Aegis security-finding work belong under `docs/superpowers/plans/` unless a
   task says otherwise.
2. **`tdd`** — implement the planned slice red-green.
3. **`code-review`** — review the diff on the Standards and Spec axes.
4. **`re-review`** — adversarially verify `code-review` findings are real,
   then, after `tdd` fixes them, confirm the fix actually closed them. The
   loop is capped at 2 rounds; see `~/.agents/ENGINEERING_GATES.md`.

Only push and open a PR once `re-review` reports a clean cycle.

The Definition-of-Done checklist, `TASKS.md` traceability convention, and
branch protection policy are defined once, project-agnostically, in
`~/.agents/ENGINEERING_GATES.md` — consult it, don't duplicate it here.

This project's required GitHub branch-protection status checks are the CI job
contexts from `.github/workflows/ci.yml`:

- `Determine heavy-job gate`
- `Quality (fmt, clippy, test)`
- `Security (audit, deny)`
- `Release build (ubuntu-latest)`
- `Release build (macos-26)`
- `Performance baseline (scanner bench)`
- `Live installer validation (ubuntu-latest)`
- `Live installer validation (macos-26)`
- `Live snapshot/rollback (Docker + SQLite)`
- `Fuzzing (parser + scanner + heredoc)`

Do not require `release.yml` jobs for ordinary branch protection; that workflow
is for tagged release artifacts.

## Rust Skills

**Always load and follow the `rust-best-practices` skill
(`~/agents/skills/rust-best-practies/SKILL.md`) before writing or reviewing
Rust code in this repo.** It encodes the idiomatic-Rust guidance this project
expects (ownership/borrowing, error handling, testing style). Apply it on top
of — never instead of — `CONVENTION.md`, which is authoritative for this
project's specific architecture, security invariants, and release gates.

## Execution Policy

- All shell commands must go through `rtk` (for example `rtk cargo build`,
  `rtk cargo test`, `rtk git status`).
- Never execute raw commands directly (`cargo`, `git`, `rustc`, etc.).
- `rtk` is an agent-side execution guard and context-noise reducer; it is not
  part of Aegis' runtime product contract or a requirement for end users.

Aegis governs command execution in this repo. If Aegis denies a command, that
decision must be respected — do not propose shell-escape workarounds or
out-of-band bypasses for a blocked risky command.

## Architecture decisions

When you make a significant architectural decision (new crate, public API
change, security-model change, intentional non-goal), write an ADR in
`docs/adr/` — number sequentially, required sections: Status, Context,
Decision, Consequences. Update `docs/adr/README.md`'s index in the same
change. Do not record decisions anywhere else (not in `PROJECT_STATE.md`, not
in scattered planning docs).

## Verification

For code changes, finish the change, then verify with:

- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
- a benchmark run if the hot path was touched

Only mark tasks complete or update completion state after the relevant gates
are green. For docs-only changes, verify the edited files and skip product
runtime gates unless the docs change asserts runtime behavior that should be
tested.

## Commit style

Short conventional commits (`feat:`, `fix:`, `perf:`, `test:`, `docs:`).
Never add a `Co-Authored-By` trailer or other attribution footer.

## What not to do

- Do not put business logic in `main.rs`.
- Do not use `regex` in the scanner's quick first pass — Aho-Corasick only.
- Do not block the main thread during subprocess calls — use `tokio`.
- Do not add dependencies that require a C build step, except for the pinned
  Tree-sitter runtime and production-qualified generated grammars under ADR-022.
  This is a narrow exception, not permission for general native dependencies.
- Do not use `once_cell` — use `std::sync::LazyLock` (stable since Rust 1.80).
- Do not `.unwrap()`/`.expect()` on a production path — see `CONVENTION.md`.
