# AGENTS.md — Aegis (Codex instructions)

## What this project is

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

## Before writing any code

**Always load and follow the `rust-best-practices` skill
(`~/agents/skills/rust-best-practies/SKILL.md`) before writing or reviewing
Rust code in this repo.** It encodes the idiomatic-Rust guidance this project
expects (ownership/borrowing, error handling, testing style). Apply it on top
of — never instead of — `CONVENTION.md`, which is authoritative for this
project's specific architecture, security invariants, and release gates.

## Read first, in this order

1. `PROJECT_STATE.md` — what changed last session, current milestone status,
   open blockers. Read at the start of every non-trivial task.
2. `CONVENTION.md` — authoritative contract for style, architecture, security
   invariants, dependency rules, testing, and release gates. Precedence order
   is defined inside it: security invariants → CI-enforced rules →
   `CONVENTION.md` → contributor guidance.
3. `CONTEXT.md` — the project's domain glossary. Use its canonical terms (and
   avoid the words listed under each term's `_Avoid_`) in code, commits, and
   PR descriptions. If a task sharpens or introduces a domain term, update
   `CONTEXT.md` in the same change.
4. `TASKS.md` — the open security-finding backlog blocking 1.0 (P0/P1/P2),
   with a `[ ]`/`[x]` status per item.

## Shell commands

Always prefix shell commands with `rtk` (e.g. `rtk cargo build`, `rtk cargo
test`, `rtk git status`) to reduce context noise. Never run bare `cargo`,
`git`, `rustc`, etc.

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

## After finishing a task — verify, then document

Do not fill these files in before the task is actually done and verified.
Order matters:

1. **Finish the change.**
2. **Verify it**: `rtk cargo test --workspace`, `rtk cargo clippy -- -D
   warnings`, `rtk cargo fmt --check`, plus a benchmark run if the hot path
   was touched. Only proceed once this is green.
3. **Only then, in the same change, fill in:**
   - `CHANGELOG.md` — prepend one line under `## [Unreleased]` (Keep a
     Changelog categories: `Added`, `Changed`, `Fixed`, `Removed`,
     `Security`), referencing the ADR or milestone ID when applicable.
   - `PROJECT_STATE.md` — update "Last session" with what changed and what
     was verified, any `Milestone status` rows that changed, and
     `Open decisions / blockers` if something was resolved or newly
     surfaced. Keep it terse — this file is a pointer, not a log; history
     belongs in git and `CHANGELOG.md`.
   - `TASKS.md` — flip the relevant `[ ]` to `[x]` if the task closes a
     tracked finding.

Never mark a task done in `PROJECT_STATE.md`/`TASKS.md`/`CHANGELOG.md` before
verification actually passed.

## Commit style

Short conventional commits (`feat:`, `fix:`, `perf:`, `test:`, `docs:`).
Never add a `Co-Authored-By` trailer or other attribution footer.

## What not to do

- Do not put business logic in `main.rs`.
- Do not use `regex` in the scanner's quick first pass — Aho-Corasick only.
- Do not block the main thread during subprocess calls — use `tokio`.
- Do not add dependencies that require a C build step.
- Do not use `once_cell` — use `std::sync::LazyLock` (stable since Rust 1.80).
- Do not `.unwrap()`/`.expect()` on a production path — see `CONVENTION.md`.
