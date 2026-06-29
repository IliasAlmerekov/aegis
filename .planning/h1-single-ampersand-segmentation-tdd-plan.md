# H1 TDD Plan — Single `&` command segmentation gap

## Task

Implement `TASKS.md` finding **H1 — Single `&` command segmentation gap**.

Command segmentation handles `&&`, `||`, `;`, `|`, and newlines, but a standalone
background `&` is **not** treated as a `Command separator`. In both
`split_top_level_segments` and `split_top_level_command_groups`
(`crates/aegis-parser/src/segmentation.rs`) the `&` arm fires only when
`chars.peek() == Some(&'&')`, so `echo ok & git push --force` stays a single
`Logical segment`. Its `Effective program` resolves to `echo`, and the Git/Docker/DB/
Cloud/Process `Token-prefix rule`s — which key on the effective program of each
segment — never fire. Regex `Pattern`s (match anywhere) and `Intrinsic Block` still
fire, so the leak is specifically the token-prefix layer.

## Threat model (agreed during grilling)

- **Real, not theoretical**, on `assess()`: `echo ok & git push --force`,
  `true & docker system prune -af` bypass token-prefix rules of `Warn`/`Danger`
  severity.
- Catastrophic intrinsic blocks (`rm -rf /`) are unaffected — they match anywhere.

## Constraints

- All shell commands run through `rtk`.
- Do **not** add dependencies; do not touch `Cargo.toml` / `Cargo.lock` / `deny.toml`.
- **Scope is exactly two functions:** `split_top_level_segments` and
  `split_top_level_command_groups`. Do **not** change `split_pipeline_segments`
  (groups are already split on `&` upstream) or the tokenizer `split_tokens` (the
  glued-`&` literal, e.g. `10&`, is a separate backlog item, not a bypass vector).
- Preserve the **fail-closed invariant**: the change only _adds_ segment boundaries
  → more scan targets, never fewer. `Intrinsic Block` stays untouched.
- Stay within project boundaries (ADR-010): a narrow heuristic, **not** a full redirect
  parser.
- Hot path stays synchronous (ADR-002), no new allocations; safe path < 2ms.
- No `&` ADR — the change is reversible, expected, and carries no non-trivial trade-off.

## The discriminator (surprising trade-off — redirects)

A naive "split on every `&`" would corrupt legitimate bash redirect syntax. Treat `&`
as a background `Command separator` **only if all three** hold:

1. `peek != '&'` — not `&&` (logical AND, already handled).
2. `peek != '>'` — not `&>` / `&>>` (combined stdout+stderr redirect).
3. last non-whitespace char of `current` is not `>` — not `>&` / `2>&1` / `3>&-`
   (fd duplication/close).

Otherwise `&` is pushed to `current` as an ordinary character (current behavior).

### Reference shape

Replace the existing `&&`-only arm in **both** functions with:

```rust
'&' if !in_single_quote
    && !in_double_quote
    && !in_backticks
    && paren_depth == 0
    && command_subst_depth == 0 =>
{
    if chars.peek() == Some(&'&') {
        chars.next();                       // && — logical AND
        finalize_segment(&mut current, &mut segments);
    } else if chars.peek() != Some(&'>')    // not &> / &>>
        && current.trim_end().chars().next_back() != Some('>') // not >& / 2>&1
    {
        finalize_segment(&mut current, &mut segments); // background separator
    } else {
        current.push(ch);                   // part of a redirect — ordinary char
    }
}
```

`current.trim_end().chars().next_back()` yields the last non-whitespace char already
accumulated — the "preceded by `>`" check.

## Definition of Done

1. `split_top_level_segments` and `split_top_level_command_groups` segment on a
   standalone background `&`.
2. Redirect forms `&>`, `&>>`, `>&`, `2>&1` are **not** split.
3. `assess("echo ok & git push --force", …)` raises the Git token-prefix rule
   (no longer `Safe`).
4. Regression tests fail on the old code and pass on the new code.
5. No changes to `split_pipeline_segments`, `split_tokens`, dependencies, or lockfile.
6. Local gates pass:
   - `rtk cargo fmt --check`
   - `rtk cargo clippy --all-targets -- -D warnings`
   - `rtk cargo test --workspace`

## Iteration 0 — Baseline inspection

### Commands

```bash
rtk sed -n '85,250p' crates/aegis-parser/src/segmentation.rs
rtk sed -n '100,200p' crates/aegis-parser/src/tests/parsing_tests.rs
rtk cargo test -p aegis-parser
```

### Checks

- Confirm both segmentation functions gate `&` on `peek == Some(&'&')`.
- Confirm the `logical_segments` test section location.
- Locate a concrete Git `Token-prefix rule` in the scanner for the end-to-end test
  (candidate: `git push --force`).

## Iteration 1 — RED: segmentation unit tests

### File

`crates/aegis-parser/src/tests/parsing_tests.rs` (the `logical_segments` section).

### Test cases (must fail on current code)

1. `segments_single_ampersand` —
   `logical_segments("echo hi & git push --force")` → `["echo hi", "git push --force"]`.
2. `segments_ampersand_chain` —
   `logical_segments("a & b & c")` → `["a", "b", "c"]`.
3. `segments_ampersand_no_spaces` —
   `logical_segments("echo hi&git push")` → `["echo hi", "git push"]`.
4. `segments_trailing_ampersand` —
   `logical_segments("sleep 10 &")` → `["sleep 10"]`.

### RED command

```bash
rtk cargo test -p aegis-parser segments_
```

### Expected

Tests 1–4 fail: the `&` segments are not split today.

## Iteration 2 — GREEN: apply the discriminator

### Change

Apply the reference-shape `&` arm to **both** `split_top_level_segments` and
`split_top_level_command_groups`.

### GREEN command

```bash
rtk cargo test -p aegis-parser segments_
```

## Iteration 3 — Redirect anti-regression tests (stay green)

### Test cases (must be green right after Iteration 2)

1. `segments_combined_redirect_not_split` —
   `logical_segments("echo foo &> /dev/null")` → one segment.
2. `segments_append_redirect_not_split` —
   `logical_segments("echo foo &>> log")` → one segment.
3. `segments_fd_dup_not_split` —
   `logical_segments("ls >&2")` and `logical_segments("cmd 2>&1")` → one segment each.

### Command

```bash
rtk cargo test -p aegis-parser segments_
```

### Rule

If any of these is red, the discriminator (condition 2 or 3) is wrong — fix the
discriminator, not the test. This is the guard against breaking bash redirect syntax.

## Iteration 4 — RED→GREEN: scanner end-to-end test

### Goal

Prove the bypass is closed at `assess()`, not only in the parser.

### File

A scanner test module (e.g. `crates/aegis-scanner/src/scanner/tests/...`), matching the
existing test layout.

### Test case

`ampersand_does_not_bypass_git_prefix_rule` —
`assess("echo ok & git push --force", …)` returns a `RiskLevel` of at least the level
the Git token-prefix rule assigns (`Warn`/`Danger`), whereas the old code returned
`Safe`. Pick the exact rule/command from the existing Git rule set during
implementation so the test goes red on this bug.

### Commands

```bash
rtk cargo test -p aegis-scanner ampersand_does_not_bypass
```

## Iteration 5 — TASKS/docs synchronization

### Change

- Mark `TASKS.md` H1 from `[ ]` to `[x]` with a one-line completion summary.
- Add a short line to `PROJECT_STATE.md` if that file tracks per-finding status.
- `CONTEXT.md` already gained `Logical segment` and `Command separator` during grilling.

### Rule

Do not mark H1 complete before the parser and scanner tests pass.

## Iteration 6 — Final verification

### Required commands

```bash
rtk cargo fmt --check
rtk cargo clippy --all-targets -- -D warnings
rtk cargo test --workspace
```

### Hot-path check

If a parser/scanner bench exists, run it and confirm the safe path stays < 2ms (the
change adds one `peek` + one last-char check, no allocations):

```bash
rtk cargo bench -p aegis-parser 2>/dev/null || true
```

### Security/dependency gates (if environment allows)

```bash
rtk cargo audit
rtk cargo deny check
```

If these fail on pre-existing baseline debt, record separately — this diff changes
neither dependencies nor the dependency graph.

## Review checklist

- [ ] Both segmentation functions split on a standalone background `&`.
- [ ] `&>`, `&>>`, `>&`, `2>&1` are not split.
- [ ] `a & b & c`, no-space `&`, and trailing `&` behave as specified.
- [ ] Scanner test: `echo ok & git push --force` raises the Git token-prefix rule.
- [ ] The new tests fail on the old code and pass on the new code.
- [ ] `split_pipeline_segments` and `split_tokens` are unchanged.
- [ ] No dependency or lockfile changes.
- [ ] Fail-closed preserved (more scan targets, never fewer); `Intrinsic Block` untouched.
- [ ] `TASKS.md` marked complete only after verification.

## Suggested commit

```text
fix(parser): segment on standalone background &
```

Include only the files needed for H1:

- `crates/aegis-parser/src/segmentation.rs`
- `crates/aegis-parser/src/tests/parsing_tests.rs`
- the scanner test file touched in Iteration 4
- `TASKS.md` / `PROJECT_STATE.md` if marking the finding complete
- this plan file if it is intended to stay in the repository
