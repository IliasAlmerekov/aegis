# P2 Production Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the local verification baseline fully green and make that green status trustworthy by triaging suspicious integration gaps, fixing the small/safe ones, and documenting the rest concretely.

**Architecture:** Treat P2 as a confidence-oriented polish pass. First inspect the known suspicious test surfaces (`watch_mode`, `audit_integrity`, `snapshot_integration`) and capture evidence in a summary artifact. Then make the smallest reliable testability/integration fixes that improve release confidence without changing product semantics, and finally run the full baseline and record command-by-command outcomes.

**Tech Stack:** Rust 2024, Cargo test/bench toolchain, existing integration tests in `tests/`, audit logging in `src/audit/logger.rs`, markdown summary artifacts in `docs/superpowers/`

---

## File Structure

- `tests/watch_mode.rs`
  - Watch-mode E2E confidence checks; currently contains an ignored audit-path test that is the clearest small/safe hardening candidate.
- `tests/audit_integrity.rs`
  - Integrity/rotation confidence checks; likely more of a triage/reference surface than a code-change surface.
- `tests/snapshot_integration.rs`
  - Snapshot/rollback confidence checks; likely source of environment-dependent or platform-sensitive limitations.
- `src/audit/logger.rs`
  - Smallest likely implementation surface for making the ignored watch-mode audit test real by allowing test-only audit-path injection.
- `docs/superpowers/reports/2026-04-12-p2-production-polish-summary.md`
  - New implementation summary artifact for triage evidence, known limitations, and per-command baseline outcomes.

---

## Milestones

1. Capture triage evidence for the three suspicious test areas.
2. Convert at least the clearest small/safe confidence gap into a real test if feasible.
3. Document anything scope-expanding as a known limitation with destination.
4. Run and record the full verification baseline.

---

## Task Graph

- Task 1 (triage summary) comes first and informs all later work.
- Task 2 (watch-mode audit-path confidence fix) depends on Task 1 confirming it is still the best small/safe fix.
- Task 3 (document deferred gaps) depends on Task 1 and any findings from Task 2.
- Task 4 (full baseline) depends on Tasks 1–3 so the final run reflects the polished state.

---

## Task Details

### Task 1: Create the P2 triage summary artifact

**Files:**
- Create: `docs/superpowers/reports/2026-04-12-p2-production-polish-summary.md`
- Review: `tests/watch_mode.rs`
- Review: `tests/audit_integrity.rs`
- Review: `tests/snapshot_integration.rs`

- [ ] **Step 1: Write the summary skeleton**

Create `docs/superpowers/reports/2026-04-12-p2-production-polish-summary.md` with this exact skeleton:

```markdown
# P2 Production Polish Summary

## Triage Evidence

### tests/watch_mode.rs
- File / test:
- Evidence:
- Decision:

### tests/audit_integrity.rs
- File / test:
- Evidence:
- Decision:

### tests/snapshot_integration.rs
- File / test:
- Evidence:
- Decision:

## Known Limitations / Deferred Follow-Ups

- **Issue:**
- **Why deferred:**
- **Why acceptable now:**
- **Next step:**
- **Owner / destination:**

## Baseline Summary

- `rtk cargo fmt --check` —
- `rtk cargo clippy -- -D warnings` —
- `rtk cargo test` —
- `rtk cargo bench --bench scanner_bench` —
- `rtk cargo audit` —
- `rtk cargo deny check` —
```

- [ ] **Step 2: Fill the initial triage evidence before changing code**

Populate the `## Triage Evidence` section with at least these concrete observations:

```markdown
### tests/watch_mode.rs
- File / test: `watch_mode_audit_entry_sets_transport_watch`
- Evidence: test is marked `#[ignore]` and explicitly says it requires `AEGIS_AUDIT_PATH` support in `AuditLogger`
- Decision: Fix now

### tests/audit_integrity.rs
- File / test: entire file
- Evidence: coverage appears active; no ignored tests are present, but this file still needs baseline confirmation rather than immediate code changes
- Decision: Keep under verification, no immediate fix

### tests/snapshot_integration.rs
- File / test: `git_snapshot_and_rollback_work_from_git_worktree`
- Evidence: helper `add_worktree(...)` can cause an early return, which weakens confidence because the test may silently skip itself in some environments
- Decision: Defer unless a truly local/safe fix becomes obvious during implementation
```

- [ ] **Step 3: Save the summary artifact**

No command needed yet beyond writing the file.

- [ ] **Step 4: Commit the triage artifact**

```bash
rtk git add docs/superpowers/reports/2026-04-12-p2-production-polish-summary.md
rtk git commit -m "docs: add p2 polish triage summary"
```

### Task 2: Turn the ignored watch-mode audit test into a real passing test

**Files:**
- Modify: `src/audit/logger.rs`
- Modify: `tests/watch_mode.rs`

- [ ] **Step 1: Write the failing test by un-ignoring the existing watch audit test**

In `tests/watch_mode.rs`, remove this line:

```rust
#[ignore = "requires AEGIS_AUDIT_PATH env var support in AuditLogger"]
```

Leave the test body in place.

- [ ] **Step 2: Run the single test to verify RED**

Run:

```bash
rtk cargo test --test watch_mode watch_mode_audit_entry_sets_transport_watch
```

Expected: FAIL because `AuditLogger` does not yet honor `AEGIS_AUDIT_PATH`.

- [ ] **Step 3: Implement the smallest audit-path override needed for tests**

In `src/audit/logger.rs`, change `default_audit_path()` from:

```rust
fn default_audit_path() -> PathBuf {
    let home = env::var_os("HOME").unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".aegis").join("audit.jsonl")
}
```

to:

```rust
fn default_audit_path() -> PathBuf {
    if let Some(path) = env::var_os("AEGIS_AUDIT_PATH").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }

    let home = env::var_os("HOME").unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".aegis").join("audit.jsonl")
}
```

This is the whole intended behavior change for this task: test/runtime infrastructure only, no product-policy semantics change.

- [ ] **Step 4: Re-run the single watch-mode test to verify GREEN**

Run:

```bash
rtk cargo test --test watch_mode watch_mode_audit_entry_sets_transport_watch
```

Expected: PASS.

- [ ] **Step 5: Update the triage summary with the completed fix**

Edit `docs/superpowers/reports/2026-04-12-p2-production-polish-summary.md` so the `tests/watch_mode.rs` entry becomes:

```markdown
### tests/watch_mode.rs
- File / test: `watch_mode_audit_entry_sets_transport_watch`
- Evidence: test was previously ignored because `AuditLogger` lacked `AEGIS_AUDIT_PATH` support
- Decision: Fix now
- Result: fixed by honoring `AEGIS_AUDIT_PATH` in `src/audit/logger.rs` and re-enabling the test
```

- [ ] **Step 6: Commit**

```bash
rtk git add src/audit/logger.rs tests/watch_mode.rs docs/superpowers/reports/2026-04-12-p2-production-polish-summary.md
rtk git commit -m "test: re-enable watch audit integration coverage"
```

### Task 3: Record the deferred snapshot/worktree limitation concretely

**Files:**
- Modify: `docs/superpowers/reports/2026-04-12-p2-production-polish-summary.md`

- [ ] **Step 1: Add the concrete deferred-item entry**

Under `## Known Limitations / Deferred Follow-Ups`, add this concrete entry:

```markdown
- **Issue:** `tests/snapshot_integration.rs::git_snapshot_and_rollback_work_from_git_worktree` can return early when `git worktree add` is unavailable or unsupported in the test environment, which weakens confidence in that specific scenario.
- **Why deferred:** making this fully deterministic may require broader environment/test-fixture decisions rather than a local confidence fix.
- **Why acceptable now:** this affects one environment-sensitive integration scenario only; core snapshot, rollback, and audit confidence still come from the active snapshot integration coverage plus the full baseline.
- **Next step:** decide whether to redesign the test fixture to make worktree support deterministic or move this case into an explicitly environment-gated integration suite.
- **Owner / destination:** follow-up phase / backlog item for snapshot-integration hardening.
```

- [ ] **Step 2: Tighten the triage evidence entry for `tests/snapshot_integration.rs`**

Make the triage section say:

```markdown
### tests/snapshot_integration.rs
- File / test: `git_snapshot_and_rollback_work_from_git_worktree`
- Evidence: helper `add_worktree(...)` can cause an early return, so the test may not exercise the intended path in every environment
- Decision: Defer
```

- [ ] **Step 3: Commit**

```bash
rtk git add docs/superpowers/reports/2026-04-12-p2-production-polish-summary.md
rtk git commit -m "docs: record deferred snapshot integration limitation"
```

### Task 4: Run the full baseline and record exact outcomes

**Files:**
- Modify: `docs/superpowers/reports/2026-04-12-p2-production-polish-summary.md`

- [ ] **Step 1: Run formatting verification**

Run:

```bash
rtk cargo fmt --check
```

Expected: PASS with no output or rustfmt diffs.

- [ ] **Step 2: Record the fmt outcome**

Update the summary line to:

```markdown
- `rtk cargo fmt --check` — PASS
```

- [ ] **Step 3: Run lint verification**

Run:

```bash
rtk cargo clippy -- -D warnings
```

Expected: PASS with zero warnings promoted to errors.

- [ ] **Step 4: Record the clippy outcome**

Update the summary line to:

```markdown
- `rtk cargo clippy -- -D warnings` — PASS
```

- [ ] **Step 5: Run the full test suite**

Run:

```bash
rtk cargo test
```

Expected: PASS, including the newly re-enabled watch audit test.

- [ ] **Step 6: Record the test outcome**

Update the summary line to:

```markdown
- `rtk cargo test` — PASS; includes watch-mode audit entry coverage
```

- [ ] **Step 7: Run the benchmark verification**

Run:

```bash
rtk cargo bench --bench scanner_bench
```

Expected: PASS with Criterion output generated successfully.

- [ ] **Step 8: Record the bench outcome**

Update the summary line to:

```markdown
- `rtk cargo bench --bench scanner_bench` — PASS; benchmark completed successfully
```

- [ ] **Step 9: Run security audit verification**

Run:

```bash
rtk cargo audit
```

Expected: PASS with no release-blocking advisories.

- [ ] **Step 10: Record the audit outcome**

Update the summary line to:

```markdown
- `rtk cargo audit` — PASS
```

- [ ] **Step 11: Run dependency-policy verification**

Run:

```bash
rtk cargo deny check
```

Expected: PASS with dependency/license policy satisfied.

- [ ] **Step 12: Record the deny outcome**

Update the summary line to:

```markdown
- `rtk cargo deny check` — PASS
```

- [ ] **Step 13: Commit the final P2 summary**

```bash
rtk git add docs/superpowers/reports/2026-04-12-p2-production-polish-summary.md
rtk git commit -m "docs: record p2 production polish verification"
```

---

## Verification Plan

The verification commands are part of Task 4 and are the primary exit criteria:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
rtk cargo bench --bench scanner_bench
rtk cargo audit
rtk cargo deny check
```

Additionally, the focused confidence test for the watch-mode audit path must pass before the full baseline:

```bash
rtk cargo test --test watch_mode watch_mode_audit_entry_sets_transport_watch
```

---

## Rollback Plan

If the `AEGIS_AUDIT_PATH` override causes unexpected regressions:

1. revert the `default_audit_path()` change in `src/audit/logger.rs`
2. re-apply the `#[ignore]` marker in `tests/watch_mode.rs`
3. keep the deferred-item documentation in the summary artifact

This restores the previous behavior without affecting core runtime policy.

---

## Confirmation

This plan intentionally does **not** turn P2 into a broad hardening phase.

It does:

- triage the suspicious surfaces explicitly
- fix the clearest small/safe confidence gap now
- document a scope-expanding snapshot/worktree gap concretely
- run and record the full baseline in a trustworthy way

