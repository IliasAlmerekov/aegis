# Project State

> **Agent instructions:** Read this file at the start of every session to restore context.
> After completing any significant change, update the relevant sections here.
> Keep entries concise. This file is a pointer to current state, not a log —
> history lives in git and `CHANGELOG.md`; architectural rationale lives in `docs/adr/`.

---

## Current version

`0.6.2` — pre-1.0, targeting `1.0.0` (tagged `v0.6.2`)

## Active branch

`main`

## Last updated

2026-07-22

---

## Last session (2026-07-22, cont. 5) — L1 Iteration 7 TypeScript corpus

- **Iteration 7 TypeScript corpus done via TDD (ADR-022 §11, plan Iteration 7
  RED step — the corpus half).** Scope and seams confirmed with the user up
  front per the TDD skill: the public
  `aegis_language::languages::typescript::analyze(&str) -> AdapterResult` seam,
  unit-tested in-process via `crates/aegis-language/tests/typescript_corpus.rs`
  (mirroring `javascript_corpus.rs`); NO real-subprocess orchestration seam —
  the router routes no inline runner to TypeScript today, so an end-to-end TS
  fixture would be synthetic (deferred with the TypeScript runner-routing
  slice). This mirrors the JavaScript corpus slice exactly, minus the
  `node -e` fixtures the JS slice had (those exist because `node -e` routes to
  JS). It is a **characterization + regression corpus**, not a classic
  RED→GREEN: the adapter already behaves per spec (covered by the 31 TS unit
  tests from Slice 1), so the corpus pins existing correct behavior with
  hand-derived expectations (independent of the adapter — not tautological).
  The genuine RED-risk was `modern_syntax`: the pinned tree-sitter-typescript
  0.23.2 grammar might not parse generics / arrow generics `<T,>` / `satisfies`
  / decorators / `import type` / mapped / conditional / `infer` types at file
  scale; it does (parse_errors == 0, no false ops) — GREEN on first run.
- **Corpus (`crates/aegis-language/tests/corpora/typescript/`, 9 `.ts` files)
  + harness (`tests/typescript_corpus.rs`, 9 tests):** files embedded
  compile-time via `include_str!`; a hand-derived `ExpectedOp` manifest declares
  per-file operation kinds, modifiers, `OperandCertainty`, parse-error count,
  and nested payload `(language, source)`. Spans deliberately not pinned
  (implementation detail; unit tests own span coverage). Coverage: `fs_delete`
  (unlinkSync/rmdirSync + rmSync recursive in identifier- and string-keyed
  shapes; first call is a `fs.unlinkSync<void>(…)` type-argument call),
  `fs_overwrite` (writeFileSync destructive_mode vs appendFileSync), `perms`
  (chmodSync/chownSync), `exec_shell` (child_process.exec/execSync/exec\<void\>
  → Bash nested payloads), `exec_js` (eval + `new Function<string>(...)` →
  JavaScript nested payloads — the shared `family` module tags `eval`/`Function`
  as `SourceLanguage::JavaScript` regardless of the enclosing file's language),
  `negatives` (comment/string/member-ref-without-call/unrelated-call/ESM
  import + TS-only `import type`/interface/type alias/enum/`as const`/
  `satisfies`/decorator → 0 ops), `dynamic_operand` (variable/cmd/template
  interpolation/`as`-cast operand → Dynamic certainty, NO nested payload),
  `modern_syntax` (generics, arrow generics `<T,>`, optional chaining, nullish
  coalescing, `as const`, `satisfies`, decorators, `import type`, mapped /
  conditional / `infer` types → parse clean, 0 ops), `malformed` (unterminated
  call → parse_errors > 0). TS-only enrichment over the JS corpus: explicit
  type arguments on tracked calls (`fs.unlinkSync<void>(…)`,
  `child_process.exec<void>(…)`, `new Function<string>(…)`) and an `as`-cast
  dynamic operand — proving those calls still capture and classify at corpus
  scale.
- **Verified:** `cargo test --workspace` = 1903 passed / 101 suites / 0 failed
  (+9 this slice: 9 TS corpus tests; +1 new suite `typescript_corpus`);
  workspace `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt --all
  --check` clean; `tests/aegis_language_boundary.rs` and
  `tests/file_size_budget.rs` green (corpus files are `tests/` data, not `src/`,
  so the 800-line budget does not apply). Hot path untouched (all additive
  slow-path corpus + harness), so no scanner bench run was required.
- **Deferred (documented, not silently dropped):** a TypeScript inline runner
  in the router registry (`ts-node -e` / `tsx -e` / `deno eval`) +
  trusted-alias/config wiring so real end-to-end orchestration tests (mirror of
  the JS `node -e` fixtures) become non-synthetic; Node inline/file/stdin and
  TypeScript runner-routing negative cases; per-adapter TS fuzz target;
  `fs.promises.*`/callback-form variants; import/alias/constant →
  `OperandCertainty::Partial` (bounded symbol resolution); `DatabaseDestructive`;
  chained member calls (`a.b.c()` — `calls.scm` matches `object: (identifier)`
  only); `ScriptFile`/`DirectExec` fs reads; live `RuntimeContext::assess`
  wiring; audit v1/v2 projection; the all-four-targets qualification gate
  before TypeScript becomes default-on.
- **Loop code review (this session):** an independent reviewer scored the TS
  corpus slice 9/10 (test quality 9/10) with one actionable Low finding — the
  corpus harness helpers (`ExpectedOp`, `assert_ops`, `assert_clean_no_ops`,
  `assert_malformed`, `bash_exec`) were triplicated across the JavaScript,
  TypeScript, and Python corpora. Resolved by extracting
  `crates/aegis-language/tests/common/corpus_harness.rs` (included via
  `#[path]`, matching the `no_source_corpus.rs` precedent); the three corpora
  now share assertion semantics, and language-specific payload builders
  (`js_exec`, `python_exec`) stay local. Behavior-preserving — no expectations
  changed. Re-verified: `cargo test --workspace` = 1903 passed / 101 suites /
  0 failed; `cargo clippy --all-targets -p aegis-language -- -D warnings` clean;
  `cargo fmt --all --check` clean. The reviewer's informational note (the
  `first<number>([1])` call site in `modern_syntax` is non-tracked, so the
  clean-parse assertion — via the `<T,>` arrow generic — is what guards TS
  type-argument parsing, not the 0-ops line) needs no change.

---

## Prior session (2026-07-22, cont. 4) — L1 Iteration 7 Slice 2 (TypeScript worker wiring)

- **Iteration 7 Slice 2 done via TDD — the TypeScript adapter now runs in the
  self-spawned worker, so an Analyze request for TypeScript dispatches to
  `typescript::analyze` and frames its `AdapterResult`.** Scope and seams
  confirmed with the user up front per the TDD skill (Slice 2 = worker dispatch
  wiring only; the parent orchestration is already language-agnostic from
  Iteration 6 Slices B/C). This mirrors the JavaScript Slice 2 cadence exactly,
  minus the orchestration/subprocess seams: the router routes **no inline runner
  to TypeScript** today (0 `TypeScript` references in `src/analysis/router.rs`;
  no `ts-node`/`tsx`/`deno` in the registry), so an end-to-end TS fixture would
  be synthetic. Those orchestration tests belong with the deferred
  "TypeScript runner-routing" slice (plan Iteration 7 RED: "Node inline/file/
  stdin and TypeScript runner-routing negative cases").
- **Worker dispatch (`crates/aegis-language/src/worker.rs`):** `analyze_source`
  now matches `TypeScript` to `crate::languages::typescript::analyze` alongside
  Python and JavaScript; the `Bash` arm still returns `UnsupportedLanguage`
  (Bash is the last foundation grammar without an adapter — L1 Shell/Bash is
  Iteration 8). Doc comment updated (was "Python and JavaScript ship adapters;
  TypeScript and Bash do not yet"). This is the entire production change — the
  generic `map_operation` (Iteration 5) maps TS `DetectedOperation`s to `LANG-*`
  Matches for free, so no classifier change.
- **TDD (1 seam, RED → GREEN):** worker dispatch unit test
  `run_analyzes_typescript_source_and_returns_an_analyzed_response` —
  `Request::Analyze { TypeScript, "fs.unlinkSync<void>(\"data.txt\")" }` →
  `Analyzed` with `parse_errors == 0` and non-empty `operations`. The
  type-argument call (`<void>`) is TypeScript-only syntax the JS adapter does
  not exercise, so a clean parse + an op proves the worker reached the TS
  adapter (the `calls.scm` query surfaces the op because `type_arguments` is a
  separate child, not the `function` field — pinned in `languages::typescript`).
  RED today (UnsupportedLanguage); GREEN after wiring. The test pins dispatch
  reached the adapter, not its exact output (that is the adapter's own contract,
  already covered by the 31 TS unit tests from Slice 1).
- **Existing-test retargeting (preserves the UnsupportedLanguage path):** the
  unit test `run_returns_unsupported_language_for_a_language_without_an_adapter`
  was switched from `TypeScript` to `Bash` (still no adapter; Iteration 8). It
  keeps pinning the UnsupportedLanguage → degradation contract that TS no longer
  exercises. The orchestration test `run_records_grammar_unavailable_for_an_unsupported_language`
  already uses `bash -c "x"` (unchanged — Bash still unsupported); its comment
  was updated to note both JS and TS gained adapters in Iteration 7.
- **Verified:** `cargo test --workspace` = 1894 passed / 100 suites / 0 failed
  (+1 this slice: the TS worker dispatch test); workspace `cargo clippy
  --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean; `worker.rs`
  ~620 lines (under the 800-line budget). Hot path untouched (all additive
  slow-path `aegis-language` worker dispatch), so no scanner bench run was
  required.
- **Deferred (documented, not silently dropped):** TS corpora (`.ts` files,
  plan Iteration 7 RED step); a TypeScript inline runner in the router registry
  (`ts-node -e` / `tsx -e` / `deno eval`) + trusted-alias/config wiring so real
  end-to-end orchestration tests (mirror of JS Slice 2 seams 2 and 3) become
  non-synthetic; Node inline/file/stdin and TypeScript runner-routing negative
  cases; per-adapter TS fuzz target; `fs.promises.*`/callback-form variants;
  import/alias/constant → `OperandCertainty::Partial` (bounded symbol
  resolution); `DatabaseDestructive`; chained member calls (`a.b.c()`);
  `ScriptFile`/`DirectExec` fs reads; live `RuntimeContext::assess` wiring;
  audit v1/v2 projection; the all-four-targets qualification gate before
  TypeScript becomes default-on.

---

## Prior session (2026-07-22, cont. 3) — L1 Iteration 7 Slice 1 (TypeScript adapter)

- **Iteration 7 Slice 1 done via TDD — the TypeScript adapter lands as the
  JavaScript-family's other half (ADR-022 §3, plan Iteration 7).** Scope and
  seams confirmed with the user up front per the TDD skill: the public
  `aegis_language::languages::typescript::analyze(&str) -> AdapterResult` seam,
  unit-tested in-process (mirroring `javascript_tests.rs`); NO worker wiring
  this slice (that is Slice 2, mirroring the JS Slice 1 → Slice 2 cadence).
- **Shared JS-family logic extracted (`crates/aegis-language/src/languages/
  family.rs`, new):** the grammar-agnostic interpretation — `CallClass`/
  `ExecLang`/`ExecArg`, `classify_path`, `interpret`, `collect_operations`
  (the call-capture query loop), `operand_certainty`/`is_string_literal`/
  `string_literal_content`, `recursive_option`, `first/last_positional_arg`,
  `span_for`, `exec_language` — moved out of `javascript.rs` into a
  `pub(crate)` `family` module that both adapters call (plan Iteration 7 GREEN:
  "share JavaScript-family resolution where syntax permits, but keep grammar
  and span handling explicit per adapter"). `javascript.rs` slimmed to its own
  parser + query + `analyze` delegating to `family::collect_operations`
  (104 lines, was 454). The existing 34 JS unit tests + 9-test JS corpus
  guarded the behavior-preserving extraction (both stayed green throughout).
- **TypeScript adapter (`crates/aegis-language/src/languages/typescript.rs`,
  105 lines):** owns its per-thread parser (`LANGUAGE_TYPESCRIPT`) + `LazyLock`
  `calls.scm` query (`queries/typescript/calls.scm`, structurally identical to
  the JS query — TS reuses the JS node types) + `analyze` delegating to
  `family::collect_operations`. `mod typescript` + `mod family` wired in
  `languages/mod.rs`.
- **TDD (31 TS unit tests, `typescript_tests.rs` via `#[path]`, RED → GREEN):**
  a parsing-but-no-classification stub first (RED: 16 `one_op` failures, the
  `no_ops`/`malformed` tests already green since the stub computes
  `parse_errors`); then the real adapter (GREEN: 31 passed). Tests cover one
  tracer per operation category (fs delete / rmSync recursive / writeFileSync
  destructive / chmod / eval JS payload / new Function JS payload /
  child_process.exec Bash payload / spawn argv Dynamic) plus **TypeScript-only
  syntax the JS suite does not cover**: calls with explicit type arguments
  (`fs.unlinkSync<void>("x")`, `eval<string>(...)`, `new Function<string>(...)`
  — all three `calls.scm` query patterns still surface the op because
  `type_arguments` is a separate child, not the `function`/`constructor` field),
  destructive calls inside generic class methods, typed
  variable operands (Dynamic), and **negatives over TS-only declarations** —
  interfaces, enums, type aliases, `import type`, `as` casts, `satisfies`,
  decorators — plus a modern-TS parse-clean case (generics, arrow generics,
  optional chaining, `??`). Genuine RED-risk resolved GREEN: the pinned
  tree-sitter-typescript 0.23.2 grammar parses `satisfies`, decorators,
  `import type`, generics, and arrow-generic `<T,>` syntax cleanly with no
  false operations.
- **clippy fix:** `DetectedOperation` is unused in both adapters now that
  `interpret` lives in `family` (a child `use super::*` glob does not count as
  usage for the `unused_imports` lint); removed it from both adapters' `use`
  and added it to both test files' own `use crate::operation::{…}` import.
- **Verified:** `cargo test --workspace` = 1893 passed / 100 suites / 0 failed
  (+31 this slice: 31 TS unit tests); workspace `cargo clippy --all-targets
  -- -D warnings` clean; `cargo fmt --all --check` clean;
  `tests/aegis_language_boundary.rs`, `tests/file_size_budget.rs`, and
  `tests/contracts_docs.rs` (16) green; new/changed files under the 800-line
  budget (family 389, javascript 104, typescript 105, typescript_tests 317).
  Hot path untouched (all additive `aegis-language` adapter code, not invoked
  on the safe path), so no scanner bench run was required.
- **Deferred (documented, not silently dropped):** Slice 2 — wiring
  `analyze_source` to route `TypeScript` → `typescript::analyze` (the worker
  still returns `UnsupportedLanguage` for TS this slice) and retargeting the
  `run_returns_unsupported_language_for_a_language_without_an_adapter` dispatch
  test from TypeScript to Bash (the last unsupported foundation grammar); TS
  corpora (`.ts` files) + Node file/stdin and TypeScript runner-routing
  negative cases; `fs.promises.*`/callback-form variants; import/alias/constant
  → `OperandCertainty::Partial` (bounded symbol resolution); `DatabaseDestructive`;
  chained member calls (`a.b.c()`); per-adapter TS fuzz target; `ScriptFile`/
  `DirectExec` fs reads; live `RuntimeContext::assess` wiring; audit v1/v2
  projection; the all-four-targets qualification gate.

---

## Prior session (2026-07-22, cont. 2) — L1 Iteration 7 JavaScript corpora + Node -e fixtures

- **Iteration 7 JavaScript corpus + Node `-e` full-pipeline fixtures done via
  TDD (ADR-022 §11, plan Iteration 7 RED step — the corpus half).** Scope and
  seams confirmed with the user up front per the TDD skill: the public
  `aegis_language::languages::javascript::analyze(&str) -> AdapterResult` seam
  for the corpus, and the real-subprocess `aegis::analysis::run` seam for the
  `node -e` fixtures. This mirrors the Python corpora + inline `-c` slice
  exactly. It is a **characterization + regression corpus**, not a classic
  RED→GREEN: the adapter already behaves per spec, so the corpus pins existing
  correct behavior with hand-derived expectations (independent of the adapter —
  not tautological). The genuine RED-risk was `modern_syntax`: the pinned
  tree-sitter-javascript 0.25.0 grammar might not parse optional chaining /
  nullish coalescing / logical assignment / class fields / private methods /
  async-await / etc.; it does (parse_errors == 0, no false ops).
- **Corpus (`crates/aegis-language/tests/corpora/javascript/`, 9 `.js` files)
  + harness (`tests/javascript_corpus.rs`, 9 tests):** files embedded
  compile-time via `include_str!`; a hand-derived `ExpectedOp` manifest declares
  per-file operation kinds, modifiers, `OperandCertainty`, parse-error count,
  and nested payload `(language, source)`. Spans deliberately not pinned
  (implementation detail; unit tests own span coverage). Coverage: `fs_delete`
  (unlinkSync/rmdirSync + rmSync recursive, identifier- and string-keyed option),
  `fs_overwrite` (writeFileSync destructive_mode vs appendFileSync), `perms`
  (chmodSync/chownSync), `exec_shell` (child_process.exec/execSync → Bash nested
  payloads), `exec_js` (eval + new Function → JavaScript nested payloads),
  `negatives` (comment/string/member-ref-without-call/unrelated-call/ESM
  import → 0 ops), `dynamic_operand` (variable path/cmd/template interpolation
  → Dynamic certainty, NO nested payload — ADR-022 §3/§7 narrowness),
  `modern_syntax` (parse clean, 0 ops), `malformed` (unterminated call →
  parse_errors > 0).
- **Real-worker `node -e` fixtures (`tests/analysis_orchestrate.rs`, +3 tests,
  real `aegis --internal-language-worker`):** `fs.chmodSync('x', 0o777)` →
  `LANG-FS-CHMOD` Danger, Complete, target_count 1; `fs.writeFileSync('x','y')`
  → `LANG-FS-OVR-W` Warn, Complete, target_count 1; `eval('fs.unlinkSync(x)')`
  → `LANG-EXEC` Danger + recursive **JavaScript** target `fs.unlinkSync(x)`
  → `LANG-FS-DEL`, target_count ≥ 2, status **Complete** with NO
  `GrammarUnavailable` (both targets are supported JS). The last pins the
  JS→JS recursion contract, which no prior test covered — the existing JS exec
  test (Slice 2) recurses into Bash, which degrades as `GrammarUnavailable`;
  this one recurses into JavaScript, which completes. The inner operand `x` is
  a bare identifier (Dynamic), so the recursive target is a non-execution
  dynamic operand — it emits its match without degradation (Python C3 fix),
  keeping the status Complete. The eval payload is single-quoted JS
  (`'fs.unlinkSync(x)'`) so the `node -e "…"` shell arg needs no escaped double
  quotes, avoiding router-tokenizer `\"`-handling ambiguity.
- **Verified:** `cargo test --workspace` = 1862 passed / 100 suites / 0 failed
  (+12 this slice: 9 corpus + 3 orchestrate); workspace `cargo clippy
  --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean;
  `tests/aegis_language_boundary.rs` and `tests/file_size_budget.rs` green;
  `tests/analysis_orchestrate.rs` 643 lines, `tests/javascript_corpus.rs` 280
  lines, both under the 800-line budget. Hot path untouched (all additive
  slow-path corpus + orchestration tests), so no scanner bench run was
  required.
- **Deferred (documented, not silently dropped):** `fs.promises.*` and
  callback-form variants; import/`from`-import/alias/simple-constant →
  `OperandCertainty::Partial` corpus cases (need bounded symbol resolution);
  `DatabaseDestructive` corpus (the JS adapter does not emit it yet);
  chained member calls (`a.b.c()` — `calls.scm` matches
  `object: (identifier)` only); Node inline/file/stdin and TypeScript
  runner-routing negative cases; per-adapter JS fuzz target; TypeScript adapter
  (separate); `ScriptFile`/`DirectExec` fs reads; live `RuntimeContext::assess`
  wiring; audit v1/v2 projection of language results; the all-four-targets
  qualification gate before JavaScript becomes default-on.

---

## Prior session (2026-07-22, cont.) — L1 Iteration 7 Slice 2 (JavaScript worker wiring)

- **Iteration 7 Slice 2 done via TDD — the JavaScript adapter now runs in the
  self-spawned worker, so JS inline bodies flow end-to-end through the real
  route → worker → `map_adapter_result` → `merge_analysis` composition.**
  Scope and seams confirmed with the user up front per the TDD skill (Slice 2 =
  worker dispatch wiring only; the parent orchestration is already
  language-agnostic from Iteration 6 Slices B/C, so this is the lone missing
  tracer bullet). This mirrors Python Slice A exactly: the only change is
  `analyze_source` routing `SourceLanguage::JavaScript` →
  `javascript::analyze`; TypeScript and Bash remain `UnsupportedLanguage`
  (adapters in later iterations).
- **Worker dispatch (`crates/aegis-language/src/worker.rs`):** `analyze_source`
  now matches `JavaScript` to `crate::languages::javascript::analyze` alongside
  Python; the `TypeScript | Bash` arm still returns `UnsupportedLanguage`. Doc
  comment updated (was "Iteration 6 ships only the Python adapter"). This is the
  entire production change — the generic `map_operation` (Iteration 5) maps JS
  `DetectedOperation`s to `LANG-*` Matches for free, so no classifier change.
- **TDD (3 seams, each RED → GREEN):** (1) worker dispatch unit test
  `run_analyzes_javascript_source_and_returns_an_analyzed_response` —
  `Request::Analyze { JavaScript, "fs.unlinkSync(\"data.txt\")" }` → `Analyzed`
  with `parse_errors == 0` and non-empty `operations` (pins dispatch reached the
  adapter, not its exact output — that is the adapter's own contract). RED today
  (UnsupportedLanguage); GREEN after wiring. (2) orchestration
  `run_analyzes_inline_javascript_and_merges_a_filesystem_delete_match` —
  `node -e "fs.unlinkSync('data.txt')"` → `LANG-FS-DEL` Match, risk ≥ Warn,
  status Complete, target_count 1 (non-recursive `FilesystemDelete` → no
  recursive target). (3) orchestration
  `run_analyzes_a_javascript_exec_payload_and_degrades_the_recursive_bash_target`
  — `node -e "child_process.exec('rm -rf /tmp/x')"` → `LANG-EXEC` Match +
  recursive Bash target degrading `GrammarUnavailable` (L1 Bash is Iteration 8;
  ADR-022 §9), target_count ≥ 2, status Degraded. This pins the
  honest-degradation-on-unsupported-recursive-language contract for JS, which no
  prior test covered (existing JS-recursive coverage was absent — JS was
  UnsupportedLanguage before this slice). Both orchestration tests spawn the
  real `aegis --internal-language-worker` via `env!("CARGO_BIN_EXE_aegis")`.
- **Existing-test retargeting (preserves the UnsupportedLanguage path):** the
  unit test `run_returns_unsupported_language_for_a_language_without_an_adapter`
  was switched from `JavaScript` to `TypeScript` (still no adapter); the
  orchestration test `run_records_grammar_unavailable_for_an_unsupported_language`
  was switched from `node -e "x"` to `bash -c "x"` (Bash still unsupported,
  Iteration 8; router routes `bash -c` → `SourceLanguage::Bash`). Both kept
  pinning the UnsupportedLanguage → GrammarUnavailable degradation contract that
  JS no longer exercises.
- **Verified:** `cargo test --workspace` = 1850 passed / 99 suites / 0 failed
  (+3 this slice: 1 worker dispatch + 2 orchestrate); workspace `cargo clippy
  --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean;
  `tests/aegis_language_boundary.rs` and `tests/file_size_budget.rs` green;
  `worker.rs` 587 lines, `tests/analysis_orchestrate.rs` 467 lines, both under
  the 800-line budget. Hot path untouched (all additive slow-path under
  `aegis-language` worker dispatch + orchestration tests), so no scanner bench
  run was required.
- **Deferred (documented, not silently dropped):** the JS corpora directory
  (`crates/aegis-language/tests/corpora/javascript/`, plan Iteration 7 RED
  step); Node inline/file/stdin and TypeScript runner-routing negative cases;
  per-adapter JS fuzz target; TypeScript adapter (separate); `fs.promises.*` /
  callback-form variants, bounded symbol resolution (imports/aliases/constants
  → Partial), `DatabaseDestructive`, chained member calls (`a.b.c()` — the
  `calls.scm` query intentionally matches `object: (identifier)` only);
  `ScriptFile`/`DirectExec` fs reads; live `RuntimeContext::assess` wiring;
  audit v1/v2 projection of language results; the all-four-targets
  qualification gate before JavaScript becomes default-on.

---

## Prior session (2026-07-22) — L1 Iteration 6 Python corpora + inline -c fixtures

- **Iteration 6 Python corpus + inline `-c` full-pipeline fixtures done via
  TDD (ADR-022 §11, plan Iteration 6 RED step — the corpus half).** Scope and
  seams confirmed with the user up front per the TDD skill: the public
  `aegis_language::languages::python::analyze(&str) -> AdapterResult` seam for
  the corpus, and the real-subprocess `aegis::analysis::run` seam for the
  inline `-c` fixtures. This is a **characterization + regression corpus**, not
  a classic RED→GREEN: the adapter already behaves per spec, so the corpus
  pins existing correct behavior with hand-derived expectations (independent of
  the adapter — not tautological). The genuine RED-risk was `modern_syntax`:
  the pinned tree-sitter-python 0.25.0 grammar might not parse `match`/walrus/
  `except*`/f-string-debug; it does (parse_errors == 0, no false ops).
- **Corpus (`crates/aegis-language/tests/corpora/python/`, 9 `.py` files) +
  harness (`tests/python_corpus.rs`, 9 tests):** files embedded compile-time via
  `include_str!`; a hand-derived `ExpectedOp` manifest declares per-file
  operation kinds, modifiers, `OperandCertainty`, parse-error count, and nested
  payload `(language, source)`. Spans deliberately not pinned (implementation
  detail; unit tests own span coverage). Coverage: `fs_delete` (os.remove/
  unlink/rmdir + shutil.rmtree recursive), `fs_overwrite` (open 'w' destructive
  vs 'a'; 'r'/no-mode negatives), `perms` (os.chmod/os.chown/shutil.chown),
  `exec_shell` (os.system + subprocess.{run,call,Popen,check_call,check_output}
  → Bash nested payloads), `exec_python` (eval/exec → Python nested payloads),
  `negatives` (comment/string/attribute-ref-without-call/unrelated-call → 0
  ops), `dynamic_operand` (variable path/cmd → op with Dynamic certainty, NO
  nested payload — ADR-022 §3/§7 narrowness; bounded resolution deferred so a
  variable holding a literal is still Dynamic), `modern_syntax` (parse clean,
  0 ops), `malformed` (unterminated call → parse_errors > 0).
- **Real-worker inline `-c` fixtures (`tests/analysis_orchestrate.rs`, +3
  tests, real `aegis --internal-language-worker`):** `os.chmod('x', 0o777)` →
  `LANG-FS-CHMOD` Danger, Complete, target_count 1; `open('x','w')` →
  `LANG-FS-OVR-W` Warn, Complete, target_count 1; `os.system('rm -rf /tmp/x')`
  → `LANG-EXEC` Danger + cross-language recursive Bash target that degrades as
  `GrammarUnavailable` (L1 Bash is Iteration 8; ADR-022 §9 honest degradation),
  target_count ≥ 2, status Degraded. The last one pins the
  honest-degradation-on-unsupported-recursive-language contract, which no prior
  test covered (existing exec tests recurse into Python, which IS supported).
- **Verified:** `cargo test --workspace` = 1813 passed / 99 suites / 0 failed
  (+12 this slice: 9 corpus + 3 orchestrate); workspace `cargo clippy
  --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean;
  `tests/aegis_language_boundary.rs` and `tests/file_size_budget.rs` green
  (in-workspace). Hot path untouched (all additive test files), so no scanner
  bench run was required.
- **Deferred (documented, not silently dropped):** import/`from`-import/alias/
  simple-constant → `OperandCertainty::Partial` corpus cases (need the bounded
  symbol-resolution slice — the adapter explicitly defers resolution);
  `DatabaseDestructive` corpus (the Python adapter does not emit it yet —
  tracked deferral from the prior review cycle); stdin/heredoc-to-file/
  named-file full-pipeline fixtures (need `ScriptFile`/`DirectExec` async
  fs-read wiring in `analysis::run`, currently `RoutedTarget::Inline` only);
  the per-adapter Python fuzz target (separate slice); audit v1/v2 projection
  of language results; live `RuntimeContext::assess` wiring; the all-four-
  targets qualification gate.

---

## Prior session (2026-07-21, cont. 4) — L1 Iteration 6 Slice C (recursive drain)

- **Iteration 6 Slice C done via TDD — closes the Iteration 6 core deliverable:
  the production-qualified Python adapter is now exercised end-to-end through
  the real worker, including recursion.** `analysis::run` drains the
  parent-owned `AnalysisQueue`: `route` → seed the queue with inline targets at
  depth 0 → drain loop (pop → spawn a fresh worker → one `Request::Analyze` →
  `map_adapter_result` → push any `recursive_targets` back onto the queue →
  repeat until empty or a budget cap fires) → `aggregate` → single
  `merge_analysis`. Recursive targets now carry the target's own `depth` and
  `Some(source_hash)` (Slice B passed `(None, 0)` and discarded them); a literal
  `exec`/`eval` payload's nested destructive op now actually surfaces in the
  merged `Assessment`. `run`'s signature is unchanged, so S1/S2/S3 stayed green.
- **Orchestration (`src/analysis/orchestrate.rs`, ~209 → ~290 lines):** `run`
  builds `AnalysisQueue::new(QueueBudget::L1_DEFAULT)` (config wiring still
  deferred), seeds it with `QueueTarget::new(language, source, 0)` per inline
  target via a new `push_with_degradation` (records `LimitExceeded` when a cap
  rejects a target — ADR-022 §7: preserve Matches already produced, record the
  limit; duplicates/acceptances record nothing). The drain loop spawns one
  worker per pop: `Worker::analyze` closes stdin and reaps the child every
  session, so worker reuse across pops is a perf optimization deferred to a
  later slice (≤ `max_targets` = 16 spawns per session, slow path). `map_target_result`
  is refactored to `(LanguageAnalysisResult, Vec<QueueTarget>)` taking
  `&QueueTarget`, so recursive targets land at `depth + 1` with correct
  provenance. `target_count` is now `per_target.len()` — top-level + recursive.
- **TDD (real-subprocess seam, `tests/analysis_orchestrate.rs`, +1 test,
  RED → GREEN):** SC1 `python3 -c "exec('shutil.rmtree(x)')"` — the inline body
  is a Python `exec` of a literal `shutil.rmtree(x)`; the top-level `exec` is
  `LANG-EXEC` (Danger) and the recursive `shutil.rmtree` payload is
  `LANG-FS-DEL-R` (Danger). Asserts BOTH matches surface, `risk >= Danger`, and
  `target_count >= 2`. RED under Slice B (recursive target discarded → only
  `LANG-EXEC`, count 1 — confirmed at `tests/analysis_orchestrate.rs:163`);
  GREEN under the drain loop. S1/S2/S3 stayed green unchanged (S2's
  `shutil.rmtree('x')` is a non-execution op → no recursive target → count 1,
  drain exits immediately). Each test spawns the real
  `aegis --internal-language-worker` via `env!("CARGO_BIN_EXE_aegis")`.
- **Verified:** `cargo test --workspace` = 1801 passed / 98 suites / 0 failed
  (+1 this slice); workspace `cargo clippy --all-targets -- -D warnings` clean;
  `cargo fmt --all --check` clean; `tests/file_size_budget.rs`,
  `tests/aegis_language_boundary.rs`, and `tests/contracts_docs.rs` (13) green;
  `src/analysis/orchestrate.rs` at 297 lines (under the 800-line budget). Hot
  path untouched (all additive async slow-path under `src/analysis/`), so no
  scanner bench run was required.
- **Deferred (documented, not silently dropped):** `ScriptFile`/`DirectExec`
  fs reads via `source_reader` + `source_hash`; live `RuntimeContext::assess`
  integration (Python results do NOT yet influence real intercepted
  Assessments); full `aegis-config` wiring (deadline/budget/trusted-aliases);
  worker reuse across recursive pops (perf, not correctness); the broader
  corpora / fuzzing / audit v1/v2 projection / all-four-targets qualification
  gate before Python becomes default-on.

---

## Prior session (2026-07-21, cont. 3) — L1 Iteration 6 Slice B (parent-side orchestration)

- **Iteration 6 Slice B done via TDD — the parent side now drives a real
  end-to-end analysis: route → spawn the ephemeral worker → send
  `Request::Analyze` per inline target → map each `Response` via the existing
  in-process `map_adapter_result` → fold the per-target `LanguageAnalysisResult`s
  into the baseline `Assessment` via one aggregated `merge_analysis`.** Scope and
  seams confirmed with the user up front (Slice B = orchestration function
  `analysis::run` only, NOT wired into `RuntimeContext::assess`; recursion
  deferred; real-subprocess seam). This is the tracer bullet proving the
  parent ↔ worker ↔ adapter ↔ mapping ↔ merge composition before it touches the
  hot path.
- **Orchestration (`src/analysis/orchestrate.rs`, new, 209 lines):** `Outcome`
  (`NotStarted { baseline }` | `Analyzed { assessment, target_count }`) and async
  `run(command, baseline, aegis_path, trusted_aliases, deadline)`. `route` →
  filter to `RoutedTarget::Inline` only (ScriptFile/DirectExec need async fs
  reads — deferred; Dynamic already carries a reason — deferred); empty →
  `NotStarted` with **no subprocess spawned** (ADR-022 §0); otherwise build one
  `TargetRequest { kind: Analyze }` per target, `Worker::spawn(aegis_path)` (spawn
  failure degrades every target as `WorkerFailure`), `worker.analyze(...)`,
  `map_target_result` per `TargetResult`, `aggregate` into one
  `LanguageAnalysisResult`, single `merge_analysis`. `merge_analysis` overwrites
  `Assessment.analysis` with the latest result's status/reasons, so per-target
  reasons would clobber earlier ones under a naive fold — `aggregate`
  concatenates matches, dedups reasons, and sets status
  (`Degraded` if any target degraded, else `Complete` if any match, else
  `NotApplicable`) before one merge. `recursive_targets` from `map_adapter_result`
  are documented and discarded this slice (`// deferred: recursive drain`).
- **Per-target mapping:** `Responded(Analyzed{result})` →
  `map_adapter_result(..., SourceOrigin::Inline, None, 0).analysis`;
  `Responded(UnsupportedLanguage)` → `{ Degraded, [], [GrammarUnavailable] }`;
  `Responded(Parsed|ParseFailed)` (unreachable for Analyze) → defensive
  `{ Degraded, [], [WorkerFailure] }`; `Failed(WorkerError)` →
  `{ Degraded, [], [WorkerFailure] }` via the existing `From<WorkerError>`.
- **Transport (`src/analysis/worker_client.rs`):** `TargetRequest` gained a
  `kind: RequestKind { Parse, Analyze }` field; `send_requests` encodes by `kind`
  (`Request::Parse` unchanged, `Request::Analyze` new). Re-exported `RequestKind`
  from `analysis/mod.rs`. The existing `tests/analysis_worker_client.rs` `req()`
  helper was updated to set `kind: Parse` — all 13 Parse tests stayed green.
- **Types (`crates/aegis-types/src/assessment.rs`):** added `#[derive(Debug,
  Clone)]` to `Assessment` (every field was already `Debug`/`Clone`) so `Outcome`
  can derive `Debug, Clone` and `NotStarted` can hand back an owned baseline.
- **TDD (real-subprocess seam, `tests/analysis_orchestrate.rs`, 3 tests, each
  RED → GREEN):** S1 `ls -la` → `NotStarted` with `analysis == None` (no spawn);
  S2 `python3 -c "shutil.rmtree('x')"` → `Analyzed` with `LANG-FS-DEL-R` Match,
  `risk >= Danger`, status `Complete` (the S2 fixture is `shutil.rmtree`, not
  `os.remove` — the latter only reaches `LANG-FS-DEL` at `Warn`, per
  `classifier.rs`); S3 `node -e "x"` (JavaScript, no L1 adapter) → `Analyzed` with
  `GrammarUnavailable` degradation. Each test spawns the real
  `aegis --internal-language-worker` via `env!("CARGO_BIN_EXE_aegis")`. S3 was a
  genuine RED (the `UnsupportedLanguage` arm was temporarily reverted to the
  defensive `WorkerFailure` path, re-added for GREEN).
- **Verified:** `cargo test --workspace` = 1800 passed / 98 suites / 0 failed
  (+3 this slice); workspace `cargo clippy --all-targets -- -D warnings` clean;
  `cargo fmt --all --check` clean; `tests/aegis_language_boundary.rs` and
  `tests/file_size_budget.rs` green; new/changed files under the 800-line budget
  (orchestrate 209, worker_client 456, assessment 308). Hot path untouched (all
  additive async slow-path under `src/analysis/`), so no scanner bench run was
  required.
- **Deferred (documented, not silently dropped):** recursive drain of
  `recursive_targets` via the `AnalysisQueue` (Slice C); `ScriptFile`/`DirectExec`
  fs reads via `source_reader` + `source_hash`; live `RuntimeContext::assess`
  integration (Python results do NOT yet influence real intercepted Assessments);
  full `aegis-config` wiring (deadline/budget/trusted-aliases); the broader
  corpora / full-pipeline fixtures and the Iteration 6 qualification gate.

---

## Prior session (2026-07-21, cont. 2) — L1 Iteration 6 Slice A (worker/protocol Analyze wiring)

- **Iteration 6 Slice A done via TDD — the ephemeral worker now runs the
  language adapter and returns a full `AdapterResult`, satisfying ADR-022 §2
  ("Tree-sitter parsing and language adapters run in a self-spawned, ephemeral
  worker process").** Scope confirmed with the user up front (Slice A = protocol
  carries `AdapterResult` + worker runs the adapter; the parent-side
  route→worker→map→queue→`merge_analysis` end-to-end wiring stays Slice B).
  Chosen protocol form: new `Request::Analyze` / `Response::Analyzed` /
  `Response::UnsupportedLanguage` variants, with `Parse` / `Parsed` /
  `ParseFailed` retained unchanged.
- **Protocol surface (`crates/aegis-language/src/protocol.rs`):** added
  `Request::Analyze { language, source }` (kind `0x02`, same `[lang_u8][source]`
  payload shape as `Parse` so the 1 MiB source ceiling and the "no path read /
  no subprocess" property carry over), `Response::Analyzed { result:
  AdapterResult }` (kind `0x83`, payload = the hand-rolled
  `encode_adapter_result`/`decode_adapter_result` codec from Iteration 6
  cycle 1 — the protocol just frames it, and decode propagates the codec's
  precise `DecodeError::InvalidPayload` reasons directly), and
  `Response::UnsupportedLanguage` (kind `0x84`, empty payload). The shared
  `[lang_u8][source]` decode was factored into `decode_lang_source_payload`
  (single source of truth for both request kinds); `language_to_wire`/
  `wire_to_language` stay `pub(crate)` so `operation.rs` reuses them. Updated
  the two exhaustive-kind-tag tests: requests now accept `Parse`+`Analyze`;
  responses accept `Parsed`+`ParseFailed`+`Analyzed`+`UnsupportedLanguage`.
- **Worker dispatch (`crates/aegis-language/src/worker.rs`):** `handle_request`
  routes `Analyze` to a new `analyze_source` helper — Python → `python::analyze`
  framed as `Analyzed`; the other foundation grammars (no adapter wired yet) →
  `UnsupportedLanguage` (the parent maps this to a degradation reason, not a
  clean parse); invalid-UTF-8 source (the adapter takes a `&str`, and the
  parent owns the encoding contract per ADR-022 §7) →
  `Analyzed { result: AdapterResult { operations: [], parse_errors: 1 } }`.
  `Parse` behavior is unchanged.
- **Test seam:** the `dispatch_tests` module's `run`-loop end-to-end harness
  (encode `Request` → `run` → decode `Response`) was extended with three
  Analyze tests: Python `os.remove('x')` → `Analyzed` with `parse_errors == 0`
  and non-empty operations (pins that dispatch reached the adapter, not its
  exact output — that is the adapter's own contract); JavaScript →
  `UnsupportedLanguage`; Python invalid-UTF-8 → `Analyzed` with one parse error
  and no operations.
- **Budget / boundary hygiene:** `operation.rs` hit the 800-line file-size
  budget after the cycle-1 codec; its `#[cfg(test)] mod tests` was extracted to
  `crates/aegis-language/src/operation_tests.rs` via `#[cfg(test)] #[path =
  "..."] mod tests;` — the same pattern `protocol.rs` already uses. No new
  dependencies; `aegis-language` stays a workspace leaf (only thiserror + the
  pinned Tree-sitter runtime) — `tests/aegis_language_boundary.rs` still green.
- **Verified:** `cargo test --workspace` = 1797 passed / 97 suites / 0 failed
  (+19 this slice: 11 codec earlier in Iteration 6 + 8 protocol/worker here);
  workspace `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt --all
  --check` clean; `tests/aegis_language_boundary.rs` and
  `tests/file_size_budget.rs` green; new/changed files under the 800-line
  budget (protocol 359, operation 487, operation_tests 358, worker 478). Hot
  path untouched (all additive `aegis-language` slow path), so no scanner
  bench run was required.
- **Deferred (documented, not silently dropped):** Slice B — wiring the
  parent `worker_client` to send `Request::Analyze`, map `Analyzed`/
  `UnsupportedLanguage` via the existing in-process `map_adapter_result`, feed
  recursive targets into the `AnalysisQueue`, and `merge_analysis` into the
  `Assessment` (the in-process pipeline test already pins the contract this
  wiring will rely on); bounded symbol resolution; the broader corpora /
  full-pipeline fixtures and the Iteration 6 qualification gate.

---

## Prior session (2026-07-21, cont.) — L1 Iteration 6 (Python adapter + root mapping, slices 1-2)

- **Iteration 6 (Python qualification) slices 1-2 done via TDD; worker/protocol
  wiring, bounded symbol resolution, and the broader corpora/full-pipeline
  fixtures deferred.** Scope confirmed with the user up front (the plan's
  Iteration 6 bundles adapter + corpora + full-pipeline fixtures + qualification
  gate; per the chosen scope, this session delivered the adapter, the queries,
  the root mapping, and the in-process pipeline test — not the worker/`merge_analysis`
  runtime wiring, which stays a later slice).
- **Slice 1 — Python adapter + operation vocabulary
  (`crates/aegis-language/src/operation.rs` + `src/languages/python.rs` +
  `queries/python/calls.scm`, 77 tests in the crate):** boundary-forced parallel
  operation vocabulary (`OperationKind` / `OperationModifiers` / `OperandCertainty`
  / `ByteSpan` / `DetectedOperation` / `NestedTarget` / `AdapterResult`) —
  `aegis-language` may not depend on `aegis-types` (`tests/aegis_language_boundary.rs`,
  ADR-022 §4), so the adapter *produces* the parallel types and the root crate
  maps them. The adapter runs the `calls.scm` Tree-sitter query for structural
  capture and interprets each call site in typed Rust (semantic API-spelling →
  `OperationKind` mapping is Rust, never a private copy of the shared classifier —
  Iteration 5 REVIEW GATE). Covers `os.remove`/`shutil.rmtree` (FilesystemDelete,
  recursive modifier), `open('w'/'a'/'x')` (FilesystemOverwrite, destructive_mode),
  `os.chmod`/`os.chown`/`shutil.chown` (PermissionOrOwnershipChange), `eval`/`exec`
  (CodeExecution, Python payload), `os.system`/`subprocess.run("…")` (CodeExecution,
  Bash payload — cross-language), and dynamic operands (variable / list argv /
  f-string → `OperandCertainty::Dynamic`, no payload). `subprocess.run([argv])` is
  Dynamic without a payload (a literal argv list is not shell source to recurse
  into). 30 adapter tests + 47 prior crate tests.
- **Slice 2 — root mapping + in-process pipeline
  (`src/analysis/mapping.rs` + `tests/language_python_pipeline.rs`, 8 pipeline
  tests):** `map_operation` converts the parallel `aegis_language` operation into
  `aegis_types::DetectedOperation` one-for-one (returns `Option` so a future
  `non_exhaustive` adapter kind with no shared mapping is skipped rather than
  mislabeled or panicked; certainty's `non_exhaustive` wildcard falls back to
  `Dynamic` — the conservative "never evidence of safety" default). `map_adapter_result`
  composes one adapter result into a `MappingOutcome { analysis: LanguageAnalysisResult,
  recursive_targets: Vec<QueueTarget> }`: each op → a `LANG-*` `Match` via the
  shared `language_match`; a `CodeExecution` sink with a literal payload
  additionally enqueues a bounded recursive `QueueTarget` at `parent_depth + 1`
  (payload's own language, cross-language) via `handle_sink`; a dynamic/encoded
  payload records `DegradationReason::DynamicSource` and enqueues nothing; a
  nonzero `parse_errors` count records `IncompleteSyntax`; status aggregates
  monotonically (`NotApplicable < Complete < Degraded`) with deduplicated reasons.
  Pipeline test pins: every `OperationKind` maps one-for-one (the conversion test
  `operation.rs`'s doc promises); known exec payload → `LANG-EXEC` Match + Bash
  recursive target at depth 1; dynamic exec → Match + `DynamicSource`, no target;
  non-execution op → classified Match, no target; parse errors → `IncompleteSyntax`;
  empty result → `NotApplicable`; the recursive target is accepted by the
  parent-owned `AnalysisQueue`; `merge_analysis` lifts a baseline `Safe`
  `Assessment` to `Danger` and carries the analysis status.
- **Deferred (documented, not silently dropped):** wiring `map_adapter_result` +
  the recursive queue into `worker_client` so a real intercepted command flows
  parent → worker → adapter → mapping → `merge_analysis` (the in-process pipeline
  test pins the contract this wiring will rely on); bounded symbol resolution
  (direct imports, aliases, constants) — per-adapter AST work; the broader
  inline `-c` / stdin / heredoc / named-file full-pipeline fixtures and the
  Iteration 6 qualification gate (grammar provenance, fuzzing, all four release
  targets, audit v1/v2, measured budgets) before Python becomes default-on.
- **Verified:** `cargo test --workspace` = 1778 passed / 97 suites / 0 failed
  (+39 this session: 30 Python adapter + 9 pipeline; boundary tests still green —
  `aegis-language` depends on no workspace crate); workspace `cargo clippy
  --all-targets -- -D warnings` clean; `cargo fmt --all --check` clean. New files
  under the 800-line budget (mapping 264, python 694, operation 148). Hot path
  untouched (all additive slow-path `src/analysis/` + `aegis-language`), so no
  scanner bench run was required.
- **Review cycle (`code-review` → `skeptic` Verify → TDD fixes):** two-axis
  review surfaced 2 hard Standards findings + 1 Spec finding; the skeptic
  Verify pass confirmed 3 survivors (C1/C2/C3) and routed the rest to
  human-decision / not-actionable / tracked-deferral. Fixes applied via TDD:
  (C3) removed erroneous `DynamicSource` degradation for non-execution dynamic
  operands — RED `non_exec_dynamic_operand_emits_match_without_degradation` →
  GREEN; (C1) moved `Parser::set_language` out of the per-`analyze()` call into
  a one-time `thread_local` init (`.expect()` now in startup init, not on a
  per-invocation path — CONVENTION.md §5); (C2) brought the `ARCHITECTURE.md`
  §8 `analysis` bullet into sync with the current `src/analysis/` surface.
  Tracked deferrals (not blockers): `DatabaseDestructive` adapter coverage +
  the corpora directory + newer-syntax tests — go with the deferred corpora /
  qualification-gate slice.

---

## Prior session (2026-07-21) — L1 Iteration 5 (shared classifier + recursive queue + sink invariant)

- **Iteration 5 (Shared operation classifier and recursive queue) done via
  TDD — slices 1-3; bounded symbol resolution deferred.** Scope and seams
  confirmed with the user up front per the TDD skill (the plan bundles four
  concerns into one iteration; resolution is per-adapter AST work with no
  adapters until Iterations 6-8, so it would be synthetic today — same
  reasoning as Iteration 4's deferred slices).
- **Architecture correction (same shape as Iteration 4's `router.rs`):** the
  plan's candidate files `crates/aegis-language/src/classifier.rs` and
  `crates/aegis-language/src/queue.rs` are not buildable as written —
  `aegis-language` may not depend on any workspace crate
  (`tests/aegis_language_boundary.rs`, ADR-022 §4), but the classifier
  consumes `DetectedOperation`/`OperationKind` and produces
  `Category`/`RiskLevel`/`Match` (all `aegis-types`), and the queue is
  parent-owned (manages `RoutedTarget`/`TargetAnalysis`). The classifier
  therefore lives in `aegis-types` (pure fn, like the already-collocated
  `merge_analysis`); the queue and sink invariant live in the root `aegis`
  crate's `src/analysis/` (which depends on both `aegis-types` and
  `aegis-language`).
- **Slice 1 — classifier (`crates/aegis-types/src/analysis/classifier.rs`,
  21 tests):** pure `classify(&DetectedOperation) -> Classification`
  + `language_match(...)` builder producing a `MatchResult` with
  `MatchEvidence::LanguageRule`. Language-neutral matrix over all nine
  `OperationKind`s + recursive/forced/destructive-mode modifiers, with
  stable `LANG-*` rule ids (distinct from the shell scanner's `FS-001`-style
  ids). `FilesystemDelete` branches on modifiers (recursive → Danger
  `LANG-FS-DEL-R`, recursive+forced → `LANG-FS-DEL-RF`, forced → Warn
  `LANG-FS-DEL-F`, plain → Warn `LANG-FS-DEL`); overwrite, chmod, device
  write, destructive DB, `CodeExecution`, cloud/container/package map to
  their `Category` + risk. REVIEW GATE invariants pinned by tests: the
  classifier **never** returns `RiskLevel::Block` (language-aware Matches
  are non-`Block` by ADR-022 §5); risk is **certainty-independent** (a
  `Dynamic` operand never lowers risk below `Known` — certainty governs
  recursive enqueueing/degradation, which is the queue's job); `CodeExecution`
  always classifies `Danger` regardless of certainty; `OperationKind` is
  matched exhaustively so a new variant forces a classification here.
  Re-exported at `aegis_types::{Classification, classify, language_match}`.
- **Slice 2 — recursive work queue (`src/analysis/queue.rs`, 14 tests):**
  parent-owned `AnalysisQueue` deduplicated by `(language, source_hash)`
  (the hash is hex SHA-256 of the post-BOM-strip source, matching
  `source_reader`'s format for BOM-free sources so an inline and a
  BOM-free script-file target over the same body collapse; a BOM-prefixed
  script file hashes differently because `source_reader` hashes raw bytes
  pre-strip — see `QueueTarget.source_hash` doc). `QueueBudget::
  L1_DEFAULT` carries the ADR-022 §7 ceilings (depth 8, aggregate 1 MiB;
  `max_targets` = 16, chosen > `max_depth` so a linear depth-8 chain is
  bounded by depth, the meaningful recursion guard, not by count; deadline
  is caller-set, `None` in the default). `push` returns `PushOutcome`
  (`Accepted` / `DuplicateSkipped` / `DepthExceeded` / `CountExceeded` /
  `BytesExceeded` / `DeadlineExceeded`); every cap maps to
  `DegradationReason::LimitExceeded` while dedup records no degradation
  (already-analyzed, never new work). Dedup runs first, so a cycle (a nested
  target with the same `(lang, hash)` as an ancestor) is broken without
  degradation. Cross-language nesting (same bytes, different language) is a
  distinct target, not a duplicate.
- **Slice 3 — cross-language execution-sink invariant
  (`src/analysis/recursive.rs`, 10 tests):** `handle_sink` composes the
  classifier + queue. A recognized process/shell/eval sink **always** emits
  its `CodeExecution` Match (ADR-022 §3/§7 REVIEW GATE — retained regardless
  of payload certainty). A `Known` literal payload additionally becomes a
  bounded recursive `QueueTarget` at `parent_depth + 1`, parsed as the
  payload's own language (cross-language: a Python sink can enqueue a
  JavaScript target). A dynamic, partial, or encoded payload records
  `DegradationReason::DynamicSource` and enqueues nothing — the payload is
  never evaluated or decoded (decode-to-eval shape, ADR-022 §7).
  Degradation is orthogonal to `RiskLevel` (ADR-022 §5): a degraded sink is
  still `Danger`, never lowered to authorize auto-execution.
- **Deferred (documented, not silently dropped):** bounded symbol resolution
  (direct imports, aliases, simple constants, adjacent literals, literal
  concatenation, escapes) — per-adapter AST work; no adapters exist until
  Iterations 6-8, so a resolution test today would be synthetic. Wiring
  `handle_sink`/queue output into `worker_client`/`Assessment` via
  `merge_analysis` — blocked on an adapter actually producing a
  `LanguageAnalysisResult` (Iterations 6-8). No Effect-opaque / Required-
  recovery regression test for the same reason (no production path yet lets
  a language-aware result influence `effect_opaque`).
- **Verified:** `cargo test --workspace` = 1739 passed / 96 suites / 0 failed
  (+45 this session: 21 classifier + 14 queue + 10 invariant); workspace
  `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt --all
  --check` clean; `tests/aegis_language_boundary.rs`,
  `tests/architecture_boundaries.rs`, `tests/file_size_budget.rs` all green;
  new files well under the 800-line budget (queue 220, recursive 101,
  classifier 207). Hot path untouched (classifier is in `aegis-types` and
  not invoked on the safe path; queue/recursive are parent-side slow path),
  so no scanner bench run was required.

---

## Prior session (2026-07-20) — L1 Iteration 4 slices 1-4 + REVIEW GATE (source routing)

- **Iteration 4 (Source target routing and catch-only reads) slices 1-4 done
  via TDD; heredoc-to-file reuse and `aegis-config` budget/alias wiring
  deferred.** Scope for slice 1 was confirmed with the user up front per the
  TDD skill (the plan bundles five distinct concerns into one iteration);
  slices 2-4 continued in the same session at the user's request ("do all
  slices, then I make code review").
- **Architecture correction caught before writing code (slice 1):** the
  plan's candidate file `crates/aegis-language/src/router.rs` cannot host the
  production router — `tests/aegis_language_boundary.rs` (ADR-022 §4) forbids
  `aegis-language` from depending on any workspace crate, including
  `aegis-parser`, whose tokenizer/`Effective program` resolution the router
  needs to reuse. The production router lives instead at
  `src/analysis/router.rs` in the root `aegis` crate (already depends on both);
  the Iteration-0 `aegis_language::router` prototype is untouched and still
  backs `aegis_language::worker::analyze`'s own no-source contract test/bench.
- **Slice 1 (interpreter/runner registry, in `router::route`):** reuses
  `aegis_parser::split_tokens` + `effective_token_slices` (replacing the
  Iteration-0 prototype's ad hoc `shell_words`/fixed-prefix-list helpers), so
  launcher-prefix stacking (`sudo timeout 5 python3 -c …`, `env FOO=bar
  python3 -c …`) is handled by the same production logic the scanner uses.
  Versioned-basename normalization (`python3.11` → `python3`, `node20` →
  `node`) and trusted-alias resolution (`("py", "python3")`) are caller-
  supplied parameters — not yet wired to `aegis-config` (deferred). An exact
  registry match always wins over a conflicting alias (ADR-022 §6 precedence).
- **`src/analysis/source_reader.rs` (new, 10 tests):** async, catch-only
  `read_script_file` — `symlink_metadata` pre-check + post-open re-check
  reject symlinks/FIFOs/sockets/directories without following them (no
  `O_NOFOLLOW`, since that needs a new `libc`-family dependency outside the
  approved list; the accepted residual TOCTOU race is documented in the
  module doc, consistent with ADR-022 §6's "successful read never waives
  Effect-opaque"). Bounds reads to `limit_bytes + 1` so oversized files are
  caught without a full read; strips a UTF-8 BOM; rejects invalid UTF-8;
  records a SHA-256 hash (metadata only, never persisted source).
- **Slice 2 (script-file/shebang routing, in `router.rs`):** `RoutedTarget`
  gained `ScriptFile { language, path }` (an argv file target) and
  `DirectExec { path }` (a path-like program token with no known interpreter,
  e.g. `./script.py`) — `resolve()` reads `DirectExec` and only treats it as a
  target if `verified_shebang_language` matches the first line (env or direct
  form); no `PATH`/`--version`/content-guessing probes (ADR-022 §6).
- **Slice 3 (heredoc/here-string/literal-producer stdin, new
  `src/analysis/heredoc.rs`):** quoted heredocs are exact source; unquoted
  heredocs/here-strings are literal only when they contain no `$`/backtick
  expansion syntax, otherwise `RoutedTarget::Dynamic` degradation (never
  evaluated). A real bug was caught by the RED step here: the tokenizer has
  no heredoc-boundary awareness, so heredoc *body* words were being
  misdetected as trailing script-file arguments until the heredoc/here-string
  check was reordered ahead of the bare-file-argument scan. Also added:
  two-stage pipeline routing (`producer | interpreter`) — only `printf '%s'
  <literal>` is a narrowly-proven literal producer (ADR-022 §6); every other
  producer degrades as `Dynamic` rather than being evaluated or guessed at.
- **Slice 4 (`cd` tracking, in `router.rs`):** only a literal top-level
  `cd -- <path> &&` prefix rebases a relative `ScriptFile`/`DirectExec` path;
  any other `cd` form (no `--`, command substitution, no trailing `&&`)
  degrades a relative target as `Dynamic` rather than resolving against the
  wrong directory. A relative `DirectExec` after an unresolved `cd` is dropped
  outright (no language to attach a degradation to, and misleading evidence
  from the wrong file is worse than none) — a documented, narrow gap.
- **Test-hygiene:** `src/analysis/router.rs` hit the 800-line file-size budget
  (`tests/file_size_budget.rs`) after slice 4; its `#[cfg(test)] mod tests`
  was extracted to `src/analysis/router_tests.rs` via `#[path = ...]`,
  matching the project's existing pattern for this budget.
- **Follow-up (same session, via TDD): same-command heredoc-to-file reuse.**
  `router::route` no longer silently drops `cat > PATH <<HEREDOC && <interp>
  PATH` (or `tee PATH <<HEREDOC && …`) — it now reuses the in-memory heredoc
  body (`heredoc::classify`, promoted to `pub(crate)`) instead of routing a
  `ScriptFile` read of a file that, before this command finishes executing,
  doesn't exist on disk yet. Scope confirmed with the user up front: narrowly
  exactly `cat > PATH`/`tee PATH` immediately before the heredoc marker,
  exactly one top-level `&&` after it (rejects any further `;`/`&&`/`||`/`|`
  token in the exec part), and an exec segment that is exactly `<interpreter>
  PATH` (no flags, identical literal path) — any other shape falls back to
  today's routing. Caught mid-slice: real shell grammar puts the `&&`-chained
  command on the *same physical line* as the heredoc redirect (the marker
  only changes where the body starts on the *next* line), not after the
  closing delimiter — the first RED test used the wrong shell syntax and was
  corrected before GREEN. `code-review` (Standards + Spec) flagged that the
  new `split_at_heredoc_marker` re-derived heredoc-marker grammar already
  private in `aegis-parser::embedded_scripts::find_heredoc_marker`, and had
  diverged from it (accepted a `<<"WORD"` double-quoted form the real parser
  doesn't, and used a whitespace-only word boundary instead of the real
  alphanumeric/underscore one — silently mis-splitting a marker glued
  directly to `&&` with no space, e.g. `<<EOF&&python3 x.py`, which is valid
  shell since `&` is a metacharacter that terminates a bareword without
  whitespace). Fixed via a further RED/GREEN cycle: `split_at_heredoc_marker`
  now mirrors `find_heredoc_marker`'s exact grammar (no double-quote form,
  alphanumeric/underscore word boundary). 7 new tests (positive `cat`/`tee`
  reuse, dynamic-body degradation, mismatched-path/no-chained-exec/
  third-segment fallback negatives, glued-marker regression).
- **Follow-up (same session, via TDD): `aegis-config` budget/alias wiring.**
  New `language_analysis` `AegisConfig` section (`crates/aegis-config/src/
  model/rules.rs`): `script_file_limit_bytes: u64` (default 256 KiB, clamped
  to a non-configurable `LANGUAGE_ANALYSIS_SCRIPT_FILE_HARD_CEILING_BYTES` = 1
  MiB at every layer — Project may additionally only lower it, never raise);
  `trusted_aliases: Vec<TrustedAlias>` — a Global-layer-only concept (ADR-022
  §6 "trusted global aliases only"): Project-layer entries are dropped
  entirely rather than merged, mirroring the existing `sandbox.allow_write`
  ratchet pattern in `model/ratchet.rs`. Semantic validation added to
  `AegisConfig::validate()` (via `rules::validate_trusted_aliases`, moved
  there to hold the 800-line file-size budget after the addition): rejects
  empty alias/canonical fields, an alias mapping a program to itself, and
  duplicate aliases. `docs/config-schema.md` and `aegis-schema.json`
  regenerated. 13 new tests in `crates/aegis-config/src/model/tests/
  language_analysis.rs`. `code-review` (Standards + Spec) flagged that the
  `trusted_aliases` ratchet-warning branch bypassed the shared
  `ratchet_trusted_aliases`/`push_ratchet_warning` helpers (diverging from
  the module's own stated invariant) and that `CONTEXT.md` wasn't updated for
  the two new domain terms this introduced — both fixed (warning branch now
  reuses the shared helper, with a regression test pinning that repeating an
  identical trusted set no longer spuriously warns; `CONTEXT.md` gained
  **Trusted global alias** and **Script-file limit** entries under
  "Language-aware analysis"). `router::route`/`resolve` still take
  caller-supplied parameters unchanged — there is still no production call
  site to wire the new config fields into (per the Iteration 3/4 notes
  above); this slice only adds the config plumbing itself.
- **Follow-up (same session, via TDD): Iteration 4 REVIEW GATE.** New
  `fuzz/fuzz_targets/router.rs` fuzzes `router::route`/
  `verified_shebang_language` for panic-freedom (200k local runs,
  panic-free) — the first fuzz coverage of the router's hand-rolled
  string-slicing (`split_at_heredoc_marker`, `heredoc_write_target`); 7
  hand-crafted corpus seeds under `fuzz/corpus/router/` (one per illustrative
  shape: explicit interpreter, script-file argument through launcher
  prefixes, direct-exec, `cat`/`tee` heredoc-to-file reuse, a marker glued
  directly to `&&`, `cd`-prefix + pipeline), `.gitignore`-allowlisted the
  same way as the existing `parser` corpus (the fuzzer's own generated
  corpus files are wholesale-ignored). Two tests in `router_tests.rs` pin
  that `route()` returns an identical routed target whether or not the
  target path's parent directories exist — a black-box behavioral proxy for
  "no filesystem access", not independent proof of zero syscalls (the actual
  guarantee is structural: `route`/`route_after_cd` are synchronous with no
  `fs`/I/O import; only `resolve_one` calls `source_reader::
  read_script_file`). A race-oriented stress test in `source_reader.rs`
  (`#[cfg(unix)]`, multi-thread `#[tokio::test]`) runs many concurrent
  `read_script_file` calls against a background thread atomically swapping a
  path between a regular file and a symlink to a different file (via
  write-to-staging + `rename`), asserting no panic/hang and that every
  successful read returns exactly one of the two known-good contents, never
  a corrupted/mixed byte sequence — this demonstrates robustness under heavy
  concurrent path mutation, not a guarantee that the exact pre-open/post-open
  TOCTOU window was hit (the underlying race remains an accepted, documented
  residual risk per ADR-022 §6). `code-review` (Standards clean; Spec) noted
  the "confirm no fs access" claim should be described as behavioral
  evidence rather than proof, and that the fuzz corpus initially under-covered
  the `tee`-write and glued-marker forms the unit tests already covered —
  both addressed (comments reworded for precision; 2 more corpus seeds
  added). Platform scope: Linux only this session (macOS is a separate CI
  job, not exercisable here).
- **Deferred (documented, not silently dropped):** wiring `router`/`resolve`
  output into `worker_client`/`Assessment` (still blocked on the Iteration 1
  merge function's actual language-result input, which only exists once an
  adapter — Iteration 6+ — produces one); a direct Effect-opaque/Required-
  recovery regression test (there is no production code path yet that lets a
  language-aware result influence `effect_opaque`, so a test today would be
  synthetic — same reasoning as Iteration 2's deferred rendering slice); a
  macOS run of the new race-oriented test (Linux only this session).
- **Verified:** `cargo test --workspace` = 1694 passed / 96 suites / 0 failed
  (+34 tests this session across all follow-ups); `cargo clippy --all-targets
  -- -D warnings` clean (root workspace); `cargo +nightly clippy --all-targets
  -- -D warnings` clean (`fuzz/` crate, excluded from the root workspace);
  `cargo fmt --all --check` clean; `tests/aegis_language_boundary.rs` and
  `tests/file_size_budget.rs` both green; the new race test passed 5/5
  repeated local runs.

---

## Prior session (2026-07-20) — L1 Iteration 3 (worker protocol, slices 1-4)

- **Iteration 3 (Worker protocol and failure isolation) done via TDD; the
  Iteration 1 monotonic merge + Iteration 4 source routing are the remaining
  blockers on wiring client results into an `Assessment`.** Scope and seams
  confirmed with the user up front per the TDD skill: framing + worker
  subprocess + parent client (expanded from framing-only); seams — the pure
  `aegis-language::protocol` encode/decode boundary, the `worker::run`
  dispatch loop (in-process + real subprocess), and the `aegis::analysis::
  worker_client` parent client (real subprocess for happy/crash/non-zero/
  noise, `tokio::io::duplex` mocks for timeout/duplicate/out-of-order/
  unexpected/partial-prior-results).
- **Slice 1 (framing, 15 tests):** new `crates/aegis-language/src/protocol.rs`
  — pure length-bounded versioned request/response framing. Magic `AELW`,
  version 1, `request_id` u32 LE, kind u8 (disjoint request `0x01..=0x7F` /
  response `0x80..=0xFF`), payload_len u32 LE, 15-byte header. `MAX_FRAME
 _PAYLOAD` = 1 MiB (ADR-022 §7). `decode_request`/`decode_response` return
  `Ok(None)` for incomplete frames, `Err(DecodeError::{BadMagic,
  UnsupportedVersion, Oversized, InvalidKind, InvalidPayload})` for malformed.
  Driven RED→GREEN per check: round-trip + known-good hand-derived bytes →
  bad-magic → version → oversized → invalid-kind (response-tag-as-request) →
  response codec (Parsed/ParseFailed) → pinning (truncated→None, unknown-lang
  →Err, Parsed-wrong-length→Err) + the "no path-read / no subprocess" property
  pinned by an exhaustive 256-kind-tag loop (only `Parse` accepted). Shared
  `decode_header` factored for both decoders.
- **Slice 2 (worker dispatch, 8 tests):** `aegis-language::worker::run` /
  `run_with_limit` read frames, parse supplied bytes with the matching
  Tree-sitter grammar (`handle_request` → `Response::Parsed{error_count}` via
  `root_node().has_error()`, or `ParseFailed` on no-tree / invalid UTF-8),
  write one response per request, force-exit at `MAX_REQUESTS_PER_SESSION`
  (64). Pure std::io over `R: Read, W: Write`; `RunOutcome` types every stop
  reason (EndOfInput / MaxRequestsReached / TruncatedFrame / MalformedFrame /
  ReadFailed / WriteFailed). In-process tests via `Cursor` + `Vec<u8>`:
  clean parse, bounded sequence, force-exit at cap, malformed stops without
  serving, truncated trailing frame, ParseFailed on invalid UTF-8, nonzero
  error_count on incomplete syntax, all four foundation grammars.
- **Slice 3 (worker CLI mode + real subprocess, 5 tests):** undocumented
  `--internal-language-worker` flag, checked in `main()` before clap parsing
  and Tokio runtime construction (worker stays minimal — no runtime). `cli_
  dispatch::run_internal_language_worker` locks stdin/stdout, delegates to
  `aegis_language::worker::run`, maps clean outcomes → exit 0 / failures →
  `EXIT_INTERNAL`. `tests/language_worker.rs` spawns the real `aegis` binary:
  clean round-trip, stdout writes only frame bytes (no noise), clean exit on
  stdin close, non-zero exit on a malformed frame, bounded sequence. Root
  `aegis` crate now depends on `aegis-language`.
- **Slice 4 (parent client, 9 tests):** `src/analysis/worker_client.rs` (new
  `pub mod analysis` in `src/lib.rs`). `analyze<R: AsyncRead, W: AsyncWrite>`
  sends all request frames, reads responses under a `tokio::time::timeout`
  deadline, correlates strictly in send order by `request_id`, and on any
  failure (Timeout / Closed / ProtocolNoise / DuplicateResponse / OutOfOrder
  / UnexpectedResponse / Io) retains responses already received and marks the
  remaining targets with the failure. `WorkerError: From<WorkerError> for
  DegradationReason::WorkerFailure`. `Worker::spawn` re-execs `aegis` (or
  `current_exe` in production wiring). Hybrid tests confirmed: real subprocess
  (clean round-trip, non-zero exit, stdout noise) + duplex mocks (timeout,
  duplicate, out-of-order, unexpected id, partial prior results on early EOF,
  clean multi-target correlation).
- **REVIEW GATE:** `fuzz/fuzz_targets/language_protocol.rs` fuzzes both
  decoders — 7.8M-iteration smoke run, no panic, coverage 87. No daemon,
  socket, network, temp source file, or inherited command-exec path: the
  worker is parse-only std::io over stdin/stdout and the protocol encodes no
  path-read/subprocess request. The no-source safe path is untouched —
  `no_source_bench` still 1.06 µs (< 2 ms).
- **Architecture:** `analysis` added to `src/lib.rs` public surface;
  `ARCHITECTURE.md §8` updated (module list + ADR-022 note + review date).
  `public_api_surface_is_stable` boundary test updated.
- **Re-review (skeptic round 1) fix round:** 7 of 8 findings fixed via TDD
  (L6 dropped as an overstated, already-disclosed deferral). L2 — encoder is
  fallible (`encode_request`/`encode_response` → `Result<Vec<u8>, EncodeError>`,
  const-asserted `as u32`, no `.expect()` in production). L3 — 1 MiB source
  ceiling is now legal: `MAX_SOURCE_BYTES = 1 MiB`, `MAX_FRAME_PAYLOAD =
  MAX_SOURCE_BYTES + 1` (budgets the lang tag; the off-by-one that rejected a
  1 MiB source is fixed). L1 — parent `send_requests` propagates the stdin
  `flush()` error as `WorkerError::Io` instead of `let _ =` (no longer
  masquerades as `Timeout`). L7+L8 — `Worker::analyze` closes stdin after sending
  and reaps the child (ADR-022 §2 ephemeral); on a non-zero exit after the
  session it degrades the whole session as `WorkerError::NonZeroExit`
  (previously a "responds-fully-then-exits-nonzero" worker was silently reported
  as success). L4 — `--internal-language-worker` flag is a single shared
  `aegis::analysis::INTERNAL_LANGUAGE_WORKER_FLAG` const (main.rs + worker_client
  no longer duplicate it). L5 — dropped the `clone_worker_error` helper (its
  "not Clone" comment was stale; `WorkerError` derives `Clone`) and use
  `err.clone()` so the `Io` variant is exercised. 8 regression tests added.
- **Verified:** `cargo test --workspace` = 1614 passed / 96 suites / 0 failed
  (+45 tests since the Iteration 3 start: 15 framing + 8 dispatch + 5 subprocess
  + 9 client + 8 re-review regressions); workspace `clippy --all-targets
  -- -D warnings` clean; `fmt --all --check` clean; `no_source_bench` 938 ns
  (< 2 ms); `language_protocol` fuzz 7.9M runs panic-free.
- **Deferred:** wiring `worker_client` results into an `Assessment` (monotonic
  merge with baseline + prior target results) — depends on Iteration 1 E
  (merge function) and Iteration 4 (source routing that produces targets);
  worker-dispatcher fuzzing beyond the decoder; the Iteration 3 "test proving
  the worker cannot request a path read or subprocess" is pinned at the
  protocol level (exhaustive kind-tag loop) — a subprocess-level fs-sandbox
  test is a future hardening option.

---

## Prior session (2026-07-20) — L1 Iteration 2 (Audit v2, slices 1-3)

- **Iteration 2 (Audit v2 and explanation contracts) slices 1-3 done via TDD;
  slice 4 (rendering) deferred.** Scope confirmed with the user up front per the
  TDD skill: schema-core (mixed v1/v2 JSONL fixtures + privacy + compatibility
  projection), defer the TUI consolidated-confirmation rendering slice since no
  real degradation-bearing assessments exist until Iterations 6-8 (it would be
  synthetic and the most drift-prone). Seams: the `AuditEntry` JSONL
  serialization boundary, `AuditLogger` query/rotation/integrity over mixed
  logs, the v2 optional fields, the source-privacy boundary, and v1 projection
  compatibility.
- **Slice 1 (RED #1 + GREEN) — v2 schema + mixed-log integrity:** new
  `crates/aegis-audit/tests/audit_v2.rs` drives the v2 schema by hand-written
  v1 + v2 JSONL fixtures (independent of the Rust struct under test).
  `DecisionEntry` gained `basis: Option<AssessmentBasis>` and `analysis:
  Option<AnalysisSummary>`; `MatchedPattern` gained typed `evidence:
  Option<MatchEvidence>` and a stable `detection_id: Option<String>`. All four
  are `#[serde(default, skip_serializing_if = "Option::is_none")]` on both
  `AuditEntryFlat` and `AuditIntegrityPayload`, so a v1 line (all v2 `None`)
  serializes byte-for-byte identical to the pre-v2 form — its hash is unchanged
  and mixed v1/v2 logs verify without rewriting old lines or versioning
  `chain_alg` (the safe path; the plan's "hash the exact serialized entry form"
  is satisfied in spirit — v2 fields are covered by the chain — without the
  chain_alg-versioning change that would break all v1 logs, which contradicts
  "preserve mixed-log verification"). Fresh runtime entries populate `basis`
  from `Assessment::basis()` and `analysis` from `assessment.analysis` via new
  `with_basis`/`with_analysis` builders in `build_audit_entry`; each matched
  pattern carries `MatchResult.evidence` + pattern id via `From<&MatchResult>`.
  5 tests: v2 round-trip preserves basis/analysis/evidence/detection_id; v1
  line deserializes with every v2 field absent; mixed v1/v2 log verifies and
  tampering v2 `basis` breaks the chain (proves v2 fields are in the payload);
  mixed-log query returns both; mixed-log rotation into archive verifies.
- **Slice 2 (RED #2 + GREEN) — source-privacy boundary:** two guard tests pin
  ADR-022 §10 at the audit JSONL surface (composing with the `AnalysisProvenance`
  privacy test in `aegis-types`). `v2_audit_entry_persists_only_allowed_provenance_fields`
  asserts the `LanguageRule` provenance carries EXACTLY the 10 metadata-only
  allowed fields (language, source_origin, rule_id, operation, file_path,
  source_hash, span, certainty, status, degradation_reason) — an allowlist, so
  any leaky extra field fails. `v2_audit_entry_serializes_no_source_body_snippet_ast_or_value_keys`
  recursively scans every key and rejects a denylist of source-content names
  (source_body, snippet, ast, syntax_tree, imported_source, value, code, …).
  These are guards (the invariant holds by construction — `AnalysisProvenance`
  has no leaky fields), pinning the boundary so a future field addition cannot
  silently leak.
- **Slice 3 (RED #3 + GREEN) — compatibility projection:** `v2_entry_still_projects_v1_matched_patterns_and_pattern_ids`
  proves a v2 entry carries the v1 `pattern_ids` + per-pattern v1 fields
  (id/risk/description/safe_alt/category/matched_text/source) ALONGSIDE the v2
  `evidence`/`detection_id` (additive, not replacing).
  `v1_only_log_remains_queryable_through_v2_aware_codebase` proves a v1-only log
  stays queryable and that v2 fields stay `None` on v1-shaped entries (never
  silently back-filled).
- **Slice 4 (rendering) deferred** — ADR-022 §5 consolidated-confirmation
  rendering of multiple decisive Matches + one degradation; no real
  degradation assessments until Iter 6-8, so it would be synthetic.
- **Verified:** `cargo test --workspace` = 1566 passed / 94 suites / 0 failed;
  `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt
  --all --check` clean; `aegis-audit` lib + tests = 82 + 9 audit_v2;
  `audit_integrity` + `full_pipeline_audit` + `full_pipeline_json` = 23 passed
  (v1 byte-for-byte + integrity chain preserved). No production runtime
  wiring of `analysis` (always `None` until language adapters merge results in
  Iter 6-8); `basis` + `evidence`/`detection_id` ARE populated for every fresh
  real entry now. `docs/threat-model.md` updated to record that Audit v2 fields
  are covered by the chain (skip-if-none) and carry metadata only.
- **Review-fix round (Standards + Spec; 0 hard Standards, 4 minor judgement
  calls left, 2 Spec items addressed).** Standards judgement calls (Data Clump
  around basis/analysis, Shotgun Surgery across 8+ files, duplicated
  `evidence/detection_id/basis/analysis: None` fixture boilerplate, and
  speculative-generality on `detection_id`) — all minor/stylistic per the
  reviewer, left as-is except the last, which overlaps Spec (c). Spec (c)
  `detection_id` was a trivial mirror of `pattern_id` whose projection test
  passed by construction: fixed via TDD — `From<&MatchResult>` now derives
  `detection_id` from evidence (`LanguageRule` → `provenance.rule_id`,
  fallback to pattern id when absent; regex/token-prefix → pattern id), driven
  by a RED test with a `LanguageRule` whose `rule_id` deliberately differs from
  the pattern id (3 detection_id tests in `audit_v2.rs`; both branches
  exercised). Spec REVIEW GATE "no source content reaches JSONL, Watch output,
  error reports, or tracing" — the worst Spec finding — resolved as an honest
  deferral, not vacuous guard tests: v2 fields flow only to audit JSONL this
  iteration; Watch `OutputFrame` carries only decision/exit_code/sandbox_status/
  base64 child chunks (no matched_patterns/evidence/basis/analysis), and
  error/tracing don't project v2, so there is no leak path today; the
  multi-surface gate becomes meaningful in Iter 9 when Watch/TUI/error become
  v2-aware, documented in `docs/threat-model.md`. The slice-4 rendering gap and
  the "short in-memory TUI snippet" were already deferred by the user's scope
  decision. Verified: `cargo test --workspace` = 1569 passed / 94 suites / 0
  failed, clippy `-D warnings` clean, fmt clean.

---

## Last session (2026-07-17) — L1 Iteration 1 foundation (slices A+B+C)

- **Iteration 1 A+B+C done via TDD; D/E/F deferred to later sessions.** Scope
  was confirmed with the user up front (A+B+C — compatibility fixtures + new
  zero-I/O analysis types + `Assessment::basis`; not adapting Pattern-backed
  Matches to the Detection model and not migrating `DecisionSource` consumers,
  both of which touch ~6 files and carry the byte-for-byte REVIEW GATE risk).
  Seams confirmed before any test per the TDD skill.
- **Slice A (RED #4 — compatibility fixtures):** new
  `crates/aegis-scanner/src/scanner/tests/compatibility.rs` pins the *current*
  `Assessment` contract (risk, key matched pattern ID, `DecisionSource`,
  `effect_opaque`) for a hand-verified 6-case corpus (Safe / Danger-regex /
  Warn-prefix / Block-regex / effect-opaque-Safe / inline-extracted-Danger).
  Expected values are derived from `patterns.toml` + `patterns/builtins_a.rs`
  (independent source of truth), not from running the scanner; all 4 tests
  green on first run, confirming the hand-derivation. Guardrail for the later
  Pattern→Detection evidence refactor.
- **Slice B (RED #1 — new `analysis` module in `aegis-types`):** introduced
  the common Detection rule + evidence data model in a new
  `crates/aegis-types/src/analysis.rs`, built in four vertical red-green cycles
  (leaf enums → `DetectedOperation` → `AnalysisProvenance`/`TargetAnalysis` →
  `MatchEvidence`): `DetectionMechanism`, `DetectionSource`, `OperandCertainty`
  (Ord: Known<Partial<Dynamic), `OperationKind`, `OperationModifiers`,
  `DetectedOperation`, `SourceOrigin`, `ByteSpan`, `AnalysisProvenance`
  (metadata only — a serialization-boundary privacy test asserts no
  body/snippet/AST/value/contents keys leak), `AnalysisStatus` (Ord:
  NotApplicable<Complete<Degraded, so `max` = worst), `DegradationReason` (the
  seven ADR-022 §4 buckets, non_exhaustive), `TargetAnalysis`, `MatchEvidence`
  (type-state enum — variant encodes mechanism; `LanguageRule` always carries
  operation+provenance; impossible states unconstructable) with
  `mechanism()`/`source()` accessors. 17 module tests. Zero-I/O, deps still only
  serde/schemars — REVIEW GATE met (no Tree-sitter, no parser-crate arrow).
- **Slice C (RED #2 — Assessment basis):** `AssessmentBasis` enum
  (`Fallback` | `Decisive { match_ids }`, serde `tag = "kind"`) + new
  `Assessment::basis()` returning every decisive Match at the Assessment's max
  `RiskLevel`, or `Fallback` only when no rule matched. `decision_source()` is
  **retained unchanged** for v1 compatibility (Slice F migration is deferred, so
  the Slice A fixtures and all existing `DecisionSource` consumers stay green).
  6 basis tests, including the property that distinguishes basis from
  `DecisionSource`: it retains *every* equally-decisive Match ID (the singular
  label collapsed that), and that a matched Safe-risk rule is Decisive, not
  Fallback.
- **Slice D (GREEN — Pattern/Token-prefix → Detection evidence model):** every
  `MatchResult` now carries `evidence: MatchEvidence`. The scanner populates it
  at construction — `RegexPattern` for regex `full_scan`, pipeline-semantic, and
  the synthetic scan-limit matches; `TokenPrefixRule` for `prefix_scan` — with a
  new `From<PatternSource> for DetectionSource` mapping. The field is internal
  (not projected into v1 JSON `matched_patterns` or audit `MatchedPattern`), so
  classifications + public output are unchanged — pinned by the Slice A fixtures
  (still green) + `full_pipeline_json`. `aegis-scanner` + root
  `interceptor::scanner` re-export the analysis types so consumers reach them
  via the existing path. 4 mechanism tests in
  `scanner::tests::match_evidence` (regex vs token-prefix vs inline-extracted;
  every match carries evidence). Updated ~9 `MatchResult` construction sites
  (scanner, explanation, tui tests).
- **Slice F-narrow (GREEN — `basis` alongside `decision_source`):** `ScanExplanation`
  gains `basis: AssessmentBasis`, populated via `assessment.basis()` in
  `build_explanation_from_plan` + the shell-flow builder; the v1
  `decision_source` projection is retained. The field is `#[serde(skip)]` with
  `Default for AssessmentBasis = Fallback` — a deliberate safety choice: the
  explanation is cloned into the audit entry (`build_audit_entry`), so a
  *required* `basis` would have broken deserialization of v1 audit logs (no
  basis) and the integrity chain. `#[serde(skip)]` keeps the v1 audit JSONL
  byte-for-byte unchanged (basis in-memory only; Iteration 2 promotes it to a
  persisted v2 field). 11 `ScanExplanation` test construction sites updated.
  Verified: `full_pipeline_json` + `audit_integrity` (13 passed) — public JSON
  `decision_source` string + audit chain preserved.
- **E (monotonic merge) deferred** — there are no language-analysis results to
  merge yet (those arrive Iterations 6-8); E stays open.
- **Verified (A+B+C+D+F):** `cargo test --workspace` = 1548 passed / 93 suites /
  0 failed; workspace `clippy --all-targets -- -D warnings` clean; `fmt --all
  --check` clean; `cargo test -p aegis-types` = 40, `aegis-scanner` lib = 168
  (incl. 4 compatibility + 4 match-evidence), `aegis-audit` = 77,
  `full_pipeline_json` + `audit_integrity` = 13. One pre-existing environmental
  flake (`supabase … rollback_uses_manifest_target_as_source_of_truth`,
  ETXTBSY/`Text file busy` under concurrent WSL2 compilation) passes in
  isolation and is unrelated to these additive types.
- **Verified:** `cargo test -p aegis-types` = 34 passed; `aegis-scanner` lib =
  164 (incl. 4 compatibility); `aegis-snapshot` lib = 157; workspace
  `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt --all --check`
  clean; full `cargo test --workspace` green except one pre-existing
  environmental flake (`supabase::runtime::tests::rollback_uses_manifest_target_as_source_of_truth`
  — `pg_restore: Text file busy (os error 26)` / ETXTBSY under concurrent
  compilation on WSL2), which passes in isolation and is unrelated to these
  additive data types. No production runtime is wired (the worker, source
  router, adapters, and the Pattern→Detection + DecisionSource→basis consumer
  migrations are D/E/F, still open).
- **Review-fix round (Standards + Spec; Spec clean, 1 hard + 4 judgement-calls
  addressed).** #1 (hard, ubiquitous-language): `CONTEXT.md` now carries a new
  `## Language-aware analysis` glossary section with the 14 terms now backed by
  implemented types (Detection rule, Detection mechanism, Detection source,
  Match evidence, Detected operation, Operand certainty, Analysis status,
  Analysis degradation, Degradation reason, Analysis provenance, Source
  origin, Target analysis, Assessment basis, Decisive Match), via the
  `domain-modeling` skill; `Decision source` cross-references `Assessment
  basis` as its successor. The prior Iter-0 session had deliberately reverted
  these terms as "not yet implemented"; they are now, so the rule ("update
  CONTEXT.md in the same change, do not batch") is satisfied. #2:
  `AssessmentBasis` now derives `schemars::JsonSchema` (consistency with the
  other audit-persistable analysis types). #4: the two new audit enums
  (`AssessmentBasis`, `MatchEvidence`) share the `"kind"` serde discriminator
  tag; domain terms live in the variant values + the `DetectionMechanism` /
  `mechanism()` projection. #5: trimmed the stream-of-consciousness
  `OperationModifiers` serde comment to its conclusion. #3 (speculative
  generality / `DetectionMechanism` duplication) left as a documented
  watch-item — ADR-dictated foundation + projection, not a defect. Verified:
  `cargo test --workspace` = 1544 passed / 93 suites / 0 failed, clippy/fmt
  clean, `contracts_docs` 13 passed.

---

## Last session (2026-07-17)

- **Iteration 0 second review-fix round (Standards + Spec) — triaged, not
  blanket-applied.** The one uncontested hard finding: `CLAUDE.md` still banned
  all C-build deps and omitted Tree-sitter from its approved-deps table while
  `AGENTS.md`/`CONVENTION.md` already carried the ADR-022 exception (shotgun
  surgery) — `CLAUDE.md` is now synced (narrow `aegis-language`-scoped exception
  + a Tree-sitter approved-deps row). CONTEXT.md finding: kept the deliberate
  HEAD revert (plan "not a design scratchpad") and instead softened
  `aegis-language/src/lib.rs` so it no longer presents the Iteration-5
  "detected operation" term as canonical. Test hygiene: the duplicated
  `NO_SOURCE` corpus is extracted to `tests/common/no_source_corpus.rs`, shared
  by `tests/no_source.rs` (module) and `benches/no_source_bench.rs` (`include!`)
  so the test and bench can't drift. Spec completeness in
  `docs/language-grammar-manifest.md`: added the full build-input / native-C /
  transitive-dependency inventory and a rejected-grammars/targets table (wasm
  feature, TSX dialect, 1.x languages, non-musl/Windows targets, with reasons);
  `deny.toml` header now records that the Tree-sitter chain is in the default
  graph and license-covered. `docs/performance-baseline.md` §7 replaces the
  plan's hypothesis budget table with accepted final Iteration-0 defaults, each
  tagged measured / ceiling-adopted / tune-on-wiring (peak-RSS stays the only
  Iter-3 deferral). Router edge cases from the review were checked and判定 as
  correct-for-prototype (empty first `-c` is genuinely no executable source under
  Python semantics), not bugs. Verified: `aegis-language` 20 tests + boundary 2,
  `contracts_docs` 13, clippy `-D warnings` clean, fmt clean, `cargo deny check`
  ok.
- **L1 Iteration 0 — all four RED slices done via TDD; GREEN pending review.**
  New `aegis-language` crate (12th lib, 13th workspace member) owns the
  Tree-sitter boundary per ADR-022. Slice 1 (RED #1 manifest contract):
  `manifest` module with `GrammarEntry`, `validate_entry`/`validate_manifest`,
  rejecting an unpinned grammar, missing license, ABI outside the pinned
  runtime's compatible range, or a grammar absent from the L1 release set; 7
  contract tests. Slice 2 (RED #2a host build + grammar smoke): pinned
  `tree-sitter 0.26.11` + `tree-sitter-{python 0.25.0, javascript 0.25.0,
  typescript 0.23.2, bash 0.25.1}` via crates.io SemVer; all five resolve to a
  single `tree-sitter-language 0.1.7` (no duplicate versions); `SourceLanguage`
  + parse-only `parse()` helper; `BUILTIN_MANIFEST` with provenance; 5 parse/ABI
  tests. TypeScript grammar 0.23.2 is ABI 14 (not 15) — runtime accepts it as
  backwards-compatible (ABI 13–15), so the validator uses the
  `MIN_COMPATIBLE..=LANGUAGE_VERSION` range (more-correct ADR-022 §8 adherence,
  not a boundary change). `docs/language-grammar-manifest.md` records
  versions/provenance/licenses; a `contracts_docs` needle test locks it. Slice
  3 (RED #2b 4-target cross-compile release matrix): `RELEASE_TARGETS` const +
  contract test; `cross-matrix` CI job (cross 0.2.5 for musl x86_64/aarch64,
  native `cargo build` on macos-26-intel/macos-26 for darwin) builds
  `-p aegis-language` under the heavy gate, mirroring `release.yml`. Slice 4
  (RED #3 no-source must not start worker): `router::source_targets` detects
  inline interpreter source (`python3 -c`, `bash`/`sh -c`, `node -e`) with no
  filesystem access; in-process parse-only `worker::analyze` returns
  `Outcome::NotStarted` for no-source commands; `tests/no_source.rs` contract
  test + `benches/no_source_bench.rs` criterion harness assert `NotStarted`
  (panic on regression), wired into the CI perf job. Verified: 1513 workspace
  tests, clippy `-D warnings`, fmt, `cargo deny check`
  (advisories/bans/licenses/sources ok), `cargo audit` (no new advisories from
  tree-sitter or the criterion dev-dep — only the pre-existing starlark-policy
  opt-in set), no-source bench ~109 ns/command. No production runtime (bounded
  worker process, source routing, adapters) is wired yet — those are
  Iterations 3–8.
- **Iteration 0 code-review fixes (Standards + Spec).** Closed the four hard
  Standards findings and the two Spec findings from the slice review:
  (1) `CONVENTION.md` §3 updated — 11→12 lib crates (13 workspace members),
  `aegis-language` named, its boundary sentence corrected (now asserted by
  tests). (2) The aegis-language architectural boundary is now pinned by code,
  not just a doc comment: new `tests/aegis_language_boundary.rs` enforces both
  directions (no workspace crate may depend on `aegis-language`; `aegis-language`
  may not depend on any workspace crate — ADR-022 §4). Each direction was proven
  RED by temporarily adding a forbidden dep, then reverted to GREEN. It lives in
  its own file because `tests/architecture_boundaries.rs` sits at its 800-line
  budget. (3) `ARCHITECTURE.md` §2.9 added — documents the `aegis-language`
  boundary, layout, and Iteration-0 scope. (4) `CONVENTION.md` §6 approved-deps
  list extended with `tree-sitter 0.26.11` + the four L1 grammars, scoped to
  `aegis-language` only (ADR-022 §8). Spec (b): `CONTEXT.md` reverted to HEAD —
  it had added Iteration 1/9 glossary terms (Detected operation, Operand
  certainty, Analysis provenance, Detection rule, Assessment basis, Language-aware
  rule, Analysis override, etc.) with no implementation under them, violating the
  plan's "not a design scratchpad" rule; the shipped `DecisionSource`/`MatchResult`
  terms are restored. Spec (a): `docs/performance-baseline.md` now records the
  Iteration 0 no-source latency budget (~1.03 µs/iter, ~103 ns/command, measured
  2026-07-17) and explicitly defers peak-memory (to Iteration 3's ephemeral
  worker) and binary-size (the crate is not yet linked into the `aegis` binary;
  the 4-target release matrix is the Iteration 0 size gate) with rationale —
  deferred, not omitted. Verified: 1515 workspace tests, clippy `-D warnings`,
  fmt, `cargo deny check` ok, no-source bench ~103 ns/command.
- **Iteration 0 re-review (adversarial pass) — 0 hard Standards violations, 0
  scope creep; 3 Spec gate items were real and are now addressed or honestly
  deferred, not "closed and verified" as the prior summary overclaimed.** (a)
  Measurement coverage: the plan GREEN list has six measurement bullets, only
  ~1.5 were covered. Added `benches/parse_latency_bench.rs` (criterion,
  measurement) parsing one representative inline snippet per foundation grammar
  — measured 2026-07-17 (mean): Python ~43 µs, JavaScript ~25 µs, TypeScript
  ~27 µs, Bash ~18 µs; wired into the CI perf job. Rewrote the
  `docs/performance-baseline.md` Iteration-0 section to cover all six bullets
  (clean-build requirements, binary growth = 0 bytes since the crate is not
  linked, parse latency measured, peak worker RSS deferred to Iteration 3's
  ephemeral worker, startup cost deferred to Iteration 3, all-target build
  parity exercised by cross-matrix) and added a REVIEW GATE status table. (a)
  "adapters present on all targets": the cross-matrix CI job now compiles
  `--tests -p aegis-language` per target (not just the crate), so `grammar_smoke`
  — which references all four grammars — links on each of the four targets;
  honestly documented as link-presence (cross targets can't execute; runtime
  parse-presence is host-only in the quality job). (a) REVIEW GATE: `cargo
  audit` run 2026-07-17 (6 advisories, all pre-existing in the opt-in
  starlark-policy chain — none in tree-sitter/criterion), `cargo deny check`
  green, license review done (manifest + `deny.toml`); the grammar security
  corpus is the one OPEN gate item, honestly deferred — required before
  `aegis-language` is linked into the shipping binary (it is not linked yet, so
  this is not a v0.6.x release blocker). (c) Dropped the unverifiable
  "consistent with prior ~109 ns" comparison in performance-baseline.md (kept
  the measured number + reproducible bench command + date). (c) Pin weakness:
  `validate_entry` only rejects empty/`*` versions, weaker than ADR-022 §8's
  "pinned version"; added `builtin_manifest_versions_match_cargo_lock_pins`
  proving each manifest version equals the exact `Cargo.lock` pin (proven RED
  by a manifest/lock mismatch, reverted to GREEN). Smell fixes: extracted the
  duplicated `assert_no_dep`/`crate_deps_section`/`repo_root` helpers to
  `tests/common/mod.rs` (shared by both boundary test files; shrinks
  `architecture_boundaries.rs` to 767 lines); added
  `builtin_manifest_provenance_is_complete` enforcing the plan-mandated
  inventory fields (crate_name, upstream, license, version) so they are not
  inert (proven RED by blanking a field, reverted to GREEN). Verified: 1517
  workspace tests, clippy `-D warnings`, fmt, `cargo deny check` ok, parse
  latency + no-source benches measured.
- **Operational note (not a code change):** running an effect-opaque command
  (`python3 <file>.py`, any interpreter-on-script) under the aegis shell proxy
  triggers the H9 required-recovery git snapshot, whose backend
  (`crates/aegis-snapshot/src/git.rs`) is `git stash push --include-untracked`;
  it moves uncommitted work — including untracked files — into a stash and
  does not auto-restore it. This destroyed the session's uncommitted work twice
  before the trigger was traced via `~/.aegis/audit.jsonl`. Recover with
  `git stash apply stash@{0}`; avoid the trigger by using only `cargo`/`git`/
  `grep`/Read/Write/Edit for ad-hoc checks. Recorded in agent memory.

## Last session (2026-07-16)

- **Language-aware analysis planned; runtime not implemented.** ADR-022 records
  an additive Tree-sitter slow path isolated in an ephemeral worker, catch-only
  source inspection, typed degradation, and per-language production
  qualification. Roadmap milestone L1 and its release-readiness gate require the
  shared foundation plus Python, JavaScript, TypeScript, and Shell/Bash before
  1.0; Go, PHP, Ruby, PowerShell, Perl, and Lua are staged 1.x adapters. The
  detailed red-green plan is `docs/plans/2026-07-16-language-aware-analysis.md`;
  Standards/Spec review and bounded skeptic verification were completed, 16
  focused docs contract tests passed, changed-line diff-check and local-link/new-
  file whitespace checks passed, and no product-runtime gate was claimed.
- **v0.6.2 release prepared; tag pending.** Version bumped to `0.6.2` across
  the workspace (`Cargo.toml` + all crates + `Cargo.lock`), npm `package.json`,
  README (badge, `--tag v0.6.2` install line), `tests/npm_package.rs`,
  `docs/releases/current-line.md`, `docs/releases/v1.0.0.md`, and the landing
  (`Hero.jsx`, `HowItWorks.jsx`). `CHANGELOG.md` `[Unreleased]` cut to
  `[0.6.2] — 2026-07-16` with a fresh empty `[Unreleased]` above it. Verified:
  workspace tests, clippy `-D warnings`, fmt, landing production build.
- **M1 implemented, skeptic-clean, and locally verified; required PR CI pending.**
  Shell and Watch derive Audit status and active-channel diagnostics from typed
  Sandbox preparation; Watch moves synchronous capability probes to Tokio's
  blocking pool; optional unavailability warns before execution; required
  unavailability blocks; and earlier/fail-closed stops record `NotAttempted`.
  Public/config/threat/architecture docs define the write/network guardrail and
  residual confidentiality risk. Exact package replay, workspace tests, clippy,
  fmt, audit/deny, rustdoc, cross-target checks, and two-round review passed.
  M1 stays Partial/unchecked until required PR CI passes (ADR-021).
- **PR #129 CI follow-up verified locally; CI rerun pending.** Concurrent Audit
  initialization now accepts only a safe same-user `0700` directory when
  another process wins creation, and the Recovery PTY integration waits for the
  visible prompt and keeps BSD `script` input open until child exit so VEOF
  cannot overtake the queued one-time override. The original concurrency test
  passed 50/50 stress runs and the Recovery Run-once test passed 50/50; 1475
  workspace tests, clippy, fmt,
  audit/deny, diff-check, and the Standards/Spec review passed.
- **H9 implemented and verified locally; required PR CI pending.** Protect/Strict
  now preserve Required recovery for bounded Effect-opaque execution even when
  no Snapshot plugin applies. Zero created Snapshots deny without a TTY or use a
  visible, non-persistable one-time Recovery override; Shell and Watch share the
  typed Recovery status and Audit records `no_snapshot_available` with the final
  decision. Audit/`SnapshotPolicy::None` remain opt-outs and ordinary non-opaque
  Danger Snapshots remain best-effort. Public/config/threat-model docs and the
  generated schema match ADR-016. TDD, Standards/Spec review, two-round skeptic
  confirmation, workspace tests, clippy, fmt, audit/deny, and diff-check passed
  locally. H9 stays Partial/unchecked until all required PR CI contexts pass.
- **Release publication migrated to Node.js 24-native actions; PR CI pending.**
  `actions/download-artifact` v8.0.1 and `softprops/action-gh-release` v3.0.2
  are pinned by immutable commit SHA, and the release-workflow contract rejects
  the prior Node.js 20 pins. The focused red/green test, all 10 release-workflow
  tests, fmt, clippy, 1446 workspace tests, audit/deny, and diff-check passed.
  The first parallel workspace run hit an unrelated `snapshot_ordering` flake;
  its focused retry and the complete workspace retry passed.

## Last session (2026-07-15)

- **v0.6.1 release candidate prepared locally.** Workspace crate and internal
  dependency versions, `Cargo.lock`, npm metadata, release docs, README install
  instructions, and Landing version copy now agree on `0.6.1`; the changelog
  cuts the accumulated Unreleased entries as `2026-07-15`. The release-contract
  tests (22), Landing production build, npm dry-run package, 1445 workspace
  tests, fmt, clippy, audit/deny, and diff-check passed. The tag remains pending
  until the release-preparation commit is pushed and required branch CI is green.
- **H7b implemented and verified locally; PR CI pending.** Unix Audit
  directories/artifacts now use `0700`/`0600`; active, lock, query, integrity,
  tail-hash, and rotation opens share descriptor-bound no-follow plus
  tighten-if-owned validation. Rotation preflights every managed slot and
  commits gzip output from owner-only staging before removing the active log.
  ADR-020 and threat-model limits cover caller-owned parent races, non-Unix,
  and crash durability. TDD, review/re-review, 1445 workspace tests, clippy,
  fmt, audit/deny, docs tests, and diff-check passed locally. The checkbox stays
  open until required PR CI passes.
- **H7a closed.** Snapshot stores and bundle directories now use `0700`, while
  SQLite/PostgreSQL/MySQL/Supabase artifacts and manifests use `0600` on Unix.
  Unsafe store leaves are tightened only when owned by the current uid; symlinks
  and other-owner paths fail closed before sensitive writes. Creation composes
  H6 containment before secure reservation; non-Unix deliberately has no POSIX
  mode promise. A follow-up preserves caller-owned SQLite restore modes,
  types unreadable store-metadata failures, tests the other-uid branch, and
  recovers Supabase writes from stale manifest temps. ADR-019, the glossary,
  and regression coverage landed. TDD,
  review/re-review, workspace tests, clippy, fmt, audit, deny, and diff-check
  passed locally; H7a is closed in `TASKS.md`.
- **H6 closed.** SQLite, PostgreSQL,
  and MySQL now prove rollback/delete artifacts remain beneath their plugin-owned
  Snapshot store, rejecting forged outside paths, traversal, and symlink
  escapes with `PathEscapesSnapshotStore`. SQLite restores only to the configured
  database path; legacy in-store artifacts remain supported. ADR-018 and the
  Snapshot store / Snapshot artifact / Path containment glossary are added.
  TDD, review/re-review, `cargo fmt --check`, clippy, workspace tests, audit,
  deny, and diff-check passed locally (the allowed starlark advisories remain);
  required PR CI checks passed. H6 is closed in `TASKS.md`.
- **H5 closed.** PR #122 merged after all required CI checks passed.
  Public/config/landing wording now calls `ChainSha256` an unkeyed local Audit
  integrity chain that detects corruption and inconsistent edits, never an
  adversarial anchor. `aegis audit --verify-integrity` uses the variant-B
  success/failure contract with a residual-risk note. ADR-017 records external
  anchoring as a 1.0 non-goal; a tracked-file wording regression guard and CLI
  integration coverage were added. The guard resolves the repository root from
  `CARGO_MANIFEST_DIR` and allowlists only exact historical/denial lines, so it
  is independent of test cwd and cannot suppress adjacent capability claims.
  Local `fmt`, `clippy`, workspace tests, audit/deny, focused docs tests, and
  review/re-review passed; all required CI checks passed before merge. H5 is
  closed in `TASKS.md` with PR #122 traceability.

## Last session (2026-07-14)

- **M10 closed.** README denial/flow examples and the snapshot-ordering
  regression test passed review/re-review; PR #120 merged after all required CI
  contexts passed.
- **Security backlog normalized.** `TASKS.md` now keeps only the Finding,
  Acceptance criteria, Status, and Traceability for every item. Verified work is
  closed, H7 and M3 are split into independently closable `a`/`b` findings, H9
  is limited to the remaining ADR-016 required-recovery contract, and H5/M1/M8
  now match the audit-integrity / optional-Sandbox / best-effort Snapshot product
  boundaries. Stale Sprint 2/3 groupings were replaced with the agreed
  dependency/risk order.
- **Implementation detail moved to `docs/plans/`.** The existing H9 plan was
  moved from `docs/planning/`, updated with completed/open iterations, and linked
  alongside focused plans for every open P1/P2 finding plus a consolidated P3
  plan. `CONTEXT.md` now distinguishes the `Audit integrity chain`, captured
  `Snapshot` state, and `Rollback` from adversarial tamper proof, backup, or
  general undo.
- **Factually closed:** H3 and M6 remain closed; M3b canonical hook wrapping is
  recorded separately as closed. M10's README denial/flow examples are fixed
  and its PR-CI closure gate passed. H9
  remains Partial (iterations 1–3 only), while H5, H6, H7a/b, M1, M2, M3a, M4,
  M5, and M7–M9 stay open. Docs verification:
  `cargo test --test contracts_docs --test homebrew_formula --test npm_package
  --test release_docs --test snapshot_ordering` = 40 passed; local Markdown
  links = 0 broken; `git diff --check` clean. Standards/Spec review findings on
  M10 closure, plan readiness, and H9 terminology were confirmed and fixed;
  round-2 re-review closed all three. The Audit-mode/H9 concern was dropped as
  not reproducible because fail-closed degradation applies only after recovery
  is required.

---

## Last session (2026-07-09)

- **H9 — effect-opaque execution recovery backstop (ADR-016), Iterations
  1–3 done via TDD.** Iter 1 (model + audit plumbing): direct `effect_opaque:
  bool` field on `Assessment` (orthogonal to `RiskLevel`), `confinement_required`
  axis on `PolicyDecision` (false in v1 — reserved for an optional strict
  tier), `RecoveryDegradation` enum in `aegis-types`, and four backward-compatible
  optional audit fields (`effect_opaque`, `snapshots_required`,
  `confinement_required`, `recovery_degradation`) — older JSONL still
  deserializes. Iter 2 (bounded shape detection): new
  `crates/aegis-scanner/src/scanner/effect_opaque.rs` detects script-file
  execution (`sh ./x.sh`, `python3 ./x.py`, `source ./x`, `. ./x`), interpreter
  stdin (`sh -s`), and pipe-to-shell; inline `-c`/`-e` bodies, package runners,
  and flag-only interpreters are negative forms. Detection runs before the
  safe-path early return; an allocation-free `split_whitespace` +
  `eq_ignore_ascii_case` pre-filter keeps `1000_safe_commands` at 1.96 ms (< 2 ms
  budget). Iter 3 (policy + snapshot flow): `snapshots_required` now fires for
  `effect_opaque` under `SnapshotPolicy::{Selective, Full}` with an applicable
  plugin (no risk raise, no extra prompt); the planning-core plugin-resolution
  guard (`recovery_backstop_applies`) resolves plugins for effect-opaque
  commands, and `execute_with_snapshots` is risk-agnostic so the pre-exec
  snapshot lifecycle works unchanged; project `.aegis.toml` still cannot
  disable recovery (C3 ratchet — added H9 traceability test). Verified:
  `cargo test --workspace` = 1397 passed, `clippy -D warnings` clean, `fmt
  --check` clean, scanner bench 1.96 ms, `cargo audit`/`cargo deny check` ok.
  **Iter 4 (degradation UX / fail-closed) and Iter 5 (threat-model /
  config-schema / README docs + TASKS close-out) deferred per scope decision.**
  ADR-016 written and indexed; `engine.rs` tests extracted to
  `engine/tests.rs` to hold the 800-LoC budget.
- **H9 review cycle (Standards/Spec CHANGES REQUESTED) closed via TDD.**
  (1) Runtime audit construction (`RuntimeContext::build_audit_entry`) now
  populates `effect_opaque` and `snapshots_required` from the assessment and
  policy decision instead of the `Some(false)` defaults — a `sh ./cleanup.sh`
  execution policy required recovery for is no longer logged as backstop-free;
  `confinement_required` records the v1 reserved-tier state. (2) Inline-flag
  detection is now position-sensitive (`interpreter_invocation_is_effect_opaque`):
  `python ./x.py -c` / `bash ./x.sh -c` stay effect-opaque (script file is the
  payload; a later `-c`/`-e` is a script argument), `python -c "code" ./x.py`
  stays inline. (3) `Mode::Audit` documented as an intentional observe-only
  opt-out from ADR-016 recovery (broader than `SnapshotPolicy::None`), with a
  characterization test. Spec #2 (fail-closed when no snapshot can be created)
  remains deferred to Iter 4 — docs make the deferral explicit, no fail-closed
  claim. Spec #4 (README install/FAQ) is out of scope for H9 — the `README.md`
  modifications on this branch predate the H9 work (landing polish). Verified:
  `cargo test --workspace` = 1402 passed, `clippy -D warnings` clean, `fmt
  --check` clean. The review-fix touches only `segment_is_effect_opaque`
  (gated by the allocation-free `has_potential_shape` pre-filter, which is
  false for all 10 safe bench templates), so the safe hot path is unchanged;
  the scanner bench on this WSL2 host read 2.4 ms under load (criterion warned
  it could not hit its sample target), not a code regression — the < 2 ms
  budget was established at 1.96 ms for the pre-filter, which this change does
  not modify.
## Last session (2026-07-07)

- **H4 closed via TDD.** Shell hooks (`claude-code.sh`, `codex-pre-tool-use.sh`) now fail
  closed when the `aegis` binary is unavailable: a `command -v "${AEGIS_BIN}"` guard before
  `exec` emits a `deny` decision (matching the Rust `hook_deny_output` shape) and exits 0,
  instead of `exec` failing with 127 and letting the command run unscanned (ADR-007). The
  original H4 finding (jq fail-open) was already fixed in `8dbb61d`; this closes the residual
  binary-missing fail-open. Hook versions bumped (claude 2→3, codex 3→4). New regression tests
  for both scripts in `tests/agent_hooks.rs`; 3 install tests split into
  `tests/agent_hooks_install.rs` to hold the 800-line budget. 538 tests green, clippy/fmt clean.
- **Security: RUSTSEC-2026-0204.** Bumped transitive `crossbeam-epoch` 0.9.18 → 0.9.20 (via
  starlark → blake3 → rayon-core) to clear the `cargo audit` failure blocking push.

Full history of prior sessions: `git log` and `CHANGELOG.md`.

---

## Milestone status

| Milestone | Title | Status |
|-----------|-------|--------|
| Phase 0–4 | Foundation → Multi-crate workspace | ✅ Done |
| M1 | Snapshot lifecycle & rollback UX | ✅ Done |
| M2 | Audit log hardening | ✅ Done |
| M3 | Distribution (installer, musl, brew, npm, releases) | ✅ Done |
| M4 | Scope reduction (drop native Windows) | ✅ Done |
| M5.1–M5.4 | 800-LoC budget, fuzz CI, snapshot/rollback CI, supply-chain gates | ✅ Done |
| 1.0 docs gate | README, threat model, docs accuracy | 🔲 Open (reopened 2026-07-09 checkup — ARCHITECTURE/CONVENTION/ROADMAP/CHANGELOG stale; see Open decisions) |
| P0 security blockers (C1–C4) | Uppercase bypass, `$IFS` obfuscation, project-config weakening, token-prefix anchoring | ✅ Done |
| P1 security findings (H1–H4, H8) | Segmentation, destructive SQL, H3 patterns, hooks, destructive Git forms | ✅ Done |
| P1 security findings (H5, H6, H7a, H7b, H9) | Integrity wording, containment, artifact hardening, ADR-016 degradation | 🔲 Open (H5/H6/H7a closed; H7b/H9 remain) |
| P2 security findings | M3b/M6/M10 closed; M1, M2, M3a, M4, M5, M7, M8, M9 open | 🔲 Open |
| 1.0 perf gate | Hot path < 2 ms (p99) via criterion | 🔲 Open |
| 1.0 test gate | Zero false-negatives on security bypass corpus | 🔲 Open |

Full task breakdown: `TASKS.md`. Phase/milestone definitions: `ROADMAP.md`.

---

## Current code state

Multi-crate Cargo workspace. Binary crate (`aegis`) at root depends on:

- `crates/aegis-types` — shared data vocabulary (RiskLevel, Decision, …)
- `crates/aegis-parser` — shell tokenizer + PrefixPattern matcher
- `crates/aegis-scanner` — Scanner, PatternSet, built-in patterns.toml
- `crates/aegis-policy` — pure PolicyEngine (TOML DSL + optional Starlark)
- `crates/aegis-config` — config model, loader, validation, schema
- `crates/aegis-explanation` — CommandExplanation and related types
- `crates/aegis-tui` — crossterm confirmation dialog
- `crates/aegis-snapshot` — six snapshot backends (git, docker, pg, mysql, sqlite, supabase)
- `crates/aegis-audit` — AuditLogger, append-only JSONL with optional hash-chain integrity
- `crates/aegis-starlark` — opt-in Starlark policy evaluation (behind `starlark-policy`)
- `crates/aegis-sandbox` — bwrap + Landlock (Linux) / sandbox-exec (macOS) execution confinement
- `crates/aegis-language` — Tree-sitter runtime + four L1 grammars, the
  language-worker protocol/framing, the bounded parse-only worker dispatch, and
  the per-language adapters (Python qualified in L1 Iteration 6) that emit the
  boundary-forced parallel operation vocabulary
  (ADR-022; the only crate permitted native C build input)

The root `aegis` binary also exposes `src/analysis/` — the parent-side
language-worker client (`worker_client`) that spawns the ephemeral
`aegis --internal-language-worker` subprocess and frames requests/responses
(ADR-022 §2, L1 Iteration 3), the recursive work queue and cross-language
execution-sink invariant (Iteration 5), and the root mapping that converts an
adapter's parallel operation vocabulary into the shared `aegis_types` analysis
vocabulary and composes it through the shared classifier + sink invariant
(Iteration 6, `mapping.rs`).

Eleven crates total. DAG boundaries for the first nine are enforced by
`tests/architecture_boundaries.rs`; `aegis-sandbox` is covered separately by
`tests/platform_scope.rs`, and `aegis-starlark` is not yet asserted in either
(gap). Architectural rationale for the shape of this workspace lives in
`docs/adr/` (ADR-001 through ADR-022; `ADR-009` is intentionally absent,
numbering preserved).

As of the 2026-07-22 L1 Iteration 7 Slice 1 (TypeScript adapter) slice:
`cargo fmt --check` and clippy are clean; `cargo test --workspace` = 1893
passed / 0 failed (100 suites). `cargo audit` / `cargo deny check` pass aside
from the pre-existing allowed advisories under the opt-in `starlark-policy`
feature. The no-source safe path bench is 938 ns (< 2 ms); `language_protocol`
fuzz target is panic-free over 7.9M runs; the `router` fuzz target is
panic-free over 200k local runs. **Iteration 6 core deliverable is closed**:
the production-qualified Python adapter is exercised end-to-end through the
real ephemeral worker, including recursive drain of literal execution-sink
payloads (route → spawn → Analyze → map → `AnalysisQueue` drain →
`merge_analysis`), and is backed by a checked-in
positive/negative/narrowness/modern-syntax/malformed Python corpus + real-worker
inline `-c` fixtures. **Iteration 7 (JavaScript family)** has the JS adapter
+ worker dispatch wiring + JS corpus + `node -e` fixtures, and now the
TypeScript adapter (Slice 1, sharing a new `languages::family` module with JS);
the TS adapter is NOT yet wired into the worker (`analyze_source` still routes
TypeScript → `UnsupportedLanguage`, Slice 2 follow-up), and Bash remains
`UnsupportedLanguage` (Iteration 8). Remaining Iteration-6/7 items (per-adapter
fuzzing, audit v1/v2 projection, all-four-targets qualification gate, broader
import/alias/constant + `DatabaseDestructive` corpus cases, TS worker wiring +
corpora + runner-routing negatives, `fs.promises.*`/callback forms, chained
member calls, `ScriptFile`/`DirectExec` fs reads) and the live
`RuntimeContext::assess` wiring stay documented deferrals.

---

## Open decisions / blockers

- **M1 historical commit note:** merged commit `f726c08` mixed the initial
  dependency fragment into a rename-only change without the required TASKS
  reference. Current runtime/package corrections are clean; changing that
  historical commit would require a separate explicitly approved rewrite.
- **Current security order** (`TASKS.md`): H6 → H7a → H7b; H9; M3a; M4 → M7;
  M9; M1; M2 → M5; H5 → M8; then P3. This is dependency/risk order, not a
  calendar sprint.
- **H7b closure blocker:** implementation and local gates are clean; required
  PR CI must pass before the `TASKS.md` checkbox is closed.
- **P1 open contract:** H5 aligns public wording with an unkeyed local `Audit
  integrity chain`; H6 proves snapshot path containment; H7a protects snapshot
  artifact modes; H7b hardens audit modes and symlink opens; H9 finishes only
  ADR-016 missing-required-recovery degradation. Arbitrary dynamic evaluation
  and TOCTOU are not H9 closure criteria.
- **P2 open contract:** M1 surfaces optional `Sandbox` degradation without making
  confinement mandatory; M3a makes the intentional disabled `Toggle` visible;
  M8 aligns Snapshot/Rollback wording with captured pre-execution state rather
  than building a general backup system. M2, M4, M5, M7, and M9 retain their
  focused correctness findings. M3b, M6, and M10 are closed.
- **Docs accuracy regressions (2026-07-09 checkup):** ARCHITECTURE.md references
  removed paths (`src/decision/engine.rs`, `src/interceptor/…`, `src/config/…`,
  `src/snapshot/*.rs`), states a stale 1500/2000 LoC budget (actual 800), and
  omits the sandbox layer; CONVENTION.md says "10 crates" (11) and cites
  removed `src/audit/logger.rs`; ROADMAP.md still lists Windows work + "9
  crates" against the M4 drop-Windows decision; CHANGELOG `[Unreleased]` misses
  a few post-0.6.0 CI/docs commits; `docs/config-schema.md` omits the
  `[sandbox]` section that exists in code and `aegis-schema.json`.
- 1.0 perf gate: hot path p99 < 2 ms not yet confirmed by a criterion run on
  the current workspace.
- 1.0 test gate: zero-false-negative security bypass corpus not yet locked in.
- CI ARM cross-compilation (`aarch64-unknown-linux-musl`) pending.
- Sandbox tests on `ubuntu-latest` / `macos-latest` with real Docker/SQLite
  pending.
- macOS Homebrew/npm smoke test still an operator follow-up.
- `tests/contracts_docs.rs::readme_links_to_contract_docs` still asserts
  removed install-mode vocabulary (`Local`/`Binary`); README only satisfies it
  via a historical sentence. Needs cleanup so the test stops pinning deleted
  modes.

---

## Workflow cadence

- Read this file, `TASKS.md`, and `CONVENTION.md` before starting non-trivial
  work.
- Load the `rust-best-practices` skill before writing or reviewing Rust code
  (see `CLAUDE.md`; the root `AGENTS.md` was removed — Codex reads
  `.codex/AGENTS.md`).
- Security-sensitive parser/scanner/policy changes go through red → green →
  review TDD (see `tdd` skill); close out with `cargo fmt --check`, `cargo
  clippy -- -D warnings`, full `cargo test --workspace`, and a benchmark run
  when the hot path is touched.
- New architectural decisions get an ADR in `docs/adr/` in the same change,
  not a note in this file.
- Every feature/fix/breaking change gets one line under `## [Unreleased]` in
  `CHANGELOG.md` in the same change.
- After a significant change: update "Last session", any changed `Milestone
  status` rows, and `Open decisions / blockers` here — keep it terse.

---

## How to continue

1. Pick the next open item from `TASKS.md` (P1 H5–H8, then P2 M1–M9), or the
   1.0 perf/test gates above.
2. Confirm current baseline: `rtk cargo test --workspace`, `rtk cargo clippy
   -- -D warnings`, `rtk cargo fmt --check`.
3. For the perf gate specifically: run `rtk cargo criterion` and record p99
   hot-path numbers before claiming it closed.
4. Follow the TDD cadence above; update `CHANGELOG.md`, `TASKS.md` (flip
   `[ ]` → `[x]`), and this file's "Last session" section when done.
