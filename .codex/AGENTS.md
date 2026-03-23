# Aegis for Codex CLI

This file is the Codex-specific layer for this repository.

## Source of Truth

Before non-trivial work, read in this order:
1. `.claude/CLAUDE.md`
2. `.claude/AGENTS.md`
3. `CONVENTION.md`
4. this file

If rules conflict, follow the precedence defined in `CONVENTION.md` and `.claude/CLAUDE.md`.

## Codex Operating Baseline

- All shell commands must be run through `rtk`.
- Never execute raw commands (`cargo`, `git`, `rg`, `sed`, etc.).
- Keep `src/main.rs` thin.
- Keep parser/scanner hot path synchronous.
- No `unsafe {}`.
- No `unwrap()` / `expect()` in non-test runtime code unless startup panic is explicitly part of the contract.
- Do not weaken interception, approval, snapshot, or audit guarantees.

## Multi-Agent Roles

Project roles are configured in `.codex/config.toml` and `.codex/agents/*.toml`.

- `explorer`: read-only evidence gathering with file/symbol references
- `reviewer`: correctness/security/regression-focused review
- `security_reviewer`: fail-open and bypass analysis for Aegis protections
- `docs_researcher`: primary-source docs and version/date verification

## Verification Defaults

Run relevant checks (via `rtk`) for touched areas:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
rtk cargo bench --bench scanner_bench
rtk cargo audit
rtk cargo deny check
```

## Sensitive Files

Treat edits in these files as security-sensitive and review carefully:

- `src/main.rs`
- `src/interceptor/parser.rs`
- `src/interceptor/scanner.rs`
- `src/interceptor/patterns.rs`
- `src/ui/confirm.rs`
- `src/config/model.rs`
- `src/config/allowlist.rs`
- `src/snapshot/mod.rs`
- `src/snapshot/git.rs`
- `src/snapshot/docker.rs`
- `src/audit/logger.rs`
