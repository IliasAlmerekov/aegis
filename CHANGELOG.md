# Changelog

All notable changes to Aegis are documented here.  
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) · Versioning: [SemVer](https://semver.org/).

**Agent instructions:** prepend a new entry under `[Unreleased]` after every feature,
fix, or breaking change. Use categories: `Added`, `Changed`, `Fixed`, `Removed`, `Security`.
Reference the ADR number when an architectural decision was made (e.g. `(ADR-011)`).

---

## [Unreleased]

### Added

- L1 Iteration 6 Slice C: recursive drain closes the Iteration 6 core
  (ADR-022 §2/§7). `aegis::analysis::run` now drains the parent-owned
  `AnalysisQueue`: inline targets seed the queue at depth 0; the drain loop pops
  a target, spawns a fresh worker, sends one `Request::Analyze`, maps the
  `Response` through `map_adapter_result` with the target's own `depth` and
  `source_hash`, and pushes any literal execution-sink `recursive_targets` back
  onto the queue until it empties or a budget cap fires (`LimitExceeded`
  degradation recorded, ADR-022 §7). A literal `exec`/`eval` payload's nested
  destructive op now surfaces in the merged `Assessment` alongside the top-level
  sink match. `run`'s signature is unchanged; `map_target_result` is refactored
  to `(LanguageAnalysisResult, Vec<QueueTarget>)` and `target_count` covers
  top-level + recursive targets. Still NOT wired into `RuntimeContext::assess`;
  `ScriptFile`/`DirectExec` fs reads, live-assess integration, `aegis-config`
  budget/alias wiring, and worker reuse across pops remain deferred.

- L1 Iteration 6 Slice B: parent-side language-analysis orchestration
  (ADR-022 §2/§6). New `aegis::analysis::run` routes a command's inline source
  targets, spawns the ephemeral worker, sends one `Request::Analyze` per inline
  target, maps each `Response` via the existing in-process `map_adapter_result`,
  and folds the per-target `LanguageAnalysisResult`s into the baseline
  `Assessment` through a single aggregated `merge_analysis`. Returns
  `Outcome::NotStarted { baseline }` (no subprocess spawned, ADR-022 §0) when
  `route` yields no analyzable inline target, else
  `Outcome::Analyzed { assessment, target_count }`. `worker_client::TargetRequest`
  gained a `kind: RequestKind { Parse, Analyze }` field so the transport carries
  `Request::Analyze` without breaking the Iteration-3 `Parse` tests.
  `Assessment` is now `Debug`+`Clone`. This is the isolated tracer bullet proving
  the parent ↔ worker ↔ adapter ↔ mapping ↔ merge composition with a real
  subprocess; it is NOT yet wired into `RuntimeContext::assess`, and recursive
  drain / `ScriptFile`/`DirectExec` fs reads / `source_hash` remain deferred.

- L1 Iteration 6 Slice A: the ephemeral language worker now runs the language
  adapter and returns a full `AdapterResult`, satisfying ADR-022 §2
  ("Tree-sitter parsing and language adapters run in a self-spawned, ephemeral
  worker process"). The worker protocol gained `Request::Analyze` /
  `Response::Analyzed { result }` / `Response::UnsupportedLanguage` (kind tags
  `0x02` / `0x83` / `0x84`; `Parse`/`Parsed`/`ParseFailed` retained). An
  `Analyze` request for Python dispatches to `python::analyze` and frames the
  result as `Analyzed`; the other foundation grammars (no adapter yet) return
  `UnsupportedLanguage`; invalid-UTF-8 source returns `Analyzed` with one parse
  error and no operations (the adapter takes a `&str`; the parent owns the
  encoding contract, ADR-022 §7). The `Analyzed` payload is the hand-rolled
  packed little-endian `AdapterResult` codec, framed as-is by the versioned
  pipe protocol.

- L1 Iteration 6 (slices 1-2): first production-qualified language adapter
  (Python) and the root mapping that composes its output through the shared
  classifier and cross-language execution-sink invariant — ADR-022 §2/§3/§7.
  `aegis_language::languages::python::analyze` runs a `calls.scm` Tree-sitter
  query and interprets each call site in typed Rust, emitting the
  boundary-forced parallel operation vocabulary (`aegis_language::operation` —
  `aegis-language` may not depend on `aegis-types`, ADR-022 §4) covering
  filesystem delete/overwrite, permission/ownership changes, `eval`/`exec`,
  `os.system`/`subprocess.*` execution sinks with cross-language literal
  payloads, and dynamic operands (variable / list argv / f-string → no payload).
  `aegis::analysis::mapping::{map_operation, map_adapter_result}` converts the
  parallel vocabulary into `aegis_types::DetectedOperation` one-for-one and
  composes a `MappingOutcome` (`LanguageAnalysisResult` + recursive
  `QueueTarget`s): a `CodeExecution` sink with a literal payload enqueues a
  bounded recursive target at `parent_depth + 1` (the payload's own language);
  a dynamic/encoded payload records `DynamicSource` and enqueues nothing; a
  nonzero `parse_errors` count records `IncompleteSyntax`; status aggregates
  monotonically. Pinned by `tests/language_python_pipeline.rs` (9 tests,
  in-process — no worker subprocess). Worker/`merge_analysis` runtime wiring,
  bounded symbol resolution, and the full qualification gate remain deferred.

### Fixed

- L1 Iteration 6 review pass: the root mapping no longer records
  `DegradationReason::DynamicSource` for a *non-execution* op whose operand is
  dynamic (e.g. `os.remove(path)`). `DynamicSource` is "source or working
  directory was dynamic" (ADR-022 §4); ADR-022 mandates degradation only for
  dynamic execution-sink payloads (§3/§7) or dynamic source/cwd (§6), not for a
  non-execution dynamic operand — the shared classifier already assigns the
  correct risk certainty-independently. Pinned by
  `non_exec_dynamic_operand_emits_match_without_degradation`. Also moved the
  Python adapter's `Parser::set_language` out of the per-`analyze()` call into
  a one-time `thread_local` init (`.expect()` now lives in startup init, not on
  a per-invocation production path — CONVENTION.md §5), and brought the
  `ARCHITECTURE.md` §8 `analysis` bullet into sync with the current
  `src/analysis/` surface (worker client, routing, queue/sink invariant, mapping).

- L1 Iteration 5: shared language-aware operation classifier
  (`aegis_types::classify` / `language_match`), parent-owned recursive
  analysis work queue (`aegis::analysis::queue`), and the cross-language
  execution-sink invariant (`aegis::analysis::recursive::handle_sink`) —
  ADR-022 §3/§7. The classifier maps a `DetectedOperation` to
  `Category`/`RiskLevel`/`Match` (never `Block`; risk is certainty-independent
  so a `Dynamic` operand never lowers it), the queue dedups targets by
  `(language, source_hash)` with depth (8) / count / aggregate-byte (1 MiB) /
  deadline caps, and a recognized process/shell/eval sink always emits a
  `CodeExecution` Match plus a bounded recursive target (literal payload) or
  `DynamicSource` degradation (dynamic/encoded payload, never evaluated or
  decoded). The classifier lives in `aegis-types` (the plan's
  `aegis-language/src/classifier.rs` would violate the ADR-022 §4 leaf-crate
  boundary — same correction as Iteration 4's `router.rs`). No production
  wiring yet; bounded symbol resolution and adapter result merging land with
  the Iterations 6-8 adapters.

### Fixed

- Made `concurrent_symlink_swap_race_never_panics_or_corrupts_content`
  (`source_reader.rs`) tolerant of contended CI runners: widened the race
  window and retried up to 5 rounds before failing on zero observed
  successful reads, instead of asserting on a single 300ms attempt.

- Language-worker protocol/client hardening (ADR-022 §2, L1 Iteration 3
  re-review): the encoder is now fallible (`encode_request`/`encode_response`
  return `Result<Vec<u8>, EncodeError>`) so an oversized source is rejected as
  `EncodeError::Oversized` instead of `.expect()`-panicking (no `.expect()` in
  production); the 1 MiB source ceiling is now legal — `MAX_SOURCE_BYTES = 1
  MiB` and `MAX_FRAME_PAYLOAD = MAX_SOURCE_BYTES + 1` budget the 1-byte language
  tag, so a 1 MiB source round-trips instead of being rejected as oversized
  (off-by-one fixed); the parent client propagates the stdin `flush()` error
  instead of dropping it (`let _ = flush` → typed `WorkerError::Io`), so a flush
  failure no longer masquerades as a read `Timeout`; `Worker::analyze` closes
  stdin after sending and reaps the child, and a worker that responds fully then
  exits non-zero degrades the whole session as `WorkerError::NonZeroExit`
  (previously silently reported as success); the `--internal-language-worker`
  flag literal is now a single shared `aegis::analysis::INTERNAL_LANGUAGE_WORKER
  _FLAG` const (no comment-only "kept in sync" duplication). 8 regression tests
  added across `aegis-language` and the parent client; `language_protocol` fuzz
  still panic-free (7.9M runs).

### Added

- Source-target router and catch-only script-file reader (ADR-022 §6, L1
  Iteration 4 slices 1-4): `src/analysis/router::route` resolves analyzable
  source in an intercepted command — explicit interpreter, versioned-basename
  normalization (`python3.11` → `python3`), trusted-alias program names,
  argv script-file arguments, direct execution of a path verified by shebang
  (`./script.py` with `#!/usr/bin/env python3`), quoted/expanding heredocs
  and here-strings (expansions/substitutions degrade rather than being
  evaluated), a narrowly-proven `printf '%s' <literal> | interpreter`
  pipeline, and a literal top-level `cd -- <path> &&` cwd rebase (any other
  `cd` form degrades a relative target instead of resolving against the
  wrong directory) — reusing `aegis-parser`'s production tokenizer and
  `Effective program` resolution throughout (so launcher-prefix stacking like
  `sudo timeout 5 python3 -c …` is handled once, not duplicated). An exact
  registry match always takes precedence over a conflicting alias entry.
  `src/analysis/source_reader::read_script_file` performs the actual bounded,
  catch-only file read: rejects symlinks/FIFOs/sockets/directories without
  following them, bounds reads to the configured limit without a full-file
  read on oversized files, strips a UTF-8 BOM, rejects invalid UTF-8, and
  records only a SHA-256 hash — never the source itself. `router::route` also
  reuses an in-memory heredoc body instead of routing a `ScriptFile` read when
  a command writes it to a file and immediately executes that same file
  (narrowly: `cat > PATH`/`tee PATH <<HEREDOC && <interpreter> PATH`, exactly
  one top-level `&&`, identical literal path — any other shape falls back to
  the existing routing above). A new `[language_analysis]` `aegis-config`
  section (`script_file_limit_bytes`, default 256 KiB, non-configurable 1 MiB
  hard ceiling at every layer; `trusted_aliases`, a Global-layer-only concept
  — project-layer entries are dropped entirely, never merged) closes the
  Iteration 4 config-wiring gap, with full ratchet/warning/validation/schema
  coverage. Closed the Iteration 4 REVIEW GATE: a new `fuzz/fuzz_targets/
  router.rs` fuzzes `router::route`/`verified_shebang_language` for
  panic-freedom (200k local runs, panic-free); two tests pin that `route()`
  performs zero filesystem access even for a target whose parent directories
  do not exist; and a race-oriented stress test proves
  `source_reader::read_script_file` stays panic/hang/corruption-free under
  concurrent atomic symlink/regular-file swaps (the underlying TOCTOU gap
  remains an accepted, documented residual risk per ADR-022 §6 — this test
  demonstrates robustness under it, not closure).

### Fixed

- Language-worker protocol/client hardening (ADR-022 §2, L1 Iteration 3
  re-review): the encoder is now fallible (`encode_request`/`encode_response`
  return `Result<Vec<u8>, EncodeError>`) so an oversized source is rejected as
  `EncodeError::Oversized` instead of `.expect()`-panicking (no `.expect()` in
  production); the 1 MiB source ceiling is now legal — `MAX_SOURCE_BYTES = 1
  MiB` and `MAX_FRAME_PAYLOAD = MAX_SOURCE_BYTES + 1` budget the 1-byte language
  tag, so a 1 MiB source round-trips instead of being rejected as oversized
  (off-by-one fixed); the parent client propagates the stdin `flush()` error
  instead of dropping it (`let _ = flush` → typed `WorkerError::Io`), so a flush
  failure no longer masquerades as a read `Timeout`; `Worker::analyze` closes
  stdin after sending and reaps the child, and a worker that responds fully then
  exits non-zero degrades the whole session as `WorkerError::NonZeroExit`
  (previously silently reported as success); the `--internal-language-worker`
  flag literal is now a single shared `aegis::analysis::INTERNAL_LANGUAGE_WORKER
  _FLAG` const (no comment-only "kept in sync" duplication). 8 regression tests
  added across `aegis-language` and the parent client; `language_protocol` fuzz
  still panic-free (7.9M runs).

### Added

- Language-worker protocol and bounded ephemeral worker (ADR-022 §2, L1
  Iteration 3): a pure, length-bounded, versioned request/response framing
  layer in `aegis-language::protocol` — magic `AELW`, version 1, `request_id`
  correlation, disjoint request/response kind tags, a 1 MiB payload ceiling
  (ADR-022 §7), and `Ok(None)` for incomplete frames vs `Err` for malformed
  (bad magic, unsupported version, oversized, invalid kind, invalid payload).
  The only `Request` variant is `Parse { language, source }`; the wire format
  encodes no way to ask the worker for a path read or subprocess (pinned by an
  exhaustive kind-tag test). 15 framing tests.
- The ephemeral worker dispatch loop (`aegis-language::worker::run`) reads
  request frames, parses the supplied bytes with the pinned Tree-sitter
  grammar, and writes one `Response::{Parsed,ParseFailed}` frame per request,
  serving a bounded sequence (≤ `MAX_REQUESTS_PER_SESSION`) then force-exiting.
  It is parse-only — no filesystem, subprocess, daemon, or socket — and every
  stop reason is typed (`RunOutcome`). 8 in-process dispatch tests.
- An undocumented `--internal-language-worker` CLI mode: `aegis` re-execs
  itself into the worker, delegating immediately to `aegis-language::worker::run`
  over stdin/stdout before any clap parsing or Tokio runtime construction, so
  the worker process stays minimal. 5 integration tests spawn the real binary
  over pipes — clean round-trip, stdout writes only frame bytes (no noise),
  clean exit on stdin close, and non-zero exit on a malformed frame.
- The parent language-worker client (`aegis::analysis::worker_client`): spawns
  the worker, frames requests/responses, correlates responses by `request_id`
  in send order under a per-session deadline, and converts every worker
  failure (timeout, early close, protocol noise, duplicate response,
  out-of-order response, unexpected id, I/O error) into a typed `WorkerError`
  that maps to `DegradationReason::WorkerFailure`, retaining responses already
  received when a failure ends the session. Hybrid tests: real subprocess for
  clean round-trip / non-zero exit / stdout noise, `tokio::io::duplex` mocks
  for timeout / duplicate / out-of-order / unexpected / partial-prior-results.
  9 client tests. (Wiring into an `Assessment` is deferred to the Iteration 1
  monotonic merge + Iteration 4 source routing.)
- `fuzz/fuzz_targets/language_protocol.rs`: fuzzes the protocol decoders on
  arbitrary bytes; both decoders are panic-free (return `Ok(None)` or `Err`),
  verified by a 7.8M-iteration smoke run (ADR-022 §2, L1 Iteration 3 REVIEW
  GATE).
- `aegis-language` is now a dependency of the root `aegis` binary; `analysis`
  added to the `src/lib.rs` public API surface (`ARCHITECTURE.md §8` updated).
- `aegis-language` crate skeleton with the release grammar manifest
  qualification contract: it rejects an unpinned grammar, missing license,
  Tree-sitter ABI outside the pinned runtime's compatible range, or a grammar
  absent from the L1 release set; the four foundation grammars (Python,
  JavaScript, TypeScript, Shell/Bash) are pinned via crates.io SemVer, statically
  linked, and parse-qualified on the host build against the live Tree-sitter
  runtime ABI; `docs/language-grammar-manifest.md` records provenance, versions,
  and licenses (ADR-022 §8/§9, L1 Iteration 0).
- Minimal parse-only language worker experiment and no-source contract: an
  in-process `aegis-language` router detects analyzable inline interpreter
  source (`python3 -c`, `bash`/`sh -c`, `node -e`) without filesystem access,
  and the parse-only worker does not start for no-source commands; a contract
  test plus a criterion benchmark harness assert `Outcome::NotStarted` for a
  no-source corpus, failing CI if a no-source command ever starts the worker
  (ADR-022, L1 Iteration 0 RED #3).
- Architectural boundary tests for `aegis-language`: `tests/aegis_language_boundary.rs`
  pins both ADR-022 §4 directions in code — no workspace crate may depend on
  `aegis-language`, and `aegis-language` may not depend on any workspace crate
  (only the pinned Tree-sitter runtime, the four L1 grammars, and `thiserror`).
  Each direction was proven RED by injecting a forbidden dep, then reverted to
  GREEN (ADR-022, L1 Iteration 0).
- Manifest pin and provenance contract tests: `builtin_manifest_versions_match_cargo_lock_pins`
  proves each grammar manifest version equals the exact `Cargo.lock` pin
  (closing the gap where `validate_entry` only rejects empty/`*` versions, not
  caret ranges — ADR-022 §8), and `builtin_manifest_provenance_is_complete`
  enforces that the plan-mandated inventory fields (crate_name, upstream,
  license, version) are populated so they are not inert. Each was proven RED by
  a temporary manifest break, then reverted to GREEN (ADR-022, L1 Iteration 0).
- Parse-latency benchmark: `benches/parse_latency_bench.rs` measures per-grammar
  parse latency on one representative inline-source snippet per foundation
  grammar (ADR-022 Iteration 0 GREEN measurement), wired into the CI perf job.
- Language-aware analysis common Detection- rule + evidence data model in
  `aegis-types` (new `analysis` module): `DetectionMechanism`
  (`RegexPattern`/`TokenPrefixRule`/`LanguageRule`), `DetectionSource`,
  `OperandCertainty` (`Known`<`Partial`<`Dynamic`, ordered), `OperationKind`,
  `OperationModifiers`, `DetectedOperation`, `SourceOrigin`, `ByteSpan`,
  `AnalysisProvenance` (metadata only — no source body/snippet/AST/value, pinned
  by a serialization-boundary privacy test), `AnalysisStatus`
  (`NotApplicable`<`Complete`<`Degraded`, ordered), `DegradationReason` (the
  seven ADR-022 §4 buckets), `TargetAnalysis`, and `MatchEvidence` (type-state
  enum: variant encodes mechanism, `LanguageRule` always carries operation +
  provenance). Zero-I/O, no Tree-sitter, no parser-crate dependency (ADR-022 §4,
  L1 Iteration 1 RED #1).
- `Assessment::basis()` returns the new `AssessmentBasis` — every decisive Match
  at the Assessment's maximum `RiskLevel`, or `Fallback` only when no rule
  matched (ADR-022 §4). The legacy `Assessment::decision_source()` is retained
  unchanged for v1 compatibility projection (L1 Iteration 1 RED #2).
- Scanner compatibility fixtures (`crates/aegis-scanner/src/scanner/tests/
  compatibility.rs`): hand-verified golden corpus pinning the current
  `Assessment` contract (risk, key matched pattern ID, `DecisionSource`,
  `effect_opaque`) across Safe / Danger-regex / Warn-prefix / Block-regex /
  effect-opaque-Safe / inline-extracted-Danger cases, derived from the built-in
  pattern definitions as an independent source of truth — a guardrail for the
  later Pattern→Detection evidence refactor (ADR-022 §4, L1 Iteration 1 RED #4).
- Every scanner `Match` now carries typed `MatchEvidence` identifying its
  detection mechanism and source (ADR-022 §4, L1 Iteration 1 Slice D).
  `MatchResult.evidence` is populated by the scanner at construction —
  `RegexPattern` for regex `Pattern` and pipeline-semantic matches,
  `TokenPrefixRule` for `Token-prefix rule` matches — with a new
  `From<PatternSource> for DetectionSource` mapping the legacy per-pattern
  source onto the common per-Match source. The field is internal: it is not
  projected into the v1 JSON `matched_patterns` or audit `MatchedPattern`
  output, so classifications and public output are byte-for-byte unchanged
  (pinned by the Slice A compatibility fixtures + `full_pipeline_json`).
  `aegis-scanner` and the root `interceptor::scanner` re-export the analysis
  types (`MatchEvidence`, `DetectionMechanism`, `DetectionSource`,
  `AssessmentBasis`) so consumers reach them through the existing path.
- `ScanExplanation` (aegis-explanation) now carries `basis: AssessmentBasis`
  alongside the v1 `decision_source` projection (ADR-022 §4, L1 Iteration 1
  Slice F-narrow). Populated via `Assessment::basis()` in
  `build_explanation_from_plan` and the shell-flow explanation builder. The
  field is `#[serde(skip)]` with `Default for AssessmentBasis = Fallback`, so
  it is available in memory but NOT persisted into the v1 audit JSONL —
  existing audit entries deserialize byte-for-byte and the integrity chain is
  preserved; Iteration 2 (Audit v2) promotes it to a persisted, v2-compat
  field. The v1 `decision_source` string/label and JSON output are unchanged.
- Audit schema v2 optional fields on `aegis-audit`: `DecisionEntry` gains
  `basis: Option<AssessmentBasis>` and `analysis: Option<AnalysisSummary>`;
  `MatchedPattern` gains typed `evidence: Option<MatchEvidence>` and a stable
  `detection_id: Option<String>` (ADR-022 §10, L1 Iteration 2). All are
  `#[serde(default, skip_serializing_if = "Option::is_none")]`, so a legacy v1
  line (absent v2 fields) deserializes with them as `None` and serializes
  byte-for-byte identical to the pre-v2 form. Fresh runtime entries populate
  `basis` from `Assessment::basis()` and `analysis` from `Assessment::analysis`,
  and each matched pattern carries its `MatchResult.evidence` + a stable
  detection id derived from the evidence (`LanguageRule` → provenance rule id,
  falling back to the pattern id; regex/token-prefix → pattern id).
  `matched_patterns` and `pattern_ids` remain as v1 compatibility
  projections alongside the v2 fields. The integrity payload covers the v2
  fields (skip-if-none), so v1 hashes are unchanged and mixed v1/v2 logs verify
  without rewriting old lines or versioning `chain_alg`; tampering any v2 field
  breaks the chain. Pinned by `crates/aegis-audit/tests/audit_v2.rs` (v1/v2
  round-trip, mixed-log deserialize/query/rotation/integrity, source-privacy
  allowlist + denylist, and v1 projection compatibility).

### Changed

- `CONVENTION.md` §3 and §6, `ARCHITECTURE.md` §2.9, and
  `docs/performance-baseline.md` updated to reflect `aegis-language`: 11→12 lib
  crates (13 workspace members), the `aegis-language` boundary asserted by
  tests, `tree-sitter 0.26.11` + the four L1 grammars added to the approved-dep
  list (scoped to `aegis-language` only, ADR-022 §8), and the Iteration 0
  no-source latency budget recorded (~103 ns/command) with peak-memory and
  binary-size budgets explicitly deferred to the iterations that wire the
  ephemeral worker and link the crate (ADR-022, L1 Iteration 0).
- `docs/performance-baseline.md` Iteration-0 section rewritten to cover all six
  GREEN measurement bullets (clean-build requirements, binary growth = 0,
  parse latency measured, peak worker RSS deferred, startup cost deferred,
  all-target build parity exercised) and add a REVIEW GATE status table; the
  unverifiable "consistent with prior ~109 ns" comparison was dropped in favor
  of the measured number plus a reproducible bench command (ADR-022, L1
  Iteration 0).
- The `cross-matrix` CI job now compiles `--tests -p aegis-language` per target
  (not just the crate) so `grammar_smoke`, which references all four grammars,
  links on each of the four release targets — proving link-presence on all
  targets, with runtime parse-presence staying host-only (ADR-022 §8, L1
  Iteration 0 RED #2).
- Duplicated boundary-test helpers extracted to `tests/common/mod.rs`, shared
  by `tests/architecture_boundaries.rs` and `tests/aegis_language_boundary.rs`,
  removing the drift risk between the two `assert_no_dep` copies.
- `CONTEXT.md` gains a `## Language-aware analysis` glossary section for the
  terms now backed by implemented types (Detection rule, Detection mechanism,
  Detection source, Match evidence, Detected operation, Operand certainty,
  Analysis status, Analysis degradation, Degradation reason, Analysis
  provenance, Source origin, Target analysis, Assessment basis, Decisive
  Match), and the `Decision source` entry cross-references `Assessment basis`
  as its successor (ADR-022 §4, L1 Iteration 1 review-fix #1).
- `AssessmentBasis` derives `schemars::JsonSchema` (consistency with the other
  audit-persistable analysis types) and shares the `"kind"` serde discriminator
  tag with `MatchEvidence`, so the two new audit enums land with one
  convention; the domain terms live in the variant values and the
  `DetectionMechanism`/`mechanism()` projection (L1 Iteration 1 review-fix #2/#4).
- The Iteration 0 REVIEW GATE is honestly **open on one item**: `cargo audit`
  (no tree-sitter/criterion advisories), `cargo deny check`, and license review
  pass, and the four release builds are CI-gated, but the grammar security
  corpus is not yet built. It is required before `aegis-language` is linked
  into the shipping binary (not yet linked), so it is not a v0.6.x release
  blocker (ADR-022, L1 Iteration 0).
- Accepted the design for the planned pre-1.0 Language-aware analysis milestone:
  an additive, isolated Tree-sitter stage with catch-only source inspection, typed
  degradation, four-language foundation qualification, and staged 1.x adapter
  rollout; no analyzer runtime is implemented yet (ADR-022, L1).

### Fixed

- `CONTEXT.md` no longer ahead of the implementation: it had added Iteration 1/9
  glossary terms (Detected operation, Operand certainty, Analysis provenance,
  Detection rule, Assessment basis, Language-aware rule, Analysis override, and
  related scaffolding) with no code under them, against the plan's "not a design
  scratchpad" rule. Reverted to the shipped vocabulary (`DecisionSource`,
  `MatchResult`); language-aware terms will enter the glossary in the iterations
  that implement them (ADR-022, L1 Iteration 0).

## [0.6.2] — 2026-07-16

### Security

- Optional Sandbox degradation is now visible and auditable on every execution surface: Shell warns on stderr, Watch emits protocol-safe warning or required-block diagnostics without blocking its Tokio worker, Audit records the prepared path including `NotAttempted`, `aegis-sandbox` remains locally packageable with its versioned foundation dependency, and `sandbox.required = true` remains fail closed; public docs define the write/network guardrail and preparation/exec error contracts without claiming confidentiality (ADR-021, M1).

### Fixed

- Audit initialization now tolerates another same-user process winning creation of an owner-only Audit directory while still rejecting symlinks, non-directories, wrong owners, and non-`0700` modes; the cross-platform Recovery PTY integration waits for the visible prompt and keeps BSD `script` input open until the child exits, preventing VEOF from overtaking its queued one-time response on macOS (ADR-016, ADR-020, H9).

### Changed

- Renamed the repository to `IliasAlmerekov/aegis-shellguard`: all repository URLs in the README, installer scripts, packaging metadata, tests, and docs now point at the new address (GitHub redirects the old ones). The product name **Aegis**, the `aegis` binary, crate names, npm package `@iliasalmerekov/aegis`, Homebrew tap `IliasAlmerekov/aegis`, config files, and `~/.aegis/` paths are unchanged.
- Release publication now pins the Node.js 24-native `actions/download-artifact` v8.0.1 and `softprops/action-gh-release` v3.0.2 by immutable SHA, removing the Node.js 20 deprecation annotation from future tag workflows.

## [0.6.1] — 2026-07-15

### Security

- H9 required recovery now survives missing Snapshot-plugin availability: bounded ADR-016 Effect-opaque execution denies without a TTY when no required Snapshot is created, offers only a visible one-time interactive Recovery override, records `no_snapshot_available` with the final Audit decision, and keeps ordinary non-opaque Danger Snapshots best-effort (ADR-016, H9).
- H7b audit hardening now creates Unix Audit directories/artifacts with owner-only modes, rejects unsafe final-component targets through descriptor-bound no-follow opens, preflights every managed rotation slot, and stages gzip archives before commit; non-Unix and parent-entry/durability limits remain explicit (ADR-020, H7b).
- H7a follow-up: SQLite Rollback now preserves the caller-owned live database mode, unsafe Snapshot-store metadata reads yield the typed permission rejection, and a stale Supabase manifest temp is bypassed with a fresh secure reservation (ADR-019, H7a).
- H7a snapshot artifacts now use owner-only Unix modes (`0700` directories and `0600` files); unsafe Snapshot store leaves are tightened or rejected before sensitive writes, while non-Unix behavior deliberately makes no POSIX-mode claim (ADR-019, H7a).
- H6 snapshot path containment: SQLite, PostgreSQL, and MySQL now prove every rollback/delete artifact stays beneath the plugin-owned Snapshot store, rejecting traversal, outside, sibling-prefix, and symlink escapes; SQLite restores only to its configured live database path, never an identifier-provided destination (ADR-018, H6).
- H5 audit-integrity contract: `ChainSha256` is now consistently described as an unkeyed local audit integrity chain that detects corruption and inconsistent edits, not an adversarial anchor; `aegis audit --verify-integrity` states that bounded contract, and a tracked-file wording guard prevents capability overclaims (ADR-017, H5).
- Effect-opaque execution (`sh ./cleanup.sh`, `python3 ./x.py`, `source ./x`, `. ./x`, `sh -s`, and existing pipe-to-shell shapes) now requires a pre-execution recovery snapshot under `SnapshotPolicy::{Selective, Full}` when an applicable snapshot plugin exists — without raising `RiskLevel` or introducing a confirmation prompt; `SnapshotPolicy::None` remains the trusted/global opt-out and project `.aegis.toml` cannot disable the requirement under the C3 ratchet (ADR-016, H9).
- Shell hooks (`claude-code.sh`, `codex-pre-tool-use.sh`) now fail closed when the `aegis` binary is unavailable: a `command -v` guard before `exec` emits a `deny` decision (matching the Rust `hook_deny_output` shape) and exits 0 instead of letting `exec` fail with 127 and pass the command through unscanned (ADR-007, closes H4).
- Bumped transitive `crossbeam-epoch` 0.9.18 → 0.9.20 to clear RUSTSEC-2026-0204 (invalid pointer dereference in the `fmt::Pointer` impl for `Atomic`/`Shared`); pulled in via the `starlark` chain (`blake3` → `rayon-core` → `crossbeam-deque`).

### Added

- Effect-opaque execution model and detection (ADR-016, H9): a direct `effect_opaque: bool` field on `Assessment` (orthogonal to `RiskLevel`), bounded v1 shape detection in the scanner (script-file execution, interpreter stdin, pipe-to-shell) with an allocation-free pre-filter that keeps the safe hot path under 2 ms, a `confinement_required` axis plumbed through `PolicyDecision` (false in v1, reserved for an optional strict tier), and a `RecoveryDegradation` enum for future missing-recovery reasons.
- Audit entries now record `effect_opaque`, `snapshots_required`, `confinement_required`, and `recovery_degradation` as backward-compatible optional fields (`#[serde(default, skip_serializing_if = "Option::is_none")]`) so older JSONL entries still deserialize (ADR-016, H9).
- Landing: copy-to-clipboard confirmation (checkmark pop-in) on the install snippet button, a number-ticker count-up for the trust-strip stats, a sliding tab indicator with fade-in content on the "Why Aegis" tabs, a shake/pulse entrance and key-press feedback in the live demo terminal, and a hover state on trust-strip cards.
- Landing: `public/aegis.svg` replaces `shield-icon.png` as the nav and footer mark, re-exported with its square background frame removed (flood-filled to alpha transparency from the vectorized source) so only the shield-and-prompt glyph shows against any surrounding background.

### Fixed

- Effect-opaque audit + classifier (ADR-016, H9 review cycle): runtime audit construction now populates `effect_opaque` and `snapshots_required` from the assessment and policy decision instead of emitting the `Some(false)` defaults, so a `sh ./cleanup.sh` execution that policy required recovery for is no longer logged as if no backstop was needed; `confinement_required` records the v1 state (optional strict tier still reserved). Inline-flag detection is now position-sensitive — `python ./x.py -c` / `bash ./x.sh -c` stay effect-opaque (the script file is the payload, a later `-c`/`-e` is a script argument), while `python -c "code" ./x.py` stays inline — and a bounded per-interpreter table of value-consuming options (`--require`/`-r`/`--import`, `-m`/`-W`/`-X`, `-I`/`-C`) skips the option's separate-argument value when locating the first positional, so a path-like *option argument* (`./preload.js` in `node --require ./preload.js -e "code"`) no longer spoofs the script-file slot and a real inline body is no longer misclassified as effect-opaque; this is a v1 bounded heuristic (ADR-016) — unlisted value-consuming flags can still spoof the slot, accepted because the error direction is fail-safe (an extra recovery snapshot, never a block) whereas the inverted heuristic would drop a real script file's recovery snapshot. `Mode::Audit` is documented as an intentional, observe-only opt-out from ADR-016 recovery (broader than `SnapshotPolicy::None`).
- Landing: `NumberTicker` could latch onto a wrong value (e.g. showing `-5%` instead of `100%`) if the trust-strip's `IntersectionObserver` toggled `inView` during a fast scroll — the first `requestAnimationFrame` tick anchored elapsed time to a `performance.now()` call taken before the frame was scheduled, which could yield a negative delta, and the "already played" flag latched on start rather than completion so a cancelled mid-flight animation could never re-run to correct itself.

### Changed

- Locked the remaining H9 ADR-016 design: bounded Effect-opaque execution requires at least one Snapshot independently of plugin availability, non-interactive degradation denies, and interactive execution needs a non-persistable one-time Recovery override; ordinary non-opaque Danger snapshots remain best-effort (ADR-016, H9).
- Locked the H7b audit-artifact hardening design: Unix owner-only artifacts,
  target-level no-follow, tighten-if-owned migration, whole-rotation preflight,
  staged gzip commit, and explicit parent/non-Unix/durability limits; the
  implementation followed after the H7a clean-cycle fact refresh.
- Closed the M10 backlog finding after PR #120 merged with all required CI checks green; the README denial example and command-flow wording now match the verified snapshot ordering.
- Normalized the 1.0 security backlog: closed verified work, split H7/M3 into independently closable findings, narrowed H9 to the remaining ADR-016 contract, aligned H5/M1/M8 with the heuristic-guardrail product boundary, replaced stale sprints with dependency order, and moved implementation detail into linked `docs/plans/` files.
- CI: eliminated duplicate push+PR runs on feature branches (`push` trigger now scoped to `main` only) and added a `concurrency` group that cancels superseded runs on non-`main` refs only — pushes to `main` always run to completion so every commit gets a full audit/deny/fuzz pass, and the weekly schedule/`workflow_dispatch` can't race-cancel an in-flight `main` push (or vice versa).
- CI: split the `build` and `live-installer` jobs into always-on Linux jobs plus gated `build-macos`/`live-installer-macos` jobs that only run on pushes to `main`, PRs targeting `main`, the weekly schedule, and manual dispatch — cuts macOS runner minutes (billed at 10x Linux) on every feature-branch push/PR while keeping macOS coverage before merge to `main`. `release.yml` macOS builds are unaffected.
- CI: added `timeout-minutes` to the `quality`, `security`, `build`, `build-macos`, `live-installer`, and `live-installer-macos` jobs so a hung runner (macOS in particular) fails fast instead of burning minutes up to GitHub's 360-minute default.
- CI: pinned the macOS runners to `macos-26` instead of the drifting `macos-latest` label so the build/installer jobs target a fixed image.
- CI: dropped `--locked` from the `cargo-fuzz` install step so the fuzz job isn't broken by a stale lockfile for the tool.
- CI: deduped Rust toolchain setup into a composite action and gated the heavy jobs behind a single job to cut redundant work.

### Fixed

- Restored release docs required by the `release_docs` tests after a docs prune removed them.
- Docs: corrected `PROJECT_STATE.md` (crate list 9→11, test count, open-items sync), `CONVENTION.md` (10→11 crates, stale `src/audit/logger.rs` / `src/snapshot/` paths), `ROADMAP.md` (native-Windows items withdrawn per M4, crate count), and `ARCHITECTURE.md` (staleness banner) during the 2026-07-09 checkup; removed the snapshot line from the README Before/After denial example (M10).

---

## [0.6.0] — 2026-07-03

### Security

- Extended FS-015 rsync delete coverage to include `--delete-missing-args` (turns missing-source-args errors into destination-side deletions).
- Narrowed DB-006 redis-cli rule to only fire when `FLUSHALL`/`FLUSHDB` is the first non-option token (the Redis command), not when it appears as a key argument to another command (e.g. `redis-cli GET FLUSHALL`); implemented via a local `redis_cli_flush_is_command` predicate following the FS-011 pattern.

- Hardened H3-followups scanner coverage for missed destructive CLI forms:
  `wipefs` short flag bundles, `gcloud storage rm --recursive`, `rsync --delete*`,
  `blkdiscard`, `sgdisk --zap-all`/`-Z`, destructive `parted`, and
  `redis-cli FLUSHALL`/`FLUSHDB`.
- Closed the C4 token-prefix anchoring bypass (ADR-014): token-prefix and by-program indexed detections now resolve an `Effective program` per scan target by stripping built-in launcher prefixes (`rtk`, `sudo`, `env`, `command`, `nice`, `timeout`, etc.) and basename-normalizing absolute program paths, so `/usr/bin/git reset --hard`, `rtk git clean -fd`, `sudo /bin/kill -9 1`, and `/usr/local/bin/docker volume prune` no longer bypass migrated Git/Cloud/Docker/Process rules. Timeout options (`-s`/`--signal`, `-k`/`--kill-after`, `--preserve-status`, etc.), sudo environment assignments, unknown sudo/env launcher flags, and stacked sudo options (`sudo -n -u postgres ...`) are handled conservatively so option arity drift prompts rather than silently missing.
- Audit hash-chain append no longer bricks command interception when the active audit log ends with a torn/truncated final line: tail scanning now walks back to the previous valid JSONL entry instead of failing closed on the malformed tail.
- Closed the C3-residual project-config weakening paths (ADR-013): a project-layer `[[rules]]` entry whose effective decision is `Allow` — either a top-level `decision = "allow"` or a `decision = "prompt"`/`"block"` rule with `when.then = "allow"` (resolved at runtime by `effective_decision`) — is now DROPPED at merge and surfaced as a `project_security_ratchet` warning by `aegis config validate`. Unlike an `[[allow]]` entry (capped by `allowlist_override_level`), a `[[rules]] Allow` auto-approves a `Warn`/`Danger` command before `Mode` with no ceiling, so a repository could otherwise silently auto-approve a `Danger` command. The project layer may still tighten via `Prompt`/`Block`; global `[[rules]]` stays last-wins. `audit.integrity_mode` is now ratcheted so a project cannot weaken `ChainSha256` to `Off` (stricter of base/requested wins, warned); global stays last-wins. The merge and warning paths share the same `is_untrusted_allow` predicate and `most_restrictive_integrity_mode` helper, so the reported `kept` value matches the effective merged value.

### Fixed

- Codex project configuration and agent prompts now reference `.codex/AGENTS.md` after moving the Codex instruction file out of the repository root.

### Security

- Project-local `.aegis.toml` can no longer weaken security-critical config fields inherited from defaults/global config; project attempts to set audit-only mode, broader allowlist overrides, weaker CI policy, disabled snapshots (`auto_snapshot_*` flags), or a weaker sandbox (`sandbox.enabled`/`required`/`allow_network`/`allow_write`) are ratcheted to the stricter value and reported by `aegis config validate`. `true`-is-stricter fields keep `base || requested`, `allow_network` keeps `base && requested`, and `allow_write` keeps the trusted base set under the project layer (ADR-013).
- Closed the C3 sibling-field snapshot bypass: a project layer can no longer empty/narrow a provider target config (`sqlite_snapshot_path`, `postgres_snapshot`/`mysql_snapshot`/`supabase_snapshot` `database`, `docker_scope`) to make an enabled provider a silent no-op. The ratchet is conditional on the provider being enabled in the trusted base (`snapshot_policy != None && (Full || auto_snapshot_<provider>)`), matching the registry materialization rule (under `None` nothing is materialized, so no ratchet fires); repointing to another non-empty target stays allowed. `docker_scope` ratchets structurally — a project may only keep-or-broaden (`All` broadest; `Labeled`↔`Labeled` same label = keep; `Names`→`Names` with overlay patterns ⊇ base patterns = broaden); intra-rank narrowing (disjoint `Names`, pattern subset, `Labeled`↔`Names` cross-mode, `Labeled` label change) is rejected and warned (ADR-013).
- `sandbox.allow_write` now honors project-side tightening: the project overlay is merged as the intersection with the trusted base (preserving base order), so a project may narrow the writable surface; expansion attempts (paths outside the base) are dropped and reported by `aegis config validate` (ADR-013). The warning gates on genuine expansion (a requested path absent from the base), so a reordered-but-equal subset no longer triggers a spurious advisory.
- `PartialSandboxSettings` and the direct `SandboxSettings` now set `#[serde(deny_unknown_fields)]`, so misspelled sandbox fields (e.g. `require`, `allow_netork`) fail closed at parse time instead of leaving the intended security field silently unset.

## [0.5.9] - 2026-06-24

### Security

- Claude Code interception no longer depends on `aegis` being on the hook-exec PATH: `aegis install-hooks --claude-code` (and `--all`) now materializes an absolute, jq-free shim at `~/.claude/hooks/aegis-pre-tool-use.sh` and registers its absolute path in `settings.json`, at parity with the Codex hook (ADR-012).

### Fixed

- `aegis install-hooks --claude-code` now migrates away every aegis-managed legacy Bash registration — the bare `aegis hook` command and the legacy `aegis-rewrite.sh` file — to the absolute shim while preserving unrelated user hooks (including commands that merely mention `aegis`); reinstall is idempotent (ADR-012).
- The shared `aegis hook` deny response now emits a top-level `reason` mirroring `hookSpecificOutput.permissionDecisionReason`, so the deny message is visible in both Claude Code (top-level `reason`) and Codex (`permissionDecisionReason`) (ADR-012).
- `scripts/uninstall.sh` now removes the absolute Claude hook shim and prunes its `PreToolUse` `Bash` registration, alongside the existing legacy `aegis hook` / `aegis-rewrite.sh` cleanup (ADR-012).
- `scripts/uninstall.sh` normalizes a trailing slash on `$HOME` before building prune paths so they match the absolute path the Rust installer registers via `std::path::absolute` / `Path::join` (which never emits a doubled separator); root `/` is preserved (ADR-012).
- `scripts/hooks/claude-code.sh` now ends with a trailing newline (POSIX text-file convention), and the ADR-012 "byte-identical except header" wording was corrected to "behaviorally identical; only agent-specific comments differ" since the two shims cross-reference each other by name (ADR-012).
- Closed the C2 `$IFS` command-obfuscation bypass by normalizing unquoted literal `$IFS` / `${IFS}` as shell separators during tokenization, so destructive forms such as `rm$IFS-rf$IFS/`, `rm${IFS}-rf${IFS}/`, and `dd${IFS}of=/dev/sda` classify correctly across direct, nested-shell, heredoc, and process-substitution paths; quoted, escaped, and non-IFS variable forms stay opaque.
- Restored fail-closed hook test coverage for non-object `tool_input` payloads and centralized production POSIX shell quoting for setup-shell/Codex hook generation (ADR-011).
- `aegis setup-shell` now accepts scoped npm install paths (e.g. `@iliasalmerekov/aegis`); paths are POSIX single-quote escaped in the managed rc block instead of rejected, and errors name whether the real shell path or the Aegis binary path was invalid (ADR-011).
- Codex `SessionStart` hook now emits guidance under `additionalContext` instead of the invalid `context` field, fixing `hook returned invalid session start JSON output` (ADR-011).

### Changed

- The Claude Code `PreToolUse` hook is now a jq-free shim that `exec`s the Rust `aegis hook` (byte-identical to the Codex shim except for its header), replacing the legacy jq-based `aegis-rewrite.sh` script; `install::mod` now shares `write_executable`, `resolved_aegis_bin`, and `combine_outcomes` between the Claude and Codex installers instead of duplicating them (ADR-012).
- Codex `PreToolUse` hook now transparently rewrites unwrapped Bash commands through `aegis --command` (`permissionDecision: "allow"` + `updatedInput`) by delegating to the Rust `aegis hook`, instead of denying and relying on the model to retry. This removes the `jq`/`python3` runtime dependency from the Codex hook (ADR-011).
- The Rust `aegis hook` rewrite now fails closed on commands that begin with the bare `aegis` word but are not a canonical `aegis --command '<...>'` wrapper, and passes canonical wrappers through untouched (ADR-011).
- Installed Codex pre-tool-use hook embeds a shell-quoted absolute Aegis binary path so it works under a minimal hook-exec PATH; an explicit `AEGIS_BIN` still overrides it (ADR-011).

### Added

- npm postinstall best-effort agent hook setup: runs `aegis install-hooks --all` when `~/.claude` or `~/.codex` already exists, prints next steps otherwise, never creates agent directories, and never fails the npm install (opt out with `AEGIS_NPM_SKIP_HOOKS=1`) (ADR-011).

### Changed

- Simplified `README.md` to a minimal public contract (What / Why / Install / How it works) with a visible threat-model link and an honest heuristic-not-a-sandbox statement (M6 docs gate).
- Aligned landing page copy with the current install flow while keeping the existing design (3D shield and section layout unchanged): installer/Homebrew/npm/Cargo, `aegis setup-shell` opt-in, `v0.5.8`, and honest audit wording (append-only; tamper-evident when hash-chain integrity is enabled) replacing the prior overclaim (M6).
- Prepare release metadata for v0.5.8 after the v0.5.7 release build hit the stale `ldd` static-link verification path (M3.2).

### Removed

- Non-production landing source artifacts not used by the runtime: `landing/pencil.pen`, `landing/DESIGN.md`, `landing/tokens.json`, and unused image assets (`landing/images/Hitem3d-1781772057946.glb`, `landing/images/generated-1781681175337.png`) (M6).
- `test_q` stray compiled ELF binary from the repo root (M6).

### Added

- `aegis setup-shell` — explicit opt-in command for shell hook installation (ADR-009)
- Supply-chain gates: `cargo audit` + `cargo deny check` both green in CI (M5.4)
- npm wrapper package with native binary download per platform (M3.4)
- npm checksum updater script for release automation
- GitHub Releases CI: static musl targets for Linux (M3.2)
- Homebrew formula/tap with formula updater (M3.3)
- GitHub Releases with `.sha256` sidecars (M3.5)
- Fuzz corpus CI job at ≥ 100 000 iterations per target (M5.2)
- Snapshot/rollback integration tests in CI (M5.3)

### Fixed

- Fixed C1 uppercase scanner bypass by compiling built-in regex patterns case-insensitively while preserving custom regex case sensitivity.
- Render the README hero GIF through standard Markdown image syntax so GitHub treats it like other animated demos (M6 docs gate).
- Ignore `/test_q` at the repo root so the stray compiled ELF cannot be re-committed (M6).
- Release CI: verify static Linux binaries via `readelf` (ELF headers) instead of `ldd`; fixes false failures on musl `static-pie` (x86_64) and cross-compiled `aarch64` binaries (M3.2)
- `setup-shell`: block symlink recursion and rc injection
- Gate starlark-policy dependency — closed supply-chain lint warnings
- Follow GitHub release redirects in npm installer
- Keep npm package contents minimal

### Security

- `setup-shell` rejects symlink loops and prevents injection into shell rc files

---

## v0.5.6

### Highlights

- **Sandbox bypass is an audit event** (ROADMAP 6.4): every executed command
  now records a `sandbox_status` field in the audit log — `active` (a sandbox
  profile was applied), `unavailable` (a configured sandbox could not be applied
  and the command ran unconfined — a bypass), or `not_configured`. When a bypass
  occurs, Aegis also emits a `WARN` on the `aegis::sandbox` target. Setting
  `sandbox.required = true` still turns unavailability into a hard block.

### Documentation and contracts

- Audit log entries gain the canonical `sandbox_status` field. The legacy
  `sandbox_active` boolean is still written (mirrored from the status) and read,
  so existing log readers and older logs remain compatible.

---

## v0.5.3

### Highlights

- **Binary-first hook installation**: the documented release-install flow now
  describes Claude Code / Codex hook setup as running through the installed
  `aegis` binary when supported agent directories are already present, instead
  of depending on a local repository checkout.
- **Honest skip behavior**: current docs now state that automatic hook setup
  only updates agent directories that already exist and skips missing
  `~/.claude` / `~/.codex` directories without seeding them.
- **Single follow-up command**: README, troubleshooting, and release docs now
  point users at `aegis install-hooks --all` as the supported explicit rerun
  command after agent directories appear later.

### Documentation and contracts

- `README.md` now documents automatic hook setup as a binary-first flow and
  replaces local-checkout-only follow-up guidance with `aegis install-hooks --all`.
- `docs/troubleshooting.md` now explains the skip reason in terms of missing
  agent directories and tells users to rerun `aegis install-hooks --all`.
- `docs/releases/current-line.md` and `docs/releases/v1.0.0.md` now describe
  the binary-first auto-attempt path, skip semantics, and explicit follow-up
  command honestly.

---

## v0.5.1

### Highlights

- **Keyword scanner regression test hardening**: the `keywords.rs` source-slice
  helper used by the hot-path regression test now stops at the actual `mod
tests` boundary instead of relying on a naive split. This keeps literal
  `chars.next().unwrap()` strings inside test-only helpers from causing false
  positives against production-code assertions.
- **Release metadata bumped to 0.5.1**: `Cargo.toml` and `Cargo.lock` now track
  the `0.5.1` crate version for the current release line.
- **Tracker cleanup**: repository-local `REVIEW.md` and `TODO.md` were removed
  from the release tree so the tagged state reflects the current curated docs
  set more closely.

### Documentation and contracts

- `CHANGELOG.md` now tracks the `v0.5.1` release line.
- `docs/releases/current-line.md` now tracks the `0.5.1` release line.
- `docs/releases/v1.0.0.md` now references `0.5.1` as the current pre-1.0
  crate version when describing the future `v1.0.0` target.
- The release documentation continues to describe Aegis as a heuristic shell
  guardrail rather than a sandbox or hard security boundary.

---

## v0.5.0

### Highlights

- **Managed agent-hook install flow**: the current release line now documents
  and ships the local-checkout-only installation path for Claude Code and Codex
  hook payloads, including their shared toggle helper behavior.
- **Global toggle + CI posture clarified**: the `aegis on`, `aegis off`, and
  `aegis status` flow remains part of the public current line, with docs and
  tests aligned around the default-on CI enforcement contract and explicit
  `AEGIS_CI` override semantics.
- **Installer and hook hardening**: deprecated installer controls stay rejected,
  uninstall cleanup removes installed hook payloads and registrations together,
  and hook fallback paths remain best-effort without silently weakening the main
  guardrail contract.
- **Release and architecture docs refreshed**: architecture, install,
  troubleshooting, and release-readiness docs were updated to describe the
  global-first installer and current release workflow honestly.

### Documentation and contracts

- `docs/releases/current-line.md` now tracks the `0.5.0` release line.
- `docs/releases/v1.0.0.md` now references `0.5.0` as the current pre-1.0
  crate version when describing the future `v1.0.0` target.
- The release documentation continues to describe Aegis as a heuristic shell
  guardrail rather than a sandbox or hard security boundary.

---

## v0.4.0

### Highlights

- **Global-first installer flow**: the convenience installer no longer prompts
  for Global / Local / Binary setup modes. It validates shell support up front,
  performs the managed global shell setup path, and prints explicit follow-up
  guidance.
- **Dynamic on/off toggle**: Aegis now exposes `aegis on`, `aegis off`, and
  `aegis status` backed by the global `~/.aegis/disabled` flag.
- **Zero-noise disabled mode**: outside CI, disabled shell-wrapper and
  supported hook usage behave as though Aegis were absent for ordinary command
  flow while still preserving the explicit toggle history.
- **CI override contract**: detected CI environments keep enforcement active by
  default, while `AEGIS_CI` can explicitly override CI detection in either
  direction.
- **Shared hook toggle helper**: Claude Code and Codex hook installations now
  share the managed helper path `~/.aegis/lib/toggle-state.sh`, with fail-safe
  fallback behavior if that helper is missing.
- **Honest install / uninstall behavior**: local hook setup is auto-attempted
  only from a real local checkout, and uninstall now removes both installed
  hook payloads and their JSON registrations.

### Documentation and contracts

- `README.md` now documents the global-first installer, the removed
  `AEGIS_SETUP_MODE` / `AEGIS_SKIP_SHELL_SETUP` controls, and the verified
  disabled / CI-override behavior.
- Added `docs/architecture-decisions.md` to capture the current architecture,
  documented non-goals, toggle / CI decisions, and fuzzing guidance referenced
  by contributor and security docs.
- Troubleshooting and release docs now describe the current installer and hook
  setup behavior instead of the removed interactive setup-mode flow.

---

## v0.3.0

### Highlights

- **Interactive installer with three setup modes**: the install script now asks
  the user to choose Global, Local (project-only), or Binary-only setup.
- **Local mode**: creates `.aegis/enter.sh` in the project directory and
  immediately launches a protected shell — no manual activation needed.
- **ASCII banner**: the installer displays an Aegis banner on startup.
- **Simplified README**: rewritten for clarity with a "Why Aegis exists" section
  explaining the motivation (vibe coders, full-permission agents, accidental
  data loss). Installation is now a single command with an interactive prompt.
- **`AEGIS_SETUP_MODE` env var**: allows CI and scripts to select the setup mode
  non-interactively (`global`, `local`, or `binary`).
- **Supabase snapshot provider**: Aegis can now snapshot and rollback Supabase
  databases before dangerous commands, alongside existing Git, Docker, MySQL,
  and PostgreSQL providers.

### Internal

- Updated contract tests to match the new README structure.
- Fixed duplicate `#[test]` attribute in installer tests.
- Added three new installer integration tests covering all setup modes.

---

## v0.2.0

Release documentation for the current pre-1.0 line tracked by `Cargo.toml`
version `0.2.0`.

### Highlights documented for this release line

- The release workflow is configured to produce GitHub Release artifacts for four targets:
  - `x86_64-unknown-linux-gnu`
  - `aarch64-unknown-linux-gnu`
  - `x86_64-apple-darwin`
  - `aarch64-apple-darwin`
- Each binary is produced with a matching `.sha256` sidecar.
- The install path is configured to verify the downloaded checksum before writing to `BINDIR`.
- The current docs state the supported platform matrix and the known
  limitations of the heuristic guardrail model.
- Troubleshooting and recovery guidance exists for install, checksum, and
  rollback failures.

### What is not claimed

- No SBOM is published by the current release workflow.
- No provenance metadata or attestations are generated or attached by the
  current release workflow.
- This release documentation does not claim byte-for-byte reproducible builds
  across all environments.

### Reference docs

- [Current release line](docs/releases/current-line.md)
- [Planned v1.0.0 release summary](docs/releases/v1.0.0.md)
- [Release and CI guarantees](docs/ci.md)
- [Platform support](docs/platform-support.md)
- [Threat model](docs/threat-model.md)
- [Troubleshooting and recovery](docs/troubleshooting.md)
