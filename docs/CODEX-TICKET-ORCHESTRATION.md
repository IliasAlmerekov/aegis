# Codex Ticket Orchestration (Aegis)

## Purpose

Define a deterministic lead-agent loop for ticket execution until complete closure.

## Inspiration

This workflow is adapted from orchestration patterns in `everything-claude-code`:

- `AGENTS.md` (agent-first execution)
- `commands/plan.md` (plan gate before coding)
- `docs/COMMAND-AGENT-MAP.md` (explicit routing)

## Roles

- `lead_orchestrator`: ticket owner, planner, loop controller
- `coder`: implementation for one scoped task per cycle
- `tester`: test creation/updates and verification
- `reviewer`: correctness and regression review
- `security_reviewer`: security and fail-open/bypass review

## Global Rule For All Agents

Every agent must read and follow `CONVENTION.md` before starting work and must not
propose or apply changes that violate it.

## Mandatory Loop

1. Lead creates a plan and acceptance criteria.
2. Coder implements one scoped task.
3. Tester verifies behavior and test sufficiency.
4. Reviewer performs code review.
5. Security reviewer performs security review.
6. If any stage returns `CHANGES_REQUESTED`, lead routes findings back to coder.
7. Repeat until all stages are `PASSED`.

## Ticket Artifacts

Per ticket, maintain `docs/tickets/<ticket-id>/`:

- `plan.md`
- `coder.md`
- `tester.md`
- `reviewer.md`
- `security.md`
- `summary.md`

Each file must include:

- status (`PASSED` | `CHANGES_REQUESTED` | `BLOCKED`)
- evidence (paths + command summary)
- next owner

## Minimum Verification

All commands must use `rtk`:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
```

Add `rtk cargo bench --bench scanner_bench` when parser/scanner/hot-path is affected.
Add `rtk cargo audit` and `rtk cargo deny check` for dependency/security-sensitive changes.

## Security-Sensitive Scope

Always treat these paths as high-risk review scope:

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
