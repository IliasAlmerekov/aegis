---
name: plan_feature
description: Decompose research into an ordered, dependency-aware implementation plan with atomic tasks, phase sequencing, Aegis-specific quality gates, and a mandatory human confirmation before implementation begins.
allowed_tools: ["Read", "Write", "Grep", "Glob", "Agent"]
---

# Command: plan_feature

## PURPOSE

Decompose the research into an ordered, dependency-aware implementation plan with
atomic tasks, phase sequencing, quality gates, and a mandatory confirmation gate
before implementation begins.

## INVOCATION

```
/plan_feature {ticket_id}
```

## PRECONDITIONS

The following must exist and be complete:

- `docs/{ticket_id}/research.md` — with all 8 required sections

If missing or incomplete:

```
❌ plan_feature aborted
Ticket:  {ticket_id}
Missing: {specific file or section}
Fix:     run /research_codebase first
```

---

## LEAD AGENT BEHAVIOR

### Step 1 — Validate Input

Read `docs/{ticket_id}/research.md`. Verify all 8 sections are present:
`Ticket Summary`, `Feature Domain`, `Affected Modules`, `Files Involved`,
`Execution Path`, `Integration Boundaries`, `Open Questions`, `Conventions to Follow`.

If any section is missing or empty, halt with the aborted message above.

### Step 2 — Feed to Planner Agent

Pass `research.md` to the planner agent. Planner produces `docs/{ticket_id}/plan.md`.

#### Planner responsibilities:

**A. Resolve Open Questions**
For each entry in `## Open Questions` from research.md:

- If the question can be answered by reading the codebase — answer it.
- If it requires a developer decision — flag it as `DECISION REQUIRED` in the plan.
- Do not proceed past Step 2 if any `DECISION REQUIRED` items exist — surface them first.

**B. Decompose into Atomic Tasks**
Break the feature into tasks. Each task must be:

- Completable in a single focused session (no multi-day tasks)
- Independently verifiable (has its own quality gate)
- Assigned to exactly one phase (see phases below)

Task table format:

```
| ID     | Phase | Description                                  | Complexity | Needs Human Approval |
|--------|-------|----------------------------------------------|------------|----------------------|
| T-001  | 1     | Add `UnicodeNorm` variant to `Category` enum | S          | No                   |
| T-002  | 2     | Implement normalization in `parser.rs`        | M          | No                   |
| T-003  | 4     | Add 6 fixture cases to `commands.toml`        | S          | No                   |
```

Complexity: `S` = < 30 min, `M` = 30–90 min, `L` = > 90 min.

**C. Assign to Phases**
Use these phases in order. Skip phases that have no tasks.

| Phase | Name        | What belongs here                                                                                                               |
| ----- | ----------- | ------------------------------------------------------------------------------------------------------------------------------- |
| 1     | Types       | New types, enums, error variants, trait definitions. Changes to `AegisError`, `RiskLevel`, `Category`, `Pattern`, `AuditEntry`. |
| 2     | Domain      | Core logic: `patterns.rs` new entries, `scanner.rs` assess() changes, `parser.rs` tokenizer changes, `SnapshotPlugin` impls.    |
| 3     | Integration | Wiring: `main.rs` changes, config loading (`AegisConfig`), new CLI flags (clap), snapshot plugin registration.                  |
| 4     | Tests       | Unit tests (inline `#[cfg(test)]`), fixture cases in `tests/fixtures/commands.toml`, integration tests in `tests/integration/`. |
| 5     | Benchmarks  | `benches/scanner_bench.rs` — only if Phase 2 touched `scanner.rs` or `parser.rs`.                                               |
| 6     | Docs        | `AEGIS.md` pattern table updates, inline comments for non-obvious logic.                                                        |

Phases must be respected: Phase N tasks may not depend on Phase N+1 tasks.

**D. Flag Human Approval Required**
Mark `Needs Human Approval = Yes` for any task that:

- Changes `AuditEntry` fields or JSONL format (public contract from v1 — breaking change)
- Adds a `Block`-level pattern (highest risk, no recovery path for blocked commands)
- Changes shell passthrough behavior (fail-open vs fail-closed default)
- Modifies config field names or removes config keys (backwards-compatibility contract)
- Adds a new dependency to `Cargo.toml`
- Changes exit code behavior

**E. Write Quality Gates per Phase**
For each phase used, specify what must pass before the next phase begins:

| Phase | Gate                                                                                                   |
| ----- | ------------------------------------------------------------------------------------------------------ |
| 1     | `rtk cargo build` clean                                                                                |
| 2     | `rtk cargo build` + `rtk cargo clippy -- -D warnings` clean                                            |
| 3     | `rtk cargo build` + `rtk cargo clippy` + `rtk cargo test` green                                        |
| 4     | `rtk cargo test` 100% pass + all new fixture cases passing + FNR = 0 for any new Danger/Block patterns |
| 5     | `rtk cargo criterion` safe-path p99 < 2ms                                                              |
| 6     | `rtk cargo audit` clean + `rtk cargo deny check` clean                                                 |

### Step 3 — Validate Plan Output

Verify `docs/{ticket_id}/plan.md` contains:

- `## Implementation Plan — {ticket_id}` with task table
- `## Quality Gates per Phase`
- `## Open Decisions` (either empty or listing unresolved items)
- `## Human Approval Required`
- `## Awaiting Developer Confirmation`

If any `DECISION REQUIRED` items remain in `## Open Decisions`, halt:

```
⏸️  DECISIONS REQUIRED before plan can be confirmed
Ticket: {ticket_id}

The following require developer input before implementation can begin:
{list of open decisions}

Answer each in `docs/{ticket_id}/plan.md` under ## Open Decisions, then re-run /plan_feature {ticket_id}.
```

### Step 4 — Mandatory Human Confirmation ⏸️

Present to developer:

```
⏸️  HUMAN CHECKPOINT — plan_feature
Ticket: {ticket_id}

Implementation plan ready: docs/{ticket_id}/plan.md

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
PLAN SUMMARY
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Total tasks:              {count}
Phases used:              {list e.g. "Types(1), Domain(2), Tests(3), Benchmarks(1)"}
Complexity breakdown:     S={count}  M={count}  L={count}
Tasks needing approval:   {count} — {list of IDs, or "none"}
Hot path touched:         {Yes → benchmark phase required / No}
AuditEntry format change: {Yes → breaking change / No}
New dependencies:         {list, or "none"}
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Tasks requiring human approval before implementation:
{formatted list from ## Human Approval Required, or "None"}
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Review the full plan at: docs/{ticket_id}/plan.md

To approve and begin implementation:
  PLAN CONFIRMED: {ticket_id}
```

**EXECUTION PAUSES HERE. `/implement_feature` will not run without explicit confirmation.**

### Step 5 — Record Confirmation

When developer sends `PLAN CONFIRMED: {ticket_id}`, append to `docs/{ticket_id}/plan.md`:

```markdown
## Confirmation

Confirmed by developer on {ISO datetime}
Confirmation message: "PLAN CONFIRMED: {ticket_id}"
```

### Step 6 — Completion Report

```
✅ plan_feature complete
Ticket:   {ticket_id}
Tasks:    {count} across {phase_count} phases
Status:   CONFIRMED — ready for implementation
Next step: /implement_feature {ticket_id}
```

---

## HARD RULES

- `/implement_feature` must not be called without `## Confirmation` appended to `plan.md`.
- Lead agent must not self-confirm the plan — explicit developer input required.
- `DECISION REQUIRED` items block confirmation — they must be resolved first.
- If planner outputs `BLOCKED: {reason}`, propagate to developer and halt.
- Phases must be executed in order — no Phase 3 task may be planned before Phase 2 is fully specified.
