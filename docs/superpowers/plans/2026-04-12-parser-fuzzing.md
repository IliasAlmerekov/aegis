# Parser Fuzzing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add phase-1 parser fuzzing infrastructure to the repo, seed it with representative shell-input corpus files, and synchronize docs so they accurately describe the new fuzzing status.

**Architecture:** Keep the rollout narrow and infrastructure-first. Introduce a standard `cargo-fuzz` layout under `fuzz/`, target only the public string-facing parser API via `Parser::parse(&str)`, add a small committed parser corpus, then update ADR text so it states exactly what is now implemented and what is still deferred to the follow-on scanner phase.

**Tech Stack:** Rust 2024, `cargo-fuzz`, `libfuzzer-sys`, existing `aegis::interceptor::parser::Parser` API, Markdown docs, `.gitignore`.

---

## File Structure

- `fuzz/Cargo.toml`
  - Separate fuzz package metadata for `cargo-fuzz`; depends on the local `aegis` crate and `libfuzzer-sys`.
- `fuzz/fuzz_targets/parser.rs`
  - Phase-1 fuzz harness targeting the public string-facing parser API only.
- `fuzz/corpus/parser/empty.txt`
  - Empty-input parser seed.
- `fuzz/corpus/parser/whitespace.txt`
  - Whitespace-only parser seed.
- `fuzz/corpus/parser/quotes.txt`
  - Representative quoted/escaped command seed.
- `fuzz/corpus/parser/unterminated-quote.txt`
  - Unbalanced quote seed.
- `fuzz/corpus/parser/heredoc.txt`
  - Heredoc command seed.
- `fuzz/corpus/parser/unterminated-heredoc.txt`
  - Unterminated heredoc seed.
- `fuzz/corpus/parser/inline-python.txt`
  - Inline interpreter seed.
- `fuzz/corpus/parser/nested-shell.txt`
  - Nested shell invocation seed.
- `.gitignore`
  - Ignore local fuzz-generated outputs such as `fuzz/artifacts/` and `fuzz/coverage/`.
- `docs/architecture-decisions.md`
  - Replace the stale “already implemented” fuzzing claim with phase-accurate wording.

---

## Milestones

1. Add repo-safe fuzzing infrastructure and keep generated outputs untracked.
2. Add a parser-only fuzz target with committed parser seed corpus.
3. Update ADR documentation to match the real repo state.
4. Smoke-verify `cargo-fuzz` build/run on the parser target.

---

## Task Graph

- Task 1 (`.gitignore` guardrails) should land first so local fuzz outputs do not pollute the repo during verification.
- Task 2 (fuzz manifest + parser target) depends on Task 1.
- Task 3 (seed corpus) depends on Task 2 so the harness has committed inputs to run against.
- Task 4 (ADR sync) depends on Tasks 2–3 because docs must describe the actual landed state.
- Task 5 (smoke verification) depends on Tasks 1–4.

---

## Task Details

### Task 1: Ignore local fuzzing outputs

**Files:**
- Modify: `.gitignore`

- [ ] **Step 1: Add fuzz output ignores**

Append these lines to `.gitignore`:

```gitignore
fuzz/artifacts/
fuzz/coverage/
fuzz/target/
```

- [ ] **Step 2: Review the diff**

Run:

```bash
rtk git diff -- .gitignore
```

Expected: diff shows only the three new `fuzz/` ignore entries.

- [ ] **Step 3: Commit the ignore guardrail**

```bash
rtk git add .gitignore
rtk git commit -m "build: ignore local fuzz outputs"
```

### Task 2: Add `cargo-fuzz` manifest and parser harness

**Files:**
- Create: `fuzz/Cargo.toml`
- Create: `fuzz/fuzz_targets/parser.rs`

- [ ] **Step 1: Create the fuzz manifest**

Create `fuzz/Cargo.toml` with this exact content:

```toml
[package]
name = "aegis-fuzz"
version = "0.0.0"
publish = false
edition = "2024"

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4"

[dependencies.aegis]
path = ".."

[[bin]]
name = "parser"
path = "fuzz_targets/parser.rs"
test = false
doc = false
bench = false
```

- [ ] **Step 2: Create the parser fuzz target**

Create `fuzz/fuzz_targets/parser.rs` with this exact content:

```rust
#![no_main]

use aegis::interceptor::parser::Parser;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Phase 1 fuzzes the public string-facing parser API, not a raw-byte contract.
    let input = String::from_utf8_lossy(data);
    let _ = Parser::parse(&input);
});
```

- [ ] **Step 3: Build the parser target**

Run:

```bash
rtk cargo +nightly fuzz build parser
```

Expected: build succeeds. If the command fails only because `cargo-fuzz` is not installed locally, install the standard tool and rerun:

```bash
rtk cargo install cargo-fuzz
rtk cargo +nightly fuzz build parser
```

Expected after install: build succeeds without any code changes outside the planned files.

- [ ] **Step 4: Commit the fuzz harness infrastructure**

```bash
rtk git add fuzz/Cargo.toml fuzz/fuzz_targets/parser.rs
rtk git commit -m "test: add parser fuzz harness"
```

### Task 3: Add the committed parser seed corpus

**Files:**
- Create: `fuzz/corpus/parser/empty.txt`
- Create: `fuzz/corpus/parser/whitespace.txt`
- Create: `fuzz/corpus/parser/quotes.txt`
- Create: `fuzz/corpus/parser/unterminated-quote.txt`
- Create: `fuzz/corpus/parser/heredoc.txt`
- Create: `fuzz/corpus/parser/unterminated-heredoc.txt`
- Create: `fuzz/corpus/parser/inline-python.txt`
- Create: `fuzz/corpus/parser/nested-shell.txt`

- [ ] **Step 1: Add empty and whitespace seeds**

Create these two files:

`fuzz/corpus/parser/empty.txt`

```text
```

`fuzz/corpus/parser/whitespace.txt`

```text
   
```

- [ ] **Step 2: Add quote-focused seeds**

Create these two files:

`fuzz/corpus/parser/quotes.txt`

```text
echo "hello world" 'and goodbye' escaped\ space
```

`fuzz/corpus/parser/unterminated-quote.txt`

```text
echo "unterminated
```

- [ ] **Step 3: Add heredoc-focused seeds**

Create these two files:

`fuzz/corpus/parser/heredoc.txt`

```text
cat <<'EOF'
hello
EOF
```

`fuzz/corpus/parser/unterminated-heredoc.txt`

```text
cat <<EOF
hello
```

- [ ] **Step 4: Add inline-interpreter and nested-shell seeds**

Create these two files:

`fuzz/corpus/parser/inline-python.txt`

```text
python -c "print('hello from fuzz seed')"
```

`fuzz/corpus/parser/nested-shell.txt`

```text
bash -c 'echo start && printf "%s\n" done'
```

- [ ] **Step 5: Smoke-run from the committed corpus**

Run:

```bash
rtk cargo +nightly fuzz run parser fuzz/corpus/parser
```

Expected: the target starts successfully and processes the committed parser corpus as a short smoke run. Stop it after a brief run once startup and corpus ingestion are confirmed.

- [ ] **Step 6: Commit the seed corpus**

```bash
rtk git add fuzz/corpus/parser
rtk git commit -m "test: add parser fuzz seed corpus"
```

### Task 4: Synchronize ADR-009 with the implemented phase-1 status

**Files:**
- Modify: `docs/architecture-decisions.md`

- [ ] **Step 1: Replace the stale ADR text**

Replace this existing ADR-009 block:

```markdown
## ADR-009: Fuzz testing for parser

**Decision:** `parser.rs` has dedicated fuzz targets using `cargo-fuzz` (libFuzzer).

**Rationale:** The parser handles untrusted shell command strings that may contain heredoc bodies, inline Python/Node scripts, nested quotes, and escape sequences. This is the highest-complexity, highest-risk code in the project — the exact profile where fuzzing reliably finds bugs that hand-written tests miss.

Fuzz targets are in `fuzz/fuzz_targets/`. Run with `cargo +nightly fuzz run fuzz_scanner`.

**Status:** Required before v1.0 release.
```

with this exact updated block:

```markdown
## ADR-009: Fuzz testing for parser and scanner

**Decision:** Aegis uses `cargo-fuzz` (libFuzzer) for security-sensitive shell-input fuzzing. In phase 1, the repository implements a dedicated parser fuzz target; scanner fuzzing remains a required follow-on phase.

**Rationale:** The parser handles untrusted shell command strings that may contain heredoc bodies, inline Python/Node scripts, nested quotes, and escape sequences. This is the highest-complexity, highest-risk code in the project — the exact profile where fuzzing reliably finds bugs that hand-written tests miss. The scanner is the next surface in the same rollout, but it is intentionally deferred so parser failures are easier to triage first.

The phase-1 parser target lives at `fuzz/fuzz_targets/parser.rs`. Run it with `cargo +nightly fuzz run parser fuzz/corpus/parser`.

**Status:** Parser fuzz target implemented in this phase; scanner fuzz target not yet implemented in this phase. Fuzzing remains required before v1.0 release.
```

- [ ] **Step 2: Sweep for stale explicit fuzz-target claims**

Run:

```bash
rtk rg -n "has dedicated fuzz targets|fuzz_targets/|cargo \\+nightly fuzz run fuzz_scanner" README.md CONVENTION.md docs/architecture-decisions.md
```

Expected: no remaining explicit false claim that parser/scanner fuzz targets are already fully implemented. `CONVENTION.md` may still contain forward-looking requirements; do not weaken them.

- [ ] **Step 3: Commit the doc sync**

```bash
rtk git add docs/architecture-decisions.md
rtk git commit -m "docs: align fuzzing status with implementation"
```

### Task 5: Run final smoke verification and record the exact command outcomes

**Files:**
- Review: `.gitignore`
- Review: `fuzz/Cargo.toml`
- Review: `fuzz/fuzz_targets/parser.rs`
- Review: `fuzz/corpus/parser/*`
- Review: `docs/architecture-decisions.md`

- [ ] **Step 1: Rebuild the parser fuzz target from the landed files**

Run:

```bash
rtk cargo +nightly fuzz build parser
```

Expected: PASS.

- [ ] **Step 2: Re-run the parser smoke fuzz command**

Run:

```bash
rtk cargo +nightly fuzz run parser fuzz/corpus/parser
```

Expected: target launches and ingests the committed parser corpus without requiring unplanned code changes. Stop after a short smoke run.

- [ ] **Step 3: Inspect the final working tree**

Run:

```bash
rtk git status --short
```

Expected: clean working tree, or only intentionally uncommitted local fuzz artifacts ignored by `.gitignore`.

- [ ] **Step 4: Commit any final verification-only adjustment if one was needed**

If no additional file changes were required, skip this step. If a tiny verification-only doc or ignore-file adjustment was required, commit it with:

```bash
rtk git add .gitignore docs/architecture-decisions.md fuzz/Cargo.toml fuzz/fuzz_targets/parser.rs fuzz/corpus/parser
rtk git commit -m "chore: finalize parser fuzz rollout"
```

---

## Verification Plan

Run these commands during execution:

```bash
rtk cargo +nightly fuzz build parser
rtk cargo +nightly fuzz run parser fuzz/corpus/parser
rtk rg -n "has dedicated fuzz targets|fuzz_targets/|cargo \\+nightly fuzz run fuzz_scanner" README.md CONVENTION.md docs/architecture-decisions.md
rtk git status --short
```

Expected outcomes:

- the parser fuzz target builds successfully
- the parser target can be smoke-run from the committed seed corpus
- the repo does not accumulate tracked fuzz artifacts
- ADR text accurately states that parser fuzzing is implemented and scanner fuzzing is still deferred in this phase

---

## Rollback Plan

- Revert `.gitignore` changes if they accidentally ignore committed source files outside local fuzz outputs.
- Revert `fuzz/` additions if the harness cannot be made to build without unplanned parser/scanner API changes.
- Revert the ADR wording if it diverges from the actually landed fuzzing state.
- If a fuzz-discovered parser fix becomes necessary during execution and expands scope beyond an obvious crash/panic fix, stop after capturing the reproducer and create a follow-up plan instead of broadening this phase.
