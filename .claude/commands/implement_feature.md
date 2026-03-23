---
name: implement_feature
description: Execute the full implementation loop for a ticket — coder, tester, reviewer, security audit per task, in phase order. Requires confirmed plan.md.
allowed_tools: ["Read", "Write", "Edit", "Bash", "Grep", "Glob", "Agent"]
---

# Command: implement_feature

## PURPOSE

Execute the full implementation loop: coder writes code, tester writes tests,
reviewer checks quality, security agent audits each task.
Tasks run in phase order (Types → Domain → Integration → Tests → Benchmarks → Docs)
with mandatory review gates between each.

## INVOCATION

```
/implement_feature {ticket_id}
```

## PRECONDITIONS

Both must be true before execution begins:

1. `docs/{ticket_id}/plan.md` exists with all required sections
2. `docs/{ticket_id}/plan.md` contains `## Confirmation` section

If either fails:

```
❌ implement_feature aborted
Ticket: {ticket_id}
Reason: {specific precondition that failed}
Fix:    /plan_feature {ticket_id}  then  PLAN CONFIRMED: {ticket_id}
```

---

## LEAD AGENT ORCHESTRATION LOOP

```
LOAD tasks FROM docs/{ticket_id}/plan.md
ORDER tasks BY: phase number ASC, then dependency DAG (depends_on resolved)
SKIP tasks WHERE status == DONE  ← allows safe resume after ESCALATE

FOR EACH task IN ordered_tasks:

  PRINT "▶ Task {task.id} [Phase {task.phase} — {task.phase_name}]: {task.description}"
  PRINT "  Files: create={task.files_to_create} modify={task.files_to_modify}"

  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  STEP 1 — QUALITY GATE: previous phase
  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Before starting Phase N, verify the gate for Phase N-1 passes:

  Phase 1 gate: rtk cargo build
  Phase 2 gate: rtk cargo build && rtk cargo clippy -- -D warnings
  Phase 3 gate: rtk cargo build && rtk cargo clippy -- -D warnings && rtk cargo test
  Phase 4 gate: rtk cargo test  (100% pass, FNR = 0 for any Danger/Block fixtures)
  Phase 5 gate: rtk cargo criterion  (safe-path p99 < 2ms)
  Phase 6 gate: rtk cargo audit && rtk cargo deny check

  IF gate fails → write ESCALATE.md with gate output → HALT

  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  STEP 2 — CODER
  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Assign to coder agent with:
    - task row from plan.md
    - docs/{ticket_id}/research.md  (architecture, conventions card)
    - current content of all files in task.files_to_modify

  Coder must follow these non-negotiable constraints:
    - thiserror for error types in lib modules; anyhow only in main.rs
    - No .unwrap() or .expect() in production paths (tests and startup-init are exceptions)
    - LazyLock<Regex> for compiled regexes — never once_cell
    - &'static str for BuiltinPattern fields; Cow<'static, str> for unified Pattern
    - #[async_trait] on any trait with async fn
    - Aho-Corasick only for the first-pass keyword scan — no regex in AC
    - No new dependencies without explicit approval in plan.md
    - No business logic in main.rs — only wiring

  IF coder output starts with "BLOCKED:":
    → Write ESCALATE.md (see format below)
    → PRINT escalation notice
    → HALT

  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  STEP 3 — TESTER
  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Assign to tester agent with:
    - coder output files
    - docs/{ticket_id}/research.md  (edge cases, integration boundaries)
    - existing tests in the affected modules for style reference

  Tester rules by phase:

  Phase 1–3 (Types / Domain / Integration):
    - Unit tests in #[cfg(test)] block in the same file
    - Test every new public function and every new error variant
    - Edge cases from research.md § Integration Boundaries must be covered

  Phase 4 (Tests):
    - Add fixture cases to tests/fixtures/commands.toml
    - Format: id = "eval-{pattern_id}-{pos|neg}-{N}", command, expected_risk, expected_pattern_id (pos only), note
    - Minimum: 2 positive + 2 negative per new pattern; 4+4 for Danger or Block level
    - Integration tests in tests/integration/ if shell passthrough behavior changed

  Phase 5 (Benchmarks):
    - Add or update bench groups in benches/scanner_bench.rs
    - Separate bench groups: assess/safe, assess/warn, assess/danger
    - Run: rtk cargo criterion
    - FAIL if safe-path p99 >= 2ms

  IF tester output starts with "BLOCKED:":
    → Write ESCALATE.md → HALT

  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  STEP 4 — REVIEWER (max 3 cycles)
  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  SET review_cycle = 1

  WHILE review_cycle <= 3:
    Assign to reviewer agent with:
      - coder + tester output files
      - task acceptance criteria from plan.md

    Reviewer checklist (must pass ALL):
      □ rtk cargo build — clean
      □ rtk cargo clippy -- -D warnings — zero warnings
      □ rtk cargo fmt --check — no formatting drift
      □ rtk cargo test — all tests pass
      □ No .unwrap() / .expect() added outside #[cfg(test)] or startup-init
      □ No once_cell imported or used
      □ No unapproved dependency added to Cargo.toml
      □ No business logic added to main.rs
      □ Regex patterns (if any) compiled via LazyLock, not inline
      □ New BuiltinPattern entries use &'static str fields
      □ Pattern IDs follow format and are not reused retired IDs
      □ AuditEntry changes (if any) flagged in task and approved in plan.md

    IF reviewer output starts with "APPROVED":
      PRINT "  ✓ Reviewer approved (cycle {review_cycle})"
      BREAK

    IF reviewer output starts with "CHANGES_REQUESTED":
      PRINT "  ↺ Changes requested (cycle {review_cycle}) — returning to coder"
      Pass CHANGES_REQUESTED list back to coder agent (same task scope only)
      review_cycle += 1

    IF review_cycle == 3 AND not APPROVED:
      → Write ESCALATE.md with reviewer's unresolved items
      → HALT

  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  STEP 5 — SECURITY AUDIT
  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Run security audit IF:
    - task.needs_human_approval == true  (from plan.md)
    OR task.files_to_modify intersects hot-path files:
        src/interceptor/scanner.rs
        src/interceptor/parser.rs
        src/interceptor/patterns.rs
        src/audit/logger.rs
        src/main.rs

  Assign to security agent with:
    - reviewer-approved files
    - docs/{ticket_id}/research.md § Integration Boundaries

  Security agent checks (Aegis-specific):

    CRITICAL — halt immediately if any found:
      □ ReDoS risk: new regex with catastrophic backtracking potential
         (unbounded quantifiers on overlapping character classes)
      □ Shell injection via command string construction in any code path
      □ Audit log overwrite: any write mode other than append to audit.jsonl
      □ Fail-open change: interceptor now lets Danger/Block commands through on error
         without explicit approval in plan.md
      □ Panic in production path: .unwrap() on a non-infallible operation outside tests

    HIGH — halt immediately if any found:
      □ New Block-level pattern added without plan.md approval
      □ AuditEntry struct or JSONL field renamed/removed without plan.md approval
      □ Config field removed without backwards-compatibility check

    MEDIUM — log to risk_log.md, continue:
      □ False negative risk: new pattern's negative fixture set is thin (< 4 cases)
      □ Regex second-pass filter absent for a broad Aho-Corasick keyword
      □ tracing event missing for a new Danger/Block interception path
      □ Error branch fails closed but reason is not logged

    LOW — log to risk_log.md, continue:
      □ Missing negative fixture cases for a Warn-level pattern
      □ New public function lacks doc comment

  IF security output contains CRITICAL or HIGH:
    → Write ESCALATE.md with full risk report
    → PRINT "  🚨 SECURITY HALT — {severity} finding"
    → HALT IMMEDIATELY

  IF security output contains only MEDIUM or LOW:
    Append finding to docs/{ticket_id}/risk_log.md
    PRINT "  ⚠ Security note logged ({severity}) — continuing"

  IF security output is SECURE:
    PRINT "  ✓ Security audit passed"

  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  STEP 6 — MARK DONE
  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Update plan.md: mark task {task.id} status = DONE
  PRINT "  ✅ Task {task.id} complete"

END LOOP
```

---

## COMPLETION

Write `docs/{ticket_id}/summary.md`:

```markdown
# Implementation Summary — {ticket_id}

**Completed**: {ISO datetime}
**Total tasks**: {done}/{total}
**Files changed**: {count}

## Tasks Completed

| Task ID | Phase        | Description | Files Changed | Tests Added |
| ------- | ------------ | ----------- | ------------- | ----------- |
| {id}    | {phase_name} | {desc}      | {count}       | {count}     |

## Files Changed

**Created:**
{list of new files}

**Modified:**
{list of modified files}

## Security Findings (MEDIUM / LOW only)

{content from risk_log.md, or "None"}

## Open Risks

{any LOW findings the developer should monitor}

## Verification Steps

Run these before opening the PR:

- `rtk cargo build` — must be clean
- `rtk cargo clippy -- -D warnings` — zero warnings
- `rtk cargo fmt --check` — no formatting drift
- `rtk cargo test` — full suite green
- `rtk cargo criterion` — safe-path p99 < 2ms _(skip if no scanner/parser changes)_
- `rtk cargo audit` — zero CVEs
- `rtk cargo deny check` — zero policy violations
- Manual: run Aegis against a shell session with known-dangerous commands and verify
  interception fires correctly for each new/modified pattern
- Review `docs/{ticket_id}/risk_log.md` for any MEDIUM findings requiring follow-up
```

---

## ESCALATION FILE FORMAT

Write to `docs/{ticket_id}/ESCALATE.md`:

```markdown
# ESCALATION — {ticket_id}

**Halted at**: Task {task_id}, Step {quality_gate | coder | tester | reviewer | security}
**Datetime**: {ISO datetime}
**Reason**: {one-line summary}

---

## Details

{full context: gate output / BLOCKED message / reviewer unresolved items / security risk report}

---

## Required Action

{precise question or decision the developer must make — not "needs attention"}

---

## To Resume

After resolving the issue:

1. Address the root cause in the relevant source files
2. Re-run: `/implement_feature {ticket_id}`

Tasks already marked DONE in plan.md will be skipped.
Resume point: Task **{task_id}**, Step **{halted_step}**.
```

---

## COMPLETION OUTPUT

```
🏁 implement_feature complete
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Ticket:        {ticket_id}
Tasks done:    {done}/{total}
Files changed: {count}
Security:      {medium_low_count} MEDIUM/LOW findings logged | 0 HIGH/CRITICAL
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Summary:       docs/{ticket_id}/summary.md
Risk log:      docs/{ticket_id}/risk_log.md  (if any findings)

Next: run /verification_loop or open PR after manual verification steps above.
```

---

## HARD RULES

- Never start the loop without `## Confirmation` in `plan.md`.
- Phase quality gate must pass before the first task of that phase runs.
- Security CRITICAL/HIGH findings halt immediately — no exceptions, no overrides.
- Lead agent must not self-approve reviewer or security steps.
- Tasks marked DONE in `plan.md` are always skipped — idempotent resume.
- `rtk` prefix on all shell commands — never bare `cargo` or `git`.
