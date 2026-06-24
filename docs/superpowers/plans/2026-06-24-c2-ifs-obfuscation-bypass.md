# C2 `$IFS` Obfuscation Bypass Implementation Plan

> **For agentic workers:** REQUIRED ENTRY POINT: use `/implement C2 $IFS obfuscation bypass` or follow the same red → green → review loop manually. Keep each checkbox small and independently verifiable.

**Goal:** Close `TASKS.md` C2 so literal unquoted `$IFS` and `${IFS}` shell word-splitting forms cannot hide destructive commands from Aegis classification.

**Task source:** `TASKS.md` → `P0 — Critical release blockers` → `C2 — $IFS obfuscation bypasses most patterns`.

**Current release impact:** This is a P0 1.0 release blocker. Do not mark the 1.0 security false-negative gate complete until this and the other Sprint 1 bypass tasks are closed.

---

## 0. Problem summary

`TASKS.md` reports these currently classify as `Safe`:

```text
rm$IFS-rf$IFS/
rm${IFS}-rf${IFS}/
dd${IFS}of=/dev/sda
```

In a POSIX-like shell, unquoted `$IFS` / `${IFS}` expands to shell separator
characters and can cause word splitting. Aegis currently tokenizes these forms as
single words, so the scanner does not see the normalized destructive command shape
that built-in patterns expect.

The fix is **not** full shell expansion. The fix is a narrow, deterministic,
fail-closed normalization for literal IFS expansion forms that are known shell
separators when unquoted.

---

## 1. Security invariant

After this task:

- unquoted literal `$IFS` and `${IFS}` must be treated like shell separators for
  scanning and parser normalization;
- quoted literal `$IFS` and `${IFS}` must not be blindly split unless a test and
  design note justify conservative handling;
- unknown variables must remain opaque for now;
- scanner errors or uncertainty must not turn into `Safe`;
- existing safe commands must remain safe;
- scanner/parser hot path must stay synchronous and low-allocation.

---

## 2. Relevant current code

Read these files before implementation:

- `crates/aegis-parser/src/tokenizer.rs`
  - `split_tokens`
  - current quote / escape / separator behavior
- `crates/aegis-parser/src/lib.rs`
  - `Parser::parse`
  - `normalized = tokens.join(" ")`
- `crates/aegis-parser/src/tests/tokenizer_tests.rs`
  - token-level test style
- `crates/aegis-scanner/src/scanner/tests/basic.rs`
  - direct risk classification tests
- `crates/aegis-scanner/src/scanner/tests/edge_cases.rs`
  - edge-case and perf-style tests
- `crates/aegis-scanner/src/scanner/recursive.rs`
  - nested command / heredoc / process substitution recursion surface
- `crates/aegis-parser/src/embedded_scripts.rs`
  - process substitution, eval payloads, heredoc extraction
- `TASKS.md`
  - C2 acceptance context
- `PROJECT_STATE.md`
  - current blocker list

Do not modify dependency manifests, lockfiles, or CI files for this task.

---

## 3. Design decision

### Recommended design: tokenizer-level literal IFS separators

Handle `$IFS` and `${IFS}` inside `split_tokens` when not inside single quotes and
not escaped. Treat each literal occurrence like whitespace:

- flush current token if non-empty;
- consume the full marker;
- do not emit an `$IFS` token;
- continue scanning.

Why tokenizer-level:

- `Parser::parse` and scanner normalization already use `split_tokens`.
- Nested shell payloads that are parsed recursively also reuse parser/tokenizer
  behavior.
- It avoids duplicating normalization in the scanner.
- It keeps the fix narrow and easy to test.

### Preserve shell quoting semantics

Initial recommended rule:

- split only when not in single quotes and not in double quotes;
- do not split escaped `\$IFS`;
- do not split `'$IFS'`;
- do not split `"$IFS"` in iteration 1 unless explicit regression evidence shows
  that Aegis should conservatively flag it.

Rationale:

- Unquoted IFS is the confirmed bypass from `TASKS.md`.
- Quoted expansions do not participate in normal word splitting the same way.
- Over-splitting quoted strings could create false positives and surprise users.

### Rejected design: full variable expansion

Do not implement full shell variable expansion. It is too broad and risky for this
P0 fix. This task only handles the deterministic literal IFS separators.

### Rejected design: regex-only patch

Do not add special `$IFS` alternatives to every destructive regex. That would be
incomplete, repetitive, and easy to miss for future patterns.

---

## 4. Iteration plan

Each iteration should be small enough to review independently. Prefer one
behavioral change per iteration.

---

## Iteration 1 — Baseline reproduction tests at tokenizer level

**Objective:** Prove the tokenizer currently fails to split unquoted `$IFS` forms.

**Files:**

- Modify: `crates/aegis-parser/src/tests/tokenizer_tests.rs`

**Steps:**

- [ ] Add a focused test module near the existing tokenizer tests:

  ```rust
  mod ifs_obfuscation {
      use super::*;

      #[test]
      fn split_tokens_treats_unquoted_dollar_ifs_as_separator() {
          assert_eq!(split_tokens("rm$IFS-rf$IFS/"), vec!["rm", "-rf", "/"]);
      }

      #[test]
      fn split_tokens_treats_unquoted_braced_ifs_as_separator() {
          assert_eq!(
              split_tokens("rm${IFS}-rf${IFS}/"),
              vec!["rm", "-rf", "/"]
          );
      }

      #[test]
      fn split_tokens_treats_ifs_between_program_and_argument_as_separator() {
          assert_eq!(split_tokens("dd${IFS}of=/dev/sda"), vec!["dd", "of=/dev/sda"]);
      }
  }
  ```

- [ ] Run targeted parser tests and confirm red:

  ```bash
  rtk cargo test -p aegis-parser ifs_obfuscation
  ```

**Expected result before implementation:** tests fail because the tokenizer returns
single combined tokens.

**Rust best-practice notes:**

- Keep assertions direct and behavior-specific.
- Test names must read like sentences.
- Do not add production code in this iteration.

---

## Iteration 2 — Add scanner false-negative regression tests

**Objective:** Capture the security-level failure, not just tokenizer shape.

**Files:**

- Modify: `crates/aegis-scanner/src/scanner/tests/basic.rs` or
  `crates/aegis-scanner/src/scanner/tests/edge_cases.rs`

**Recommended location:** `edge_cases.rs`, because this is an obfuscation edge case.

**Steps:**

- [ ] Add a helper if one does not already exist locally:

  ```rust
  fn assert_command_matches_pattern(cmd: &str, expected_risk: RiskLevel, expected_id: &str) {
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
          "command {cmd:?}: expected pattern {expected_id}, matched {:?}",
          assessment
              .matched
              .iter()
              .map(|m| m.pattern.id.as_ref())
              .collect::<Vec<_>>()
      );
  }
  ```

- [ ] Add red tests for the exact C2 examples:

  ```rust
  #[test]
  fn scanner_blocks_rm_rf_root_obfuscated_with_dollar_ifs() {
      assert_command_matches_pattern("rm$IFS-rf$IFS/", RiskLevel::Block, "PS-006");
  }

  #[test]
  fn scanner_blocks_rm_rf_root_obfuscated_with_braced_ifs() {
      assert_command_matches_pattern("rm${IFS}-rf${IFS}/", RiskLevel::Block, "PS-006");
  }

  #[test]
  fn scanner_flags_dd_block_device_obfuscated_with_braced_ifs() {
      assert_command_matches_pattern("dd${IFS}of=/dev/sda", RiskLevel::Danger, "FS-003");
  }
  ```

- [ ] Run targeted scanner tests and confirm red:

  ```bash
  rtk cargo test -p aegis-scanner ifs
  ```

**Expected result before implementation:** scanner tests fail with `RiskLevel::Safe`.

---

## Iteration 3 — Implement a narrow tokenizer helper

**Objective:** Add small, readable logic to recognize literal IFS markers in
unquoted positions.

**Files:**

- Modify: `crates/aegis-parser/src/tokenizer.rs`

**Implementation shape:**

- [ ] Add a private helper:

  ```rust
  fn consume_ifs_marker(chars: &[char], index: usize) -> Option<usize> {
      // Return marker length in chars: 4 for "$IFS", 6 for "${IFS}".
  }
  ```

  Or, if avoiding `Vec<char>` allocation in the hot tokenizer is preferred, use
  the existing `Peekable<Chars>` flow with a small state machine. Favor minimal
  allocations.

- [ ] If using a `Vec<char>`, measure the trade-off before keeping it. The current
  tokenizer is iterator-based and allocation-light. A helper that peeks through
  the iterator without collecting is preferred.

- [ ] Recommended iterator approach:

  - when current char is `$`;
  - only if `!in_single_quote && !in_double_quote`;
  - inspect upcoming chars for either:
    - `I`, `F`, `S`
    - `{`, `I`, `F`, `S`, `}`
  - consume exactly the marker;
  - flush `current` into `tokens` if non-empty;
  - continue.

**Behavior details:**

- `$IFS` at beginning:
  - no empty token emitted.
- `$IFS` at end:
  - flush token before marker; no trailing empty token.
- repeated `$IFS$IFS`:
  - no empty tokens.
- escaped `\$IFS`:
  - current backslash branch should keep it literal.
- single-quoted `'$IFS'`:
  - remains literal `"$IFS"` content without splitting.
- double-quoted `"$IFS"`:
  - remains literal content in iteration 1.

**Rust best-practice notes:**

- Keep helper private.
- Avoid cloning unless pushing a completed token.
- Avoid panics and index assumptions.
- Prefer descriptive branch names over comments.

**Verification:**

```bash
rtk cargo test -p aegis-parser ifs_obfuscation
```

---

## Iteration 4 — Add tokenizer negative tests

**Objective:** Prevent over-broad `$IFS` handling.

**Files:**

- Modify: `crates/aegis-parser/src/tests/tokenizer_tests.rs`

**Steps:**

- [ ] Add quoted / escaped negative tests:

  ```rust
  #[test]
  fn split_tokens_does_not_split_escaped_dollar_ifs() {
      assert_eq!(split_tokens(r"echo \$IFS"), vec!["echo", "$IFS"]);
  }

  #[test]
  fn split_tokens_does_not_split_single_quoted_dollar_ifs() {
      assert_eq!(split_tokens("echo '$IFS'"), vec!["echo", "$IFS"]);
  }

  #[test]
  fn split_tokens_does_not_split_double_quoted_dollar_ifs() {
      assert_eq!(split_tokens("echo \"$IFS\""), vec!["echo", "$IFS"]);
  }

  #[test]
  fn split_tokens_does_not_split_non_ifs_variables() {
      assert_eq!(split_tokens("echo$PATH/test"), vec!["echo$PATH/test"]);
  }
  ```

- [ ] Add boundary tests:

  ```rust
  #[test]
  fn split_tokens_ignores_partial_ifs_prefixes() {
      assert_eq!(split_tokens("echo$IF"), vec!["echo$IF"]);
  }

  #[test]
  fn split_tokens_ignores_partial_braced_ifs() {
      assert_eq!(split_tokens("echo${IFS"), vec!["echo${IFS"]);
  }
  ```

- [ ] Run:

  ```bash
  rtk cargo test -p aegis-parser ifs_obfuscation
  ```

**Acceptance:** positive and negative tokenizer tests pass.

---

## Iteration 5 — Confirm parser normalized form

**Objective:** Ensure `Parser::parse` produces the scanner-facing normalized
shape expected by patterns.

**Files:**

- Modify: `crates/aegis-parser/src/tests/parsing_tests.rs` or
  `tokenizer_tests.rs` if parser tests already live there.

**Steps:**

- [ ] Add parser tests:

  ```rust
  #[test]
  fn parse_normalizes_dollar_ifs_as_space_between_rm_arguments() {
      let parsed = Parser::parse("rm$IFS-rf$IFS/");

      assert_eq!(parsed.program.as_deref(), Some("rm"));
      assert_eq!(parsed.argv, vec!["-rf", "/"]);
      assert_eq!(parsed.normalized, "rm -rf /");
  }

  #[test]
  fn parse_normalizes_braced_ifs_as_space_between_rm_arguments() {
      let parsed = Parser::parse("rm${IFS}-rf${IFS}/");

      assert_eq!(parsed.program.as_deref(), Some("rm"));
      assert_eq!(parsed.argv, vec!["-rf", "/"]);
      assert_eq!(parsed.normalized, "rm -rf /");
  }
  ```

- [ ] Run:

  ```bash
  rtk cargo test -p aegis-parser parse_normalizes
  ```

**Acceptance:** parser program, argv, and normalized command reflect separator
semantics.

---

## Iteration 6 — Re-run scanner tests and adjust evidence expectations

**Objective:** Make sure tokenizer normalization actually closes the scanner
false negative.

**Files:**

- Modify only tests if expected pattern IDs differ from the initial assumption.

**Steps:**

- [ ] Run:

  ```bash
  rtk cargo test -p aegis-scanner ifs
  ```

- [ ] If tests fail only because the matched pattern ID differs:
  - inspect the matched IDs from assertion output;
  - choose the most semantically direct destructive pattern;
  - keep exact risk expectation.

- [ ] If tests still return `Safe`:
  - inspect `Parser::parse(cmd).normalized`;
  - inspect `quick_scan` keyword coverage;
  - inspect whether pattern indexing keys match the normalized program;
  - do not paper over with a special-case scanner rule until tokenizer behavior is
    correct.

**Acceptance:** C2 exact examples no longer classify as `Safe`.

---

## Iteration 7 — Nested shell coverage

**Objective:** Prove recursive scanner paths inherit the fix.

**Files:**

- Modify: `crates/aegis-scanner/src/scanner/tests/edge_cases.rs`
- Optionally modify parser nested-shell tests if useful.

**Steps:**

- [ ] Add nested `bash -c` regression:

  ```rust
  #[test]
  fn scanner_blocks_ifs_obfuscation_inside_bash_c() {
      assert_command_matches_pattern(
          "bash -c 'rm$IFS-rf$IFS/'",
          RiskLevel::Block,
          "PS-006",
      );
  }
  ```

- [ ] Add nested `sh -c` regression:

  ```rust
  #[test]
  fn scanner_blocks_ifs_obfuscation_inside_sh_c() {
      assert_command_matches_pattern(
          "sh -c 'rm${IFS}-rf${IFS}/'",
          RiskLevel::Block,
          "PS-006",
      );
  }
  ```

- [ ] Run:

  ```bash
  rtk cargo test -p aegis-scanner bash_c ifs
  ```

**Acceptance:** recursive shell scanning catches literal IFS separators inside
quoted shell payloads passed to shell interpreters. The outer single quotes are
not the same as shell-runtime quotes inside the nested payload; after extraction,
the nested payload should be parsed as shell code.

---

## Iteration 8 — Heredoc coverage

**Objective:** Ensure heredoc body scanning benefits from the tokenizer fix when
the heredoc is a normal expanding heredoc.

**Files:**

- Modify: `crates/aegis-scanner/src/scanner/tests/edge_cases.rs`

**Steps:**

- [ ] Add a normal heredoc test:

  ```rust
  #[test]
  fn scanner_blocks_ifs_obfuscation_inside_expanding_heredoc() {
      let cmd = "bash <<EOF\nrm$IFS-rf$IFS/\nEOF";
      assert_command_matches_pattern(cmd, RiskLevel::Block, "PS-006");
  }
  ```

- [ ] Add nowdoc behavior test only if current heredoc extraction marks
  `is_nowdoc` and scanner respects it. If scanner currently scans both heredoc and
  nowdoc bodies conservatively, document that in the test name instead of changing
  behavior in this task.

- [ ] Run:

  ```bash
  rtk cargo test -p aegis-scanner heredoc ifs
  ```

**Acceptance:** confirmed coverage for the heredoc composition cited in
`TASKS.md`, or a documented follow-up if heredoc semantics need a larger design.

---

## Iteration 9 — Process substitution coverage

**Objective:** Ensure process substitution payloads are recursively scanned.

**Files:**

- Modify: `crates/aegis-scanner/src/scanner/tests/edge_cases.rs`

**Steps:**

- [ ] Add process substitution regression:

  ```rust
  #[test]
  fn scanner_blocks_ifs_obfuscation_inside_process_substitution() {
      assert_command_matches_pattern(
          "cat <(rm$IFS-rf$IFS/)",
          RiskLevel::Block,
          "PS-006",
      );
  }
  ```

- [ ] Run:

  ```bash
  rtk cargo test -p aegis-scanner process_substitution ifs
  ```

**Acceptance:** process substitution composition from `TASKS.md` is covered.

---

## Iteration 10 — More destructive-pattern coverage

**Objective:** Avoid a narrow `rm`-only fix.

**Files:**

- Modify: `crates/aegis-scanner/src/scanner/tests/edge_cases.rs`

**Steps:**

- [ ] Add cases for multiple pattern families:

  ```rust
  #[test]
  fn scanner_flags_ifs_obfuscated_find_delete() {
      assert_command_matches_pattern("find$IFS/$IFS-delete", RiskLevel::Danger, "FS-002");
  }

  #[test]
  fn scanner_flags_ifs_obfuscated_shred() {
      assert_command_matches_pattern("shred${IFS}-u${IFS}secrets.txt", RiskLevel::Danger, "FS-004");
  }

  #[test]
  fn scanner_blocks_ifs_obfuscated_mkfs() {
      assert_command_matches_pattern("mkfs.ext4${IFS}/dev/sdb1", RiskLevel::Block, "FS-006");
  }
  ```

- [ ] Run:

  ```bash
  rtk cargo test -p aegis-scanner ifs_obfuscated
  ```

**Acceptance:** at least three destructive pattern families are protected.

---

## Iteration 11 — Performance check before broad verification

**Objective:** Ensure tokenizer change did not meaningfully slow the hot path.

**Files:**

- Prefer no code changes.

**Steps:**

- [ ] Run existing quick perf-style test:

  ```bash
  rtk cargo test -p aegis-scanner ten_thousand_safe_commands_under_25ms
  ```

- [ ] If this fails:
  - inspect tokenizer helper for unnecessary allocation;
  - avoid collecting all chars;
  - avoid repeatedly cloning partial tokens;
  - keep matching `$IFS` as a simple branch off `$`.

- [ ] If parser/scanner hot path was materially changed, run:

  ```bash
  rtk cargo bench --bench scanner_bench
  ```

**Acceptance:** existing perf-style test remains green. Benchmark output, if run,
must be recorded in the implementation summary.

---

## Iteration 12 — Full local verification

**Objective:** Confirm implementation is clean before docs/status updates.

**Steps:**

- [ ] Run parser package tests:

  ```bash
  rtk cargo test -p aegis-parser
  ```

- [ ] Run scanner package tests:

  ```bash
  rtk cargo test -p aegis-scanner
  ```

- [ ] Run full tests:

  ```bash
  rtk cargo test
  ```

- [ ] Run formatting:

  ```bash
  rtk cargo fmt --check
  ```

- [ ] Run clippy:

  ```bash
  rtk cargo clippy -- -D warnings
  ```

**Acceptance:** all commands pass. If any command fails due to pre-existing
unrelated state, record exact failure and do not mark C2 complete.

---

## Iteration 13 — Update project tracking after verified green

Only do this after Iteration 12 is green.

**Files:**

- Modify: `TASKS.md`
- Modify: `PROJECT_STATE.md`
- Modify: `CHANGELOG.md`

**Steps:**

- [ ] In `TASKS.md`, mark C2 as done:

  ```text
  ### [x] C2 — `$IFS` obfuscation bypasses most patterns
  ```

- [ ] Add a short `Resolution:` bullet under C2:

  ```text
  - **Resolution:** unquoted literal `$IFS` / `${IFS}` are normalized as shell
    separators during parsing, with direct, nested shell, heredoc, and process
    substitution regressions.
  ```

- [ ] In Sprint 1, mark C2 checkbox done.

- [ ] In `PROJECT_STATE.md`:
  - update `Last updated` to the current date;
  - replace current-session summary with the C2 fix;
  - remove C2 from open P0 blockers, leaving C3 if still open.

- [ ] In `CHANGELOG.md`, under `[Unreleased]` → `Fixed`, add:

  ```text
  - Closed the C2 `$IFS` command-obfuscation bypass by normalizing unquoted literal
    `$IFS` / `${IFS}` as shell separators during parsing.
  ```

**Acceptance:** tracking files match verified behavior.

---

## Iteration 14 — Final security review checklist

Before calling the task done, answer these explicitly in the final implementation
summary:

- [ ] Does `rm$IFS-rf$IFS/` classify at least as dangerous as `rm -rf /`?
- [ ] Does `rm${IFS}-rf${IFS}/` classify at least as dangerous as `rm -rf /`?
- [ ] Does `dd${IFS}of=/dev/sda` classify as `Danger`?
- [ ] Does `bash -c 'rm$IFS-rf$IFS/'` classify as `Block`?
- [ ] Does heredoc/process substitution coverage pass?
- [ ] Are escaped and quoted `$IFS` forms tested?
- [ ] Did we avoid full variable expansion?
- [ ] Did we preserve custom/user regex semantics?
- [ ] Did all required gates pass?

---

## 5. Implementation notes and gotchas

### Tokenizer quote context matters

The outer command:

```text
bash -c 'rm$IFS-rf$IFS/'
```

contains single quotes in the outer shell command, but the nested payload extracted
from `bash -c` is:

```text
rm$IFS-rf$IFS/
```

That payload is shell code and should be scanned with IFS separator semantics.

### Do not confuse `$IFS` separator normalization with arbitrary variables

These should not be split in this task:

```text
echo$PATH/test
rm$MAYBE_SPACE-rf/
```

Unknown variables are broader shell-analysis work and should remain a separate
security design.

### Avoid unsafe false confidence

If some composed path still returns `Safe`, do not mark C2 complete. Either:

- fix the recursive path in this task, if small; or
- split out a named follow-up and keep C2 open if it is part of the reported C2
  evidence.

### Do not add dependencies

Everything needed is in std and existing crates.

### Avoid `.unwrap()` / `.expect()` in production

Test code may keep existing test style with `expect`, but production tokenizer
changes must avoid panics.

---

## 6. Suggested commit structure

If committing, keep commits small:

1. `test(scanner): cover IFS obfuscation bypass`
2. `fix(parser): normalize literal IFS separators`
3. `test(scanner): cover nested IFS obfuscation`
4. `docs(tasks): mark C2 bypass closed`

No `Co-Authored-By` trailers.

---

## 7. Verification command list

Run targeted during development:

```bash
rtk cargo test -p aegis-parser ifs
rtk cargo test -p aegis-scanner ifs
rtk cargo test -p aegis-scanner ten_thousand_safe_commands_under_25ms
```

Run before completion:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
rtk cargo audit
rtk cargo deny check
```

Run benchmark if scanner/parser hot path changes are non-trivial:

```bash
rtk cargo bench --bench scanner_bench
```

---

## 8. Completion criteria

C2 is complete only when:

- all direct C2 examples are no longer `Safe`;
- nested shell, heredoc, and process substitution compositions are covered;
- negative tests show quoted/escaped/non-IFS variables are not over-normalized;
- full verification passes;
- `TASKS.md`, `PROJECT_STATE.md`, and `CHANGELOG.md` are updated after verified
  green.
