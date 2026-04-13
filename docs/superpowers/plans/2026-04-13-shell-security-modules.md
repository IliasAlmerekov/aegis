# Shell Security Modules Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the shell-security hot path into smaller parser and scanner modules while preserving existing policy semantics and allowing only narrow, documented fail-closed or boundary fixes.

**Architecture:** Use a staged, interceptor-first rollout. First convert `src/interceptor/parser.rs` into a stable facade backed by focused parser submodules; then split `src/interceptor/scanner.rs` into focused scanner submodules once parser ownership seams are clear. Keep `RiskLevel`, allowlist precedence, approval semantics, and the quick-scan plus regex second-pass model unchanged.

**Tech Stack:** Rust 2024, existing `src/interceptor/` parser/scanner modules, `aho-corasick`, `regex`, in-file unit tests, `tests/full_pipeline.rs`, `benches/scanner_bench.rs`

---

## File Structure

- `src/interceptor/mod.rs`
  - Keep the public module layout stable while the file-backed `parser.rs` and `scanner.rs` become directory-backed modules.
- `src/interceptor/parser/mod.rs`
  - Own the stable parser facade, public parser types, and public parser entry points.
- `src/interceptor/parser/tokenizer.rs`
  - Own `split_tokens` and token-level shell quoting/escaping rules.
- `src/interceptor/parser/segmentation.rs`
  - Own top-level command and pipeline segmentation helpers.
- `src/interceptor/parser/nested_shells.rs`
  - Own shell `-c` unwrapping and nested shell extraction helpers.
- `src/interceptor/parser/embedded_scripts.rs`
  - Own heredoc, inline script, eval payload, and process substitution extraction.
- `src/interceptor/scanner/mod.rs`
  - Own the stable scanner facade and public scanner types.
- `src/interceptor/scanner/assessment.rs`
  - Own `Scanner::assess`, match merging, and final risk aggregation.
- `src/interceptor/scanner/pipeline_semantics.rs`
  - Own semantic analysis over parser-produced pipeline structure.
- `src/interceptor/scanner/highlighting.rs`
  - Own highlight-range sorting and merging only.
- `src/interceptor/scanner/keywords.rs`
  - Own Aho-Corasick keyword extraction and uncovered-pattern handling.
- `src/interceptor/scanner/recursive.rs`
  - Own recursive target collection glue between parser extraction and nested scanning.
- `benches/scanner_bench.rs`
  - Re-run only if the stage decides hot-path benchmarking is necessary.

## Milestones

1. Split parser behind a stable facade with behavior-preserving tests.
2. Split scanner behind a stable facade with classification-preserving tests.
3. Record stage-by-stage verification and benchmark decisions.

## Task Graph

- Task 1 creates the parser directory facade and must land before any parser helper moves.
- Task 2 and Task 3 depend on Task 1 and complete Stage 1.
- Task 4 creates the scanner directory facade and must land after Stage 1 is green.
- Task 5 and Task 6 depend on Task 4 and complete Stage 2.
- Task 7 runs final verification and records benchmark decisions after both stages are complete.

## Task Details

### Task 1: Create the parser facade and move tokenizer ownership

**Files:**
- Create: `src/interceptor/parser/mod.rs`
- Create: `src/interceptor/parser/tokenizer.rs`
- Modify: `src/interceptor/mod.rs`
- Modify: `src/interceptor/parser.rs` (temporary source of moved code before deletion)

- [ ] **Step 1: Add parser facade tests that lock the public entry points**

Add focused tests at the bottom of the future `src/interceptor/parser/mod.rs` test module to preserve the public parser contract:

```rust
    #[test]
    fn split_tokens_preserves_separator_tokens() {
        assert_eq!(
            split_tokens("echo hi && rm -rf /tmp/demo | cat"),
            vec!["echo", "hi", "&&", "rm", "-rf", "/tmp/demo", "|", "cat"]
        );
    }

    #[test]
    fn parse_preserves_first_command_shape() {
        let parsed = Parser::parse("FOO=bar bash -c 'echo hi'");
        assert_eq!(parsed.executable.as_deref(), Some("FOO=bar"));
        assert_eq!(parsed.raw, "FOO=bar bash -c 'echo hi'");
    }
```

- [ ] **Step 2: Run the parser contract tests to verify RED while the new module layout does not exist yet**

Run:

```bash
rtk cargo test split_tokens_preserves_separator_tokens parse_preserves_first_command_shape --lib
```

Expected: compile failure or missing-symbol failure until the facade is created and the tests are moved.

- [ ] **Step 3: Create `src/interceptor/parser/mod.rs` and `tokenizer.rs` with a stable facade**

Move the public parser types and `Parser::parse` into `src/interceptor/parser/mod.rs`, and move `split_tokens` into `src/interceptor/parser/tokenizer.rs`.

Start `src/interceptor/parser/mod.rs` with:

```rust
mod tokenizer;
mod segmentation;
mod nested_shells;
mod embedded_scripts;

use std::fmt;

pub use tokenizer::split_tokens;
pub use segmentation::{logical_segments, top_level_pipelines, PipelineChain, PipelineSegment};
pub use nested_shells::extract_nested_commands;
pub use embedded_scripts::{
    extract_eval_payloads, extract_heredoc_bodies, extract_inline_scripts,
    extract_process_substitution_bodies, HeredocBody, InlineScript,
};
```

And in `src/interceptor/parser/tokenizer.rs` start with:

```rust
pub fn split_tokens(cmd: &str) -> Vec<String> {
    // moved from parser.rs without semantic change
}
```

Keep `pub struct ParsedCommand` and `pub struct Parser` in `mod.rs`, with `Parser::parse(...)` still calling `split_tokens(cmd)` and `extract_inline_scripts(cmd)`.

- [ ] **Step 4: Point `src/interceptor/mod.rs` at the directory-backed parser module**

Keep the external declaration unchanged:

```rust
pub mod parser;
```

Then delete the old file-backed `src/interceptor/parser.rs` only after all moved parser code has been copied into the new directory-backed module tree.

- [ ] **Step 5: Run the focused parser contract tests to verify GREEN**

Run:

```bash
rtk cargo test split_tokens_preserves_separator_tokens parse_preserves_first_command_shape --lib
```

Expected: both tests pass and `crate::interceptor::parser::*` call sites still compile.

- [ ] **Step 6: Commit**

```bash
rtk git add src/interceptor/mod.rs src/interceptor/parser
rtk git commit -m "refactor: create parser facade"
```

### Task 2: Move parser segmentation and nested shell extraction behind the facade

**Files:**
- Modify: `src/interceptor/parser/mod.rs`
- Create: `src/interceptor/parser/segmentation.rs`
- Create: `src/interceptor/parser/nested_shells.rs`

- [ ] **Step 1: Add failing tests for segmentation and nested shell behavior preservation**

Add tests to `src/interceptor/parser/mod.rs`:

```rust
    #[test]
    fn top_level_pipelines_preserve_adjacent_pipeline_stages() {
        let pipelines = top_level_pipelines("printf x | xargs rm -f | cat");
        assert_eq!(pipelines.len(), 1);
        assert_eq!(pipelines[0].segments.len(), 3);
        assert_eq!(pipelines[0].segments[1].normalized, "xargs rm -f");
    }

    #[test]
    fn extract_nested_commands_unwraps_env_prefixed_shell_c() {
        assert_eq!(
            extract_nested_commands("env FOO=bar bash -lc 'echo one && echo two'"),
            vec!["echo one", "echo two"]
        );
    }
```

- [ ] **Step 2: Run the focused parser tests to verify RED if the moves are incomplete**

Run:

```bash
rtk cargo test top_level_pipelines_preserve_adjacent_pipeline_stages extract_nested_commands_unwraps_env_prefixed_shell_c --lib
```

Expected: at least one test fails until the moved helpers are exported from the facade.

- [ ] **Step 3: Move segmentation helpers into `segmentation.rs` without semantic change**

Move the existing helpers as-is:

```rust
pub fn logical_segments(cmd: &str) -> Vec<String> { /* moved */ }
pub fn top_level_pipelines(cmd: &str) -> Vec<PipelineChain> { /* moved */ }

fn split_top_level_segments(cmd: &str) -> Vec<String> { /* moved */ }
fn split_top_level_command_groups(cmd: &str) -> Vec<String> { /* moved */ }
fn split_pipeline_segments(raw_group: &str) -> Vec<PipelineSegment> { /* moved */ }
fn normalize_segment(raw_segment: &str) -> Option<String> { /* moved */ }
fn finalize_segment(current: &mut String, segments: &mut Vec<String>) { /* moved */ }
```

Keep scanner-facing function names unchanged.

- [ ] **Step 4: Move shell `-c` extraction into `nested_shells.rs` and keep recursive behavior stable**

Move the existing nested-shell helpers as-is:

```rust
pub fn extract_nested_commands(cmd: &str) -> Vec<String> { /* moved */ }
fn try_unwrap_shell_tokens(tokens: &[String]) -> Option<Vec<String>> { /* moved */ }
fn split_by_separators(tokens: Vec<String>) -> Vec<Vec<String>> { /* moved */ }
fn unescape_ansi_c(s: &str) -> String { /* moved */ }
```

Update imports so nested-shell helpers use `super::split_tokens` rather than duplicating tokenization logic.

- [ ] **Step 5: Re-run the focused parser tests to verify GREEN**

Run:

```bash
rtk cargo test top_level_pipelines_preserve_adjacent_pipeline_stages extract_nested_commands_unwraps_env_prefixed_shell_c --lib
```

Expected: both tests pass with the new module layout.

- [ ] **Step 6: Commit**

```bash
rtk git add src/interceptor/parser
rtk git commit -m "refactor: split parser segmentation"
```

### Task 3: Move embedded script extraction and finish Stage 1 verification

**Files:**
- Modify: `src/interceptor/parser/mod.rs`
- Create: `src/interceptor/parser/embedded_scripts.rs`
- Modify: `src/interceptor/parser` tests

- [ ] **Step 1: Add failing tests for embedded command body preservation**

Add tests to `src/interceptor/parser/mod.rs`:

```rust
    #[test]
    fn extract_inline_scripts_preserves_python_c_payload() {
        let scripts = extract_inline_scripts("python -c 'print(1)'");
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].interpreter, "python");
        assert_eq!(scripts[0].body, "print(1)");
    }

    #[test]
    fn extract_process_substitution_bodies_preserves_nested_command() {
        assert_eq!(
            extract_process_substitution_bodies("diff <(git status) <(git diff)"),
            vec!["git status", "git diff"]
        );
    }
```

- [ ] **Step 2: Run the focused embedded-script tests to verify RED while the move is incomplete**

Run:

```bash
rtk cargo test extract_inline_scripts_preserves_python_c_payload extract_process_substitution_bodies_preserves_nested_command --lib
```

Expected: at least one test fails until `embedded_scripts.rs` owns and re-exports the helpers.

- [ ] **Step 3: Move embedded extraction helpers into `embedded_scripts.rs`**

Move the existing items as-is:

```rust
pub struct HeredocBody { /* moved */ }
pub struct InlineScript { /* moved */ }
pub fn extract_heredoc_bodies(cmd: &str) -> Vec<HeredocBody> { /* moved */ }
pub fn extract_inline_scripts(cmd: &str) -> Vec<InlineScript> { /* moved */ }
pub fn extract_process_substitution_bodies(cmd: &str) -> Vec<String> { /* moved */ }
pub fn extract_eval_payloads(cmd: &str) -> Vec<String> { /* moved */ }
```

Keep helper visibility narrow (`fn` / `struct` private) unless an item is already part of the parser facade.

- [ ] **Step 4: Run Stage 1 parser verification**

Run:

```bash
rtk cargo test parser:: --lib
rtk cargo test --test full_pipeline
```

Expected: parser unit tests and full-pipeline regression coverage both pass, showing the parser facade is stable enough for scanner work.

- [ ] **Step 5: Record the Stage 1 benchmark decision**

If parser changes touched only file layout and did not alter `benches/scanner_bench.rs` inputs or parser hot-path logic, record in the implementation notes:

```text
Stage 1 benchmark note: hot-path benchmarking not rerun because the change was a mechanical parser file split with no intended algorithmic change.
```

If parser logic changed for a corrective fix, replace that note with the exact benchmark command you reran:

```bash
rtk cargo bench --bench scanner_bench
```

- [ ] **Step 6: Commit**

```bash
rtk git add src/interceptor/parser tests/full_pipeline.rs
rtk git commit -m "refactor: split parser embedded extraction"
```

### Task 4: Create the scanner facade and move keyword/highlight helpers

**Files:**
- Create: `src/interceptor/scanner/mod.rs`
- Create: `src/interceptor/scanner/keywords.rs`
- Create: `src/interceptor/scanner/highlighting.rs`
- Modify: `src/interceptor/mod.rs`

- [ ] **Step 1: Add failing scanner tests for keyword and highlight stability**

Add tests to the future `src/interceptor/scanner/mod.rs` test module:

```rust
    #[test]
    fn quick_scan_still_detects_known_danger_keywords() {
        let scanner = test_scanner();
        assert!(scanner.quick_scan("rm -rf /tmp/demo"));
    }

    #[test]
    fn sorted_highlight_ranges_merge_overlapping_ranges() {
        let ranges = sorted_highlight_ranges_for_tests(
            "rm -rf /tmp/demo",
            &[
                test_match_result("rm -rf", 0, 6),
                test_match_result("-rf /tmp", 3, 11),
            ],
        );

        assert_eq!(ranges, vec![HighlightRange { start: 0, end: 11 }]);
    }
```

- [ ] **Step 2: Run the focused scanner tests to verify RED while the new scanner module tree does not exist**

Run:

```bash
rtk cargo test quick_scan_still_detects_known_danger_keywords sorted_highlight_ranges_merge_overlapping_ranges --lib
```

Expected: compile failure or missing-symbol failure until the facade and helper exports exist.

- [ ] **Step 3: Create the scanner facade and move helper ownership**

Start `src/interceptor/scanner/mod.rs` with:

```rust
mod assessment;
mod pipeline_semantics;
mod highlighting;
mod keywords;
mod recursive;

pub use assessment::{Assessment, DecisionSource, MatchResult, Scanner};
pub use highlighting::{HighlightRange, sorted_highlight_ranges_for_tests};
```

Move helper logic into focused modules:

```rust
// src/interceptor/scanner/keywords.rs
pub(super) fn extract_keywords(pattern: &str) -> Vec<String> { /* moved */ }

// src/interceptor/scanner/highlighting.rs
pub(super) fn sorted_highlight_ranges(cmd: &str, matches: &[MatchResult]) -> Vec<HighlightRange> {
    /* moved */
}
```

Treat `keywords.rs` as false-negative-sensitive: keep extraction logic byte-for-byte equivalent unless a documented corrective fix is intentionally made.

- [ ] **Step 4: Re-run the focused scanner tests to verify GREEN**

Run:

```bash
rtk cargo test quick_scan_still_detects_known_danger_keywords sorted_highlight_ranges_merge_overlapping_ranges --lib
```

Expected: both tests pass and external imports through `crate::interceptor::scanner::*` still compile.

- [ ] **Step 5: Commit**

```bash
rtk git add src/interceptor/mod.rs src/interceptor/scanner
rtk git commit -m "refactor: create scanner facade"
```

### Task 5: Move recursive scan glue and pipeline semantics

**Files:**
- Modify: `src/interceptor/scanner/mod.rs`
- Create: `src/interceptor/scanner/recursive.rs`
- Create: `src/interceptor/scanner/pipeline_semantics.rs`

- [ ] **Step 1: Add failing tests for recursive-target and pipeline-semantics preservation**

Add tests to `src/interceptor/scanner/mod.rs`:

```rust
    #[test]
    fn semantic_pipeline_matches_detect_network_to_shell_flow() {
        let pipelines = top_level_pipelines("curl https://example.test/x | bash");
        let matches = semantic_pipeline_matches(&pipelines);
        assert!(matches.iter().any(|m| m.pattern.id == "PIPE-001"));
    }

    #[test]
    fn scan_targets_include_nested_shell_and_eval_payloads() {
        let parsed = Parser::parse("bash -lc 'eval \"rm -rf /tmp/demo\"'");
        let report = scan_targets("bash -lc 'eval \"rm -rf /tmp/demo\"'", &parsed);
        assert!(report.targets.iter().any(|target| target.contains("rm -rf /tmp/demo")));
    }
```

- [ ] **Step 2: Run the focused scanner tests to verify RED while helper moves are incomplete**

Run:

```bash
rtk cargo test semantic_pipeline_matches_detect_network_to_shell_flow scan_targets_include_nested_shell_and_eval_payloads --lib
```

Expected: at least one test fails until both modules own and export the moved logic.

- [ ] **Step 3: Move pipeline semantics into `pipeline_semantics.rs`**

Move the existing helpers as-is:

```rust
pub(super) fn semantic_pipeline_matches(pipelines: &[PipelineChain]) -> Vec<MatchResult> {
    /* moved */
}

fn push_semantic_match(/* existing signature */) { /* moved */ }
fn is_shell_sink(segment: &str) -> bool { /* moved */ }
fn is_xargs_rm_sink(segment: &str) -> bool { /* moved */ }
fn extract_xargs_command(tokens: &[String]) -> Option<&str> { /* moved */ }
fn is_network_sink(segment: &str) -> bool { /* moved */ }
```

Keep pipeline semantics free of allowlist, UI, and snapshot policy.

- [ ] **Step 4: Move recursive target glue into `recursive.rs`**

Move the existing helpers as-is:

```rust
pub(super) fn scan_targets(cmd: &str, parsed: &ParsedCommand) -> RecursiveScanReport { /* moved */ }
fn requires_recursive_scan(cmd: &str) -> bool { /* moved */ }
fn push_unique_target(targets: &mut Vec<String>, target: String) { /* moved */ }
```

Keep all nested scanning wired through the existing `crate::interceptor::nested` machinery.

- [ ] **Step 5: Re-run the focused scanner tests to verify GREEN**

Run:

```bash
rtk cargo test semantic_pipeline_matches_detect_network_to_shell_flow scan_targets_include_nested_shell_and_eval_payloads --lib
```

Expected: both tests pass and the parser-to-scanner traversal seam remains explicit.

- [ ] **Step 6: Commit**

```bash
rtk git add src/interceptor/scanner
rtk git commit -m "refactor: split scanner traversal"
```

### Task 6: Move assessment orchestration and finish Stage 2 verification

**Files:**
- Modify: `src/interceptor/scanner/mod.rs`
- Create: `src/interceptor/scanner/assessment.rs`
- Modify: `src/interceptor/scanner` tests

- [ ] **Step 1: Add failing classification-preservation tests around `Scanner::assess`**

Add tests to `src/interceptor/scanner/mod.rs`:

```rust
    #[test]
    fn assess_still_returns_safe_for_benign_input() {
        let scanner = test_scanner();
        let assessment = scanner.assess("echo hello world");
        assert_eq!(assessment.risk, RiskLevel::Safe);
        assert!(assessment.matched.is_empty());
    }

    #[test]
    fn assess_still_returns_uncertain_when_inline_script_exceeds_limit() {
        let scanner = test_scanner();
        let cmd = format!("python -c '{}'", "x".repeat(MAX_INLINE_SCRIPT_LEN + 1));
        let assessment = scanner.assess(&cmd);
        assert_eq!(assessment.risk, RiskLevel::Warn);
        assert!(assessment
            .matched
            .iter()
            .any(|m| m.pattern.id == "SCAN-002"));
    }
```

- [ ] **Step 2: Run the focused assessment tests to verify RED while orchestration is not fully moved**

Run:

```bash
rtk cargo test assess_still_returns_safe_for_benign_input assess_still_returns_uncertain_when_inline_script_exceeds_limit --lib
```

Expected: at least one test fails until `assessment.rs` owns the assess orchestration path.

- [ ] **Step 3: Move `Scanner::assess` orchestration into `assessment.rs`**

Move the assess path and related types so that `assessment.rs` owns:

```rust
pub struct MatchResult { /* moved */ }
pub enum DecisionSource { /* moved */ }
pub struct Assessment { /* moved */ }

impl Scanner {
    pub fn assess(&self, cmd: &str) -> Assessment {
        // quick-scan gate
        // full-scan fanout
        // recursive target scan
        // dedup / merge
        // final max-risk aggregation
    }
}
```

Keep `highlighting.rs` presentation-only by computing highlight ranges there and consuming them from `assessment.rs` without feeding them back into classification.

- [ ] **Step 4: Run Stage 2 scanner verification**

Run:

```bash
rtk cargo test scanner:: --lib
rtk cargo test --test full_pipeline
```

Expected: scanner unit tests and end-to-end regressions both pass with no policy drift.

- [ ] **Step 5: Record the Stage 2 benchmark decision**

If the scanner split remained mechanical and hot-path behavior was unchanged, record:

```text
Stage 2 benchmark note: hot-path benchmarking not rerun because the scanner split preserved the quick-scan and full-scan algorithms without intended performance change.
```

If scanner hot-path logic changed for a corrective fix or meaningful wiring change, rerun:

```bash
rtk cargo bench --bench scanner_bench
```

and record the command plus result summary in the implementation notes.

- [ ] **Step 6: Commit**

```bash
rtk git add src/interceptor/scanner tests/full_pipeline.rs
rtk git commit -m "refactor: split scanner assessment"
```

### Task 7: Run final repository verification and close out the staged rollout

**Files:**
- Modify: implementation notes / PR summary only if needed

- [ ] **Step 1: Run formatting and linting**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
```

Expected: both commands pass cleanly.

- [ ] **Step 2: Run full test coverage for the changed scope**

Run:

```bash
rtk cargo test
```

Expected: all unit and integration tests pass.

- [ ] **Step 3: Run security and dependency gates required by repo policy**

Run:

```bash
rtk cargo audit
rtk cargo deny check
```

Expected: both commands pass, or any pre-existing external issue is called out explicitly before merge.

- [ ] **Step 4: Record the final corrective-fix log if any exceptions were taken**

If any stage included a corrective fix, add a closeout note with this exact shape:

```text
- Fix: <short title>
  Before: <old behavior>
  After: <new behavior>
  Rationale: <fail-closed or boundary justification>
  Regression test: <exact test name>
```

If there were no corrective fixes, record:

```text
No corrective fixes were taken; rollout remained behavior-preserving.
```

- [ ] **Step 5: Commit any final notes if needed**

```bash
rtk git add .
rtk git commit -m "docs: record shell module verification"
```

Only perform this commit if a real notes artifact was created; otherwise skip this step and state that no closeout artifact was necessary.

## Verification Plan

- Parser-stage verification:
  - `rtk cargo test parser:: --lib`
  - `rtk cargo test --test full_pipeline`
- Scanner-stage verification:
  - `rtk cargo test scanner:: --lib`
  - `rtk cargo test --test full_pipeline`
- Final repo verification:
  - `rtk cargo fmt --check`
  - `rtk cargo clippy -- -D warnings`
  - `rtk cargo test`
  - `rtk cargo audit`
  - `rtk cargo deny check`
- Hot-path benchmark handling:
  - each stage records whether `rtk cargo bench --bench scanner_bench` was rerun
  - if not rerun, the implementation notes must explain why it was unnecessary

## Rollback Plan

- If Task 1–3 regress, revert the parser-stage commits only and keep scanner untouched.
- If Task 4–6 regress, revert the scanner-stage commits while keeping the stabilized parser facade.
- If a corrective fix causes drift, revert that single commit and keep the mechanical module split.
- Do not revert allowlist or confirm semantics as part of this initiative unless they changed accidentally.
