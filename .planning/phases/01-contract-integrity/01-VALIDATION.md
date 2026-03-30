---
phase: 01
slug: contract-integrity
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-27
---

# Phase 01 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust `cargo test` integration tests |
| **Config file** | none |
| **Quick run command** | `rtk cargo test --test full_pipeline snapshot_` |
| **Full suite command** | `rtk cargo test` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `rtk cargo test --test full_pipeline snapshot_`
- **After every plan wave:** Run `rtk cargo test --test full_pipeline`
- **Before `$gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 01-01-01 | 01 | 1 | T1.3-GIT-OFF | integration | `rtk cargo test --test full_pipeline snapshot_registry_git_flag_false_skips_plugin_and_audit` | ✅ `tests/full_pipeline.rs` | ⬜ pending |
| 01-01-02 | 01 | 1 | T1.3-DOCKER-OFF | integration | `rtk cargo test --test full_pipeline snapshot_registry_docker_flag_false_skips_plugin_and_audit` | ✅ `tests/full_pipeline.rs` | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `tests/full_pipeline.rs` — add two subprocess regression cases for git-off and docker-off snapshot flags
- [ ] shared inline helper or test-local pattern for asserting stub CLI invocation logs remain empty

---

## Manual-Only Verifications

All phase behaviors have automated verification.

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
