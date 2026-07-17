# Language-aware analysis implementation plan

**Status:** Planned; no runtime behavior in this document is implemented yet.

**Architecture contract:**
[`ADR-022`](../adr/adr-022-language-aware-analysis-is-an-additive-isolated-stage.md)

**Roadmap milestone:** `L1` in [`ROADMAP.md`](../../ROADMAP.md)

## Objective

Deliver the shared Language-aware analysis foundation and production-qualified
Python, JavaScript, TypeScript, and Shell/Bash adapters before 1.0 without
replacing the current shell Scanner or regressing the no-source safe-command hot
path. Leave Go, PHP, Ruby, PowerShell, Perl, and Lua as explicit, independently
qualified 1.x slices.

This plan is deliberately test-first. Every iteration begins with a failing
contract or regression test, implements the smallest production slice that makes
it pass, and finishes with focused verification before the next boundary is
opened.

## Locked boundaries

All architecture and security decisions for this milestone live only in the
numbered `Decision` sections of ADR-022. Those sections are the normative inputs
to every implementation PR; this plan defines delivery order and verification,
not a second decision record.

If prototype evidence requires changing one of these boundaries, stop the slice
and amend ADR-022 before implementation continues.

## Milestone split

### Pre-1.0: L1 foundation

1. qualify the Tree-sitter runtime and four grammar families;
2. introduce shared types, evidence, and Audit v2 compatibility;
3. implement the isolated worker and bounded source router;
4. implement the common Detected operation classifier;
5. qualify Python;
6. qualify JavaScript and TypeScript;
7. qualify Shell/Bash;
8. integrate Policy, TUI, Watch, Hook, CI, config ratchets, release packaging,
   fuzzing, and benchmarks; and
9. close the L1 release-readiness gate only after all required CI contexts pass.

### 1.x adapter sequence

Provisional sequence: Go, PHP, Ruby, PowerShell, Perl, Lua. Reorder only with
local opt-in usage aggregates or maintenance evidence. Each adapter is its own
plan, review cycle, qualification record, and default-on release decision.

## Provisional resource budgets

These values are prototype hypotheses, not final promises:

| Resource | Initial value | Contract |
|---|---:|---|
| Existing inline source | 16 KiB | Preserve current limit |
| Script-file default | 256 KiB | Global config may tune within ceiling |
| Script-file hard ceiling | 1 MiB | Non-configurable |
| Script files per command | 8 | Project may tighten |
| Aggregate source per command | 1 MiB | Project may tighten |
| Recursive analysis depth | 8 | Hard ceiling for pre-1.0 |
| Total language-analysis time | 100 ms | Benchmark-derived before merge |

Iteration 0 must replace hypothesis values with measured defaults and record the
accepted latency, peak-memory, and binary-size budgets in
`docs/performance-baseline.md`. No-source commands must not start a worker or
perform filesystem metadata calls.

## Iteration 0 — Native dependency and grammar qualification prototype

**Goal:** prove the approach can build, isolate failures, and meet the release
matrix before the public data model changes.

**Candidate files:**

- `crates/aegis-language/Cargo.toml`
- `crates/aegis-language/src/lib.rs`
- `crates/aegis-language/src/manifest.rs`
- `crates/aegis-language/tests/grammar_smoke.rs`
- `docs/language-grammar-manifest.md`
- `deny.toml`
- `.github/workflows/ci.yml`

**RED**

- Add a contract test that rejects an unpinned grammar, missing license,
  unsupported Tree-sitter ABI, or grammar absent from the release manifest.
- Add release-build tests proving the same four foundation adapters are present
  on Linux musl x86_64/aarch64 and macOS x86_64/aarch64.
- Add a benchmark harness that fails if a no-source command starts the worker.

**GREEN**

- Prototype pinned Tree-sitter runtime plus Python, JavaScript/TypeScript, and
  Bash grammars behind the focused crate boundary.
- Inventory every `build.rs`, native source file, license, upstream repository,
  pinned version/commit, Rust binding, and transitive dependency.
- Build a minimal parse-only worker experiment with no filesystem access.
- Measure clean-build requirements, release binary growth, parse latency, peak
  worker RSS, startup cost, and all-target build parity.
- Choose final defaults within ADR-022 ceilings and document rejected grammars or
  targets with evidence.

**REVIEW GATE**

- Do not merge a production dependency until `cargo audit`, `cargo deny check`,
  all four release builds, license review, and the grammar security corpus pass.
- If Perl, Lua, PowerShell, or another future grammar cannot meet the same gate,
  it stays unsupported rather than receiving a runtime download fallback.

## Iteration 1 — Common detection and analysis types

**Goal:** make the data model capable of representing all detection mechanisms
without changing behavior.

**Candidate files:**

- `crates/aegis-types/src/assessment.rs`
- `crates/aegis-types/src/pattern.rs`
- `crates/aegis-types/src/analysis.rs` (new)
- `crates/aegis-types/src/lib.rs`
- `crates/aegis-scanner/src/`
- `crates/aegis-policy/src/`
- `tests/full_pipeline_json.rs`

**RED**

- Add serialization and ordering tests for Detection rule mechanism/source,
  typed Match evidence, Detected operation, Operand certainty, Analysis
  provenance, per-target status, and typed degradation reasons.
- Add tests that Assessment basis retains every equally decisive Match and uses
  `Fallback` only when no rule matched.
- Add monotonic merge tests: risk cannot decrease, Matches cannot disappear, and
  degradation cannot authorize auto-execution.
- Add compatibility fixtures for the current scanner output before refactoring
  Pattern-backed Matches.

**GREEN**

- Introduce the new zero-I/O shared types in `aegis-types`.
- Adapt regex Patterns and Token-prefix rules to the common Detection rule and
  Match evidence model without changing their classifications.
- Replace singular `DecisionSource` consumers with Assessment basis.
- Add one total, deterministic merge function for baseline and language results.

**REVIEW GATE**

- Existing scanner fixture classifications and explanations are byte-for-byte or
  semantically unchanged where the public format permits.
- The new model contains no Tree-sitter types and creates no dependency arrow
  from `aegis-types` to a parser crate.

## Iteration 2 — Audit v2 and explanation contracts

**Goal:** make new evidence observable without breaking append-only v1 logs or
persisting source.

**Candidate files:**

- `crates/aegis-audit/src/`
- `crates/aegis-explanation/src/`
- `crates/aegis-tui/src/`
- `src/policy_output.rs`
- `src/watch/protocol.rs`
- `tests/full_pipeline_audit.rs`
- `tests/audit_integrity.rs`

**RED**

- Add mixed v1/v2 JSONL fixtures for deserialize, query, rotation, and integrity
  verification.
- Add a privacy test that rejects source body, full snippet, variable value, AST,
  or imported source fields in serialized Audit entries.
- Add compatibility projection tests for `matched_patterns` and `pattern_ids`.
- Add rendering tests for multiple decisive Matches plus one degradation in a
  single confirmation.

**GREEN**

- Add optional v2 fields for typed Matches, Assessment basis, status,
  provenance, and stable detection IDs.
- Interpret absent v2 fields as legacy v1 without rewriting old lines.
- Hash the exact serialized entry form and preserve mixed-log verification.
- Keep any short source snippet in memory and TUI-only.

**REVIEW GATE**

- Golden fixtures prove no source content reaches JSONL, Watch output, error
  reports, or tracing.
- Existing audit-query consumers continue to work against v1-only logs.

## Iteration 3 — Worker protocol and failure isolation

**Goal:** establish a bounded parser process before adding semantic rules.

**Candidate files:**

- `crates/aegis-language/src/protocol.rs`
- `crates/aegis-language/src/worker.rs`
- `src/analysis/worker_client.rs`
- `src/cli_dispatch.rs`
- `src/main.rs` (internal-mode dispatch only)
- `tests/language_worker.rs`
- `fuzz/fuzz_targets/language_protocol.rs`

**RED**

- Add protocol round-trip, version mismatch, truncated frame, oversized frame,
  invalid enum, duplicate response, and out-of-order response tests.
- Add integration tests for crash, hang, timeout, non-zero exit, stdout noise,
  and partial prior results.
- Add a test proving the worker cannot request a path read or subprocess.

**GREEN**

- Implement length-bounded versioned request/response framing over pipes.
- Add an undocumented internal worker CLI mode that delegates immediately to
  `aegis-language`; keep business logic out of `main.rs`.
- Permit multiple bounded requests for one intercepted command, then force exit.
- Convert every worker failure into typed degradation while retaining baseline
  and prior target results.

**REVIEW GATE**

- Fuzz the decoder and worker dispatcher.
- Prove there is no daemon, socket, network access, temporary source file, or
  inherited command execution path.

## Iteration 4 — Source target routing and catch-only reads

**Goal:** produce trustworthy analysis targets without claiming runtime identity.

**Candidate files:**

- `src/analysis/mod.rs`
- `src/analysis/router.rs`
- `src/analysis/source_reader.rs`
- `src/analysis/heredoc.rs`
- `src/analysis/budget.rs`
- `crates/aegis-config/src/`
- `tests/language_source_routing.rs`
- `fuzz/fuzz_targets/heredoc.rs`

**RED**

- Table-test explicit interpreter, versioned basename, trusted global alias,
  verified shebang, generated-file extension, and precedence conflicts.
- Test quoted and expanding heredocs, literal here-strings, strict
  `printf '%s'`, dynamic pipelines, and same-command heredoc file reuse.
- Test literal `cd -- <path> &&` plus dynamic `cd`, `pushd`, substitutions, and
  unresolved cwd.
- Test regular files, absolute paths, symlinks, FIFOs, sockets, devices,
  directories, permission failures, replacement races, size/count limits,
  aggregate limits, UTF-8 BOM mapping, invalid UTF-8, and UTF-16.
- Assert every successful script-file read still preserves Effect-opaque
  execution and Required recovery.

**GREEN**

- Implement the built-in interpreter/runner registry using existing Launcher
  prefix and Effective program normalization.
- Add the async parent-side source reader with no-follow/regular-file checks,
  original-byte hashing, descriptor metadata, and bounded Tokio reads.
- Route only command-visible or safely read source; never content-guess a
  language or probe `PATH`/`--version`.
- Reuse an in-memory heredoc body when the same command later invokes its file.
- Emit typed degradation for every unsupported or dynamic edge.

**REVIEW GATE**

- Run race-oriented and platform tests on Linux and macOS.
- Confirm that the safe path performs neither worker spawn nor filesystem stat.

## Iteration 5 — Shared operation classifier and recursive queue

**Goal:** give all adapters one narrow semantic contract.

**Candidate files:**

- `crates/aegis-language/src/operation.rs`
- `crates/aegis-language/src/classifier.rs`
- `crates/aegis-language/src/resolution.rs`
- `crates/aegis-language/src/queue.rs`
- `crates/aegis-language/tests/classifier.rs`

**RED**

- Add language-neutral matrices for delete, recursive/forced delete,
  overwrite/truncate, permission/ownership changes, device/critical writes,
  destructive database operations, process/shell execution, eval, and selected
  cloud/container/package operations.
- Cover Known, Partial, and Dynamic operands, including negative narrowness cases
  where an API name is referenced but not called.
- Add recursion tests for literal payloads, cross-language nesting, duplicate
  target hashes, cycles, depth 8, target count, aggregate bytes, and timeout.
- Add decode-to-eval tests that produce CodeExecution plus degradation without
  decoding the payload.
- Add one cross-language invariant matrix: a recognized process/shell/eval sink
  with a literal payload emits CodeExecution and a bounded recursive target;
  the same sink with a Dynamic payload emits CodeExecution and Analysis
  degradation without evaluating the payload.

**GREEN**

- Implement a single classifier from Detected operation plus modifiers and
  certainty to Category/RiskLevel/Match.
- Implement bounded resolution for direct imports, aliases, simple constants,
  adjacent literals, literal concatenation, and escapes.
- Implement a parent-owned deduplicated work queue keyed by language and original
  source hash.

**REVIEW GATE**

- No adapter may assign a final RiskLevel directly or implement private copies of
  shared operation semantics.
- Dynamic operands must never be treated as evidence of safety.
- Every recognized dynamic process/shell/eval sink must retain its CodeExecution
  Match and add typed degradation in every adapter.

## Iteration 6 — Python qualification

**Goal:** make Python the first production-qualified semantic adapter.

**Candidate files:**

- `crates/aegis-language/src/languages/python.rs`
- `crates/aegis-language/queries/python/*.scm`
- `crates/aegis-language/tests/corpora/python/`
- `tests/language_python_pipeline.rs`

**RED**

- Add positive and negative corpora for direct imports, `from` imports, aliases,
  simple constants, filesystem mutation, permissions/ownership, process and shell
  calls, eval/exec, destructive database calls, dynamic operands, malformed trees,
  comments/strings, attribute references without calls, and newer supported syntax.
- Add inline `-c`, stdin, heredoc, and named-file full-pipeline fixtures.

**GREEN**

- Add structural queries and a typed Python adapter that emits only Detected
  operations and nested literal targets.
- Map source spans and aliases without traversing imported modules.

**QUALIFICATION GATE**

- Grammar provenance, upstream plus security corpora, adapter/protocol fuzzing,
  all four release targets, Shell/Watch/Hook/CI, Audit v1/v2, and measured budgets
  all pass before Python becomes default-on.

## Iteration 7 — JavaScript and TypeScript qualification

**Goal:** qualify the JavaScript family while keeping grammar-specific syntax
separate from shared operation semantics.

**Candidate files:**

- `crates/aegis-language/src/languages/javascript.rs`
- `crates/aegis-language/src/languages/typescript.rs`
- `crates/aegis-language/queries/javascript/*.scm`
- `crates/aegis-language/queries/typescript/*.scm`
- `crates/aegis-language/tests/corpora/{javascript,typescript}/`
- `tests/language_javascript_pipeline.rs`

**RED**

- Cover CommonJS and ESM imports, renamed/destructured imports, direct built-ins,
  filesystem mutation, child processes, eval/function constructors, destructive
  database APIs, callbacks without invocation, optional chaining, template
  literals, TypeScript-only syntax, dynamic imports, malformed trees, and new
  grammar syntax.
- Add Node inline/file/stdin and TypeScript runner-routing negative cases; package
  runner expansion remains unsupported.

**GREEN**

- Share JavaScript-family resolution where syntax permits, but keep grammar and
  span handling explicit per adapter.
- Emit nested literal targets for supported process/eval calls and degradation for
  dynamic payloads.

**QUALIFICATION GATE**

- JavaScript and TypeScript are enabled independently; a failure in one adapter
  must not silently route its source through the other.

## Iteration 8 — Shell/Bash qualification

**Goal:** analyze command-visible nested shell source without replacing the outer
shell Scanner.

**Candidate files:**

- `crates/aegis-language/src/languages/bash.rs`
- `crates/aegis-language/queries/bash/*.scm`
- `crates/aegis-language/tests/corpora/bash/`
- `tests/language_bash_pipeline.rs`

**RED**

- Cover `bash -c`, `sh -c`, `source`, interpreter stdin, quoted/expanding heredocs,
  literal variables, substitutions, functions, loops, redirects, nested Python or
  Node payloads, malformed syntax, and string/comment false positives.
- Prove outer Scanner Matches remain even when the Bash adapter reports a richer
  duplicate operation.

**GREEN**

- Emit shell operations and recursive language targets without evaluating
  substitutions or becoming the execution parser.
- Deduplicate semantically identical evidence while retaining mechanism provenance.

**QUALIFICATION GATE**

- Parser/scanner/heredoc fuzz targets and the sub-2-ms no-source benchmark remain
  green. Shell/Bash analysis must not change actual shell execution semantics.

## Iteration 9 — Policy, configuration, and user experience

**Goal:** enforce the agreed degradation and approval contract consistently on
every interface.

**Candidate files:**

- `crates/aegis-policy/src/`
- `crates/aegis-config/src/`
- `crates/aegis-tui/src/`
- `src/shell_flow.rs`
- `src/watch/`
- `src/install/hook.rs`
- `tests/full_pipeline_{policy,shell,config,json}.rs`
- `tests/agent_hooks.rs`

**RED**

- Matrix-test Protect, Strict, Audit, Toggle, CI policy, TTY/no-TTY,
  language-aware Warn/Danger/Block, and every degradation class.
- Pin the Strict exception exactly: unrelated Strict denials remain terminal;
  an interactive non-`Block` semantic Match or degradation may proceed only via
  a non-persistable Analysis override; the same case denies without a TTY.
- Prove allowlist and policy-rule `Allow` cannot auto-approve a semantic Match or
  degradation in enforcing interactive modes.
- Prove project config can only tighten budgets and cannot disable/lower built-in
  Language-aware rules; trusted global config is still bounded by hard ceilings.
- Test one consolidated confirmation and the distinct Required recovery prompt.

**GREEN**

- Apply language/degradation gates before existing allow auto-approval paths while
  preserving intrinsic Block priority and trusted Audit/Toggle posture controls.
- Represent Analysis override separately from allowlist, policy-rule Allow, and
  ordinary approval so it cannot be persisted or reused for unrelated Strict
  denials.
- Add config fields only for bounded budgets and trusted global aliases; do not add
  `language_analysis.enabled` or project semantic-rule overrides.
- Render stable IDs, decisive evidence, origin, source location, certainty, and
  degradation without exposing full source.

**REVIEW GATE**

- Shell, Watch NDJSON, Claude/Codex hooks, and non-interactive CI must agree on the
  same Assessment and Decision contract.

## Iteration 10 — Production qualification and release gate

**Goal:** turn the four adapters from implemented experiments into supported
production behavior.

**Candidate files:**

- `.github/workflows/ci.yml`
- `fuzz/Cargo.toml`
- `benches/`
- `docs/performance-baseline.md`
- `docs/release-readiness.md`
- `docs/platform-support.md`
- `docs/threat-model.md`
- `docs/language-grammar-manifest.md`
- release packaging and license-notice files

**RED**

- Add CI contract tests that fail when a release target omits a qualified grammar,
  a grammar manifest drifts from Cargo metadata, a license notice is missing, or
  the safe/slow-path budgets regress.
- Add full recursive, worker-failure, Audit compatibility, source privacy, and
  supported-interface integration suites.
- Add per-adapter fuzz targets and corpora for protocol, malformed syntax, query
  captures, heredoc routing, and bounded resolution.

**GREEN**

- Run and record no-source latency, worker cold/warm-session latency, peak RSS,
  aggregate timeout, and per-target binary-size changes.
- Extend all four official release builds and installers with the same statically
  linked grammar set and provenance manifest.
- Document supported operations, unsupported/dynamic behavior, privacy limits,
  TOCTOU residual risk, and the fact that Aegis remains a heuristic guardrail.
- Add the local-only opt-in aggregation command only if its schema contains no
  command text, paths, source, or automatic exporter.

**FINAL GATE**

- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
- scanner and language slow-path benchmark budgets
- all required branch-protection release, performance, live, and fuzz contexts
- Standards and Spec `code-review`
- adversarial `re-review`, with confirmed fixes and a clean cycle within two rounds

Only then may the L1 roadmap and release-readiness items be checked complete.

## Per-adapter 1.x template

For Go, PHP, Ruby, PowerShell, Perl, and Lua, create a separate plan by copying
this qualification sequence:

1. grammar provenance and all-target build prototype;
2. positive and narrowness corpus for the agreed operation scope;
3. alias, literal, malformed, and new-syntax coverage;
4. source routing and runner-shape tests without package/build runner expansion;
5. recursive cross-language and dynamic-payload behavior;
6. worker crash/timeout/protocol and resource-limit tests;
7. adapter/protocol fuzzing;
8. Audit v1/v2, TUI, Shell, Watch, Hook, and CI integration;
9. latency, peak-memory, and binary-size evidence; and
10. independent review, re-review, and default-on release decision.

PowerShell must additionally resolve the deferred UTF-16 source contract before
qualification. Go must cover normal `go run <file>` through Script source
inspection, while `go generate` remains out of scope. Any adapter that cannot
meet the common gate stays unsupported and degrades honestly.

## Traceability and completion rules

- L1 is a roadmap milestone, not a `TASKS.md` security finding. Do not add or
  close a backlog checkbox for this plan.
- Each implementation PR names the plan iteration and links ADR-022.
- Each language has a checked-in qualification record containing exact grammar
  pins, licenses, corpus results, fuzz evidence, platform builds, and benchmarks.
- `CHANGELOG.md` and `PROJECT_STATE.md` describe only behavior actually verified
  in that slice; they must not present a planned adapter as shipped.
- Update `CONTEXT.md` only when implementation sharpens a domain term rather than
  using it as a design scratchpad.
