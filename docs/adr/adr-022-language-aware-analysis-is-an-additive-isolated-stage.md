# ADR-022: Language-aware analysis is an additive isolated stage

## Status

Accepted

## Context

Aegis currently classifies the shell command text that an agent asks it to
execute. The shell tokenizer, Aho-Corasick Quick scan, regex Patterns, and
Token-prefix rules cover visible command shapes well, but they cannot reliably
describe operations hidden inside source-language syntax. Examples include
`python -c "os.remove(path)"`, `node -e "fs.rmSync(path)"`, interpreter stdin,
heredocs, and named script files.

Replacing the shell scanner with a language parser would be the wrong trade-off.
The existing scanner owns shell boundaries, launcher normalization, fast common
cases, and the sub-2-ms safe-command target. A language parser also cannot prove
what arbitrary code will do: imports, dynamic dispatch, runtime values, generated
code, native extensions, and time-of-check/time-of-use changes remain outside a
bounded static assessment.

Tree-sitter provides production-quality concrete syntax trees and error-tolerant
parsing for many languages, but its runtime and generated grammars introduce
native C build inputs, binary-size cost, grammar supply-chain risk, and parser
isolation requirements. The project therefore needs an explicit boundary rather
than treating Tree-sitter as a drop-in scanner replacement.

The target user population runs more than Python and JavaScript. The production
language set is Shell/Bash, Python, JavaScript, TypeScript, PowerShell, PHP, Ruby,
Go, Perl, and Lua. Shipping every adapter in one unqualified release would make
correctness, platform parity, and review unmanageable.

## Decision

### 1. Preserve shell detection and add a bounded slow path

Language-aware analysis complements the existing Scanner. It does not replace
the shell tokenizer, Quick scan, regex Patterns, or Token-prefix rules. Existing
Matches are monotonic: later analysis may add Matches, raise RiskLevel, or record
Analysis degradation; it may never remove a Match or lower RiskLevel.

The no-source safe-command path remains synchronous and must stay below 2 ms.
`Scanner::assess` remains pure and performs no filesystem access. Language-aware
analysis runs only when command-visible source or a supported script invocation
creates an analysis target.

Script source inspection is an asynchronous stage after the baseline Assessment
and before Policy evaluation. The parent process performs bounded source routing
and reads using Tokio, merges all analysis results into one Assessment, and only
then asks the PolicyEngine for a Decision.

### 2. Isolate parsing in an ephemeral worker

Tree-sitter parsing and language adapters run in a self-spawned, ephemeral worker
process. One worker session may accept a bounded sequence of analysis requests for
one intercepted command and exits when that Assessment is complete. There is no
daemon, runtime plugin loader, or network service.

The parent and worker communicate over a versioned pipe protocol. Source bytes are
sent through the protocol; temporary source files are forbidden. The worker parses
only the supplied bytes and may not read the filesystem or execute subprocesses.
The parent owns recursive routing, limits, result merging, and policy integration.

A worker crash, timeout, incompatible protocol response, or invalid response is an
Analysis degradation. Results already produced by the shell Scanner or an earlier
analysis target are retained.

Language adapters and Tree-sitter dependencies live behind one focused workspace
library boundary, provisionally `aegis-language`. Root-binary code owns async
orchestration and delegates worker analysis to that library; `main.rs` remains
CLI parsing and orchestration only.

### 3. Normalize syntax into Detected operations

Each adapter uses Tree-sitter queries for structural capture and typed Rust code
for semantic interpretation. It emits language-neutral Detected operations rather
than assigning risk directly from an API spelling. A shared classifier maps:

- operation kind and effect scope;
- modifiers such as recursive, forced, or destructive mode; and
- Operand certainty (`Known`, `Partial`, or `Dynamic`)

into the existing Category, RiskLevel, explanation, safer alternative, and Match
vocabulary.

Bounded symbol resolution follows only direct imports, aliases, adjacent literals,
literal concatenation, simple constant bindings, and language escapes inside one
source target. It excludes type inference, interprocedural analysis, dependency or
import traversal, arbitrary data-flow analysis, and intent inference.

A recognized process, shell, or eval sink always emits a CodeExecution Match. A
literal payload also becomes a bounded recursive analysis target. A dynamic payload
records Analysis degradation in addition to the CodeExecution Match; uncertainty
about the payload never hides the visible execution sink.

The initial operation scope is destructive effects plus code-execution sinks:
filesystem deletion and overwrite/truncation, dangerous permission or ownership
changes, device or critical-path writes, destructive database operations, literal
shell/process/eval payloads, and a deliberately selected set of destructive cloud,
container, and package APIs. Generic HTTP/exfiltration detection, malware or
vulnerability scanning, taint analysis, and general program comprehension are
non-goals.

### 4. Use a common Detection rule and evidence model

Before 1.0, the scanner model will expose a common Detection rule contract with
three concrete mechanisms: regex Pattern, Token-prefix rule, and Language-aware
rule. Language-aware rules are built in. Project configuration cannot define
custom Tree-sitter queries, disable a built-in semantic rule, or lower its
classification.

Every Match carries typed evidence. Language-aware evidence records a Detected
operation, Operand certainty, and Analysis provenance. The current singular
`DecisionSource` concept is replaced by Assessment basis: all decisive Match IDs
at the Assessment's maximum RiskLevel, or `Fallback` when nothing matched. Each
Match also identifies its detection mechanism and whether it is built in or
custom.

Analysis status is typed as `NotApplicable`, `Complete`, or `Degraded`, with
per-target results and typed degradation reasons. Reasons include unsupported or
unavailable grammar, incomplete syntax, unsafe or unavailable source, unsupported
encoding, size/count/depth/timeout limits, dynamic source or cwd, and worker or
protocol failure.

### 5. Fail closed without inventing a synthetic RiskLevel

Analysis degradation is orthogonal to RiskLevel; it is not a synthetic `Warn`.
It may coexist with `Safe`, but it never authorizes safe auto-execution.

In enforcing `Protect`, a degraded Assessment requires explicit one-time approval.
`Strict` continues to block non-safe and Indirect execution by default, but this ADR
adds one narrow exception: an interactive, non-persistable Analysis override may
approve a non-`Block` language-aware Match or degradation. It does not authorize an
unrelated Strict denial. Non-interactive degradation is denied. Trusted global
`Audit` mode remains observe-only and records the degradation. An intrinsic `Block`
remains unbypassable in every posture.

Completed language-aware `Warn` and `Danger` results use the existing CI policy.
In interactive Protect/Strict flows, a language-aware Match or degradation cannot
be auto-approved by an allowlist or a policy-rule `Allow`; only a one-time approval
or Analysis override, respectively, is valid. Neither authorization is persistable.
Audit mode and the global Toggle remain explicit trusted posture controls.

The TUI presents one consolidated Assessment confirmation containing the decisive
Matches and any degradation, with other Matches available as detail. It does not
prompt once per operation. A Required recovery degradation may still need its
separate recovery prompt because it approves a different failure boundary.

### 6. Treat script-file reads as catch-only evidence

Script source inspection reads only local regular files through the caller's
permissions and within explicit budgets. Symlinks, FIFOs, sockets, devices,
directories, unavailable paths, unsafe metadata, and unsupported encoding produce
Analysis degradation rather than a read. Absolute paths are allowed. The parent
records the inspected bytes' hash and descriptor metadata without claiming that
the interpreter will later execute those same bytes.

Successful inspection may add Matches and raise RiskLevel, but it does not remove
Effect-opaque execution or waive Required recovery. This preserves the backstop for
TOCTOU changes, imports, generated code, and other runtime effects outside the read
source.

Routing precedence is explicit interpreter, then verified shebang for a directly
executed file, then target extension for a file created by a visible heredoc.
Explicit interpreter wins over extension. Routing uses a built-in canonical
interpreter/runner registry, basename and versioned-name normalization, existing
Launcher-prefix logic, and trusted global aliases only. It performs no `PATH`,
`--version`, or content-guessing probes. Package/build runner expansion such as
`npm run`, `go generate`, Make, Composer, Bundler, and framework CLIs is a v1
non-goal.

The parent tracks only a literal top-level `cd -- <path> &&` cwd change. Dynamic
`cd`, `pushd`, substitutions, or otherwise unresolved cwd cause degradation.

Quoted heredocs provide exact source. Expanding heredocs are analyzed as the visible
template, but expansions, substitutions, or relevant escapes also record
degradation; Aegis never evaluates them. An in-memory heredoc body may be linked to
a later invocation of the created file in the same shell command.

Interpreter stdin is analyzed only when source is statically recoverable: a quoted
heredoc, literal here-string, or a narrowly proven literal-only producer such as
`printf '%s'`. Dynamic pipelines remain Effect-opaque and degrade honestly.

### 7. Bound recursive and encoded analysis

Literal process or eval payloads become new targets in a bounded cross-language
work queue. Targets are deduplicated by language and source hash. Depth, target
count, aggregate bytes, and total time are capped; cycles or exhausted budgets
produce degradation while preserving prior Matches.

Dynamic process, shell, and eval payloads are not enqueued or evaluated. The
visible sink still emits a CodeExecution Match and the unresolved payload produces
Analysis degradation.

The initial recursion-depth ceiling is 8. The implementation plan must derive the
remaining defaults from prototype benchmarks. Initial research values are:

- existing inline-source limit: 16 KiB;
- script-file default: 256 KiB;
- hard per-file ceiling: 1 MiB;
- maximum script files per command: 8;
- maximum aggregate source: 1 MiB; and
- total language-analysis timeout: 100 ms.

Defaults are configurable within non-configurable hard ceilings. Project config may
only tighten them; trusted global config may tune them within the ceilings.

The pre-1.0 encoding contract is UTF-8, with a UTF-8 BOM mapped back to original
byte spans. Source hashes cover the original bytes. Invalid UTF-8 and UTF-16 degrade;
UTF-16 support is deferred with the PowerShell 1.x adapter.

Base64, hex, gzip, encryption, and custom payload decoding are out of scope. A
decode-to-eval shape still emits a code-execution operation and degradation rather
than pretending the decoded behavior is known.

### 8. Ship qualified grammars in one binary

Official release binaries statically include every production-qualified grammar.
They never download grammars at runtime and never load dynamic grammar libraries.
The qualified language set must be identical across:

- `x86_64-unknown-linux-musl`;
- `aarch64-unknown-linux-musl`;
- x86_64 macOS; and
- aarch64 macOS.

A grammar is eligible only after independent qualification of license, maintenance,
Tree-sitter ABI compatibility, Rust binding, pinned version or commit, `build.rs`
and bundled native source, transitive dependencies, upstream corpus, Aegis security
corpus, fuzzing, and all-target release builds. Official upstream grammars are
preferred; community grammars must pass the same gate. The release contains a
grammar manifest with versions, provenance, and licenses.

This ADR creates a narrow exception to the project's no-C-build preference: only
the pinned Tree-sitter runtime and production-qualified generated grammars may add
native C compilation. It is not permission for general native dependency growth.
Any other C/native dependency still requires a separate ADR and release-matrix
evidence.

### 9. Stage production enablement by language

The pre-1.0 milestone delivers the common foundation plus Python, JavaScript,
TypeScript, and Shell/Bash. Go, PHP, Ruby, PowerShell, Perl, and Lua follow as
independently qualified 1.x adapters.

Qualified adapters are default-on; there is no separate
`language_analysis.enabled = false` escape hatch. Existing Audit and Toggle controls
remain the trusted posture controls. An adapter remains unsupported, and produces
honest degradation when its source is encountered, until every qualification gate
passes. Release enablement is per language rather than a big-bang switch.

### 10. Extend audit without persisting source

Audit schema v2 adds typed Matches, Assessment basis, analysis status and
provenance, and stable detection IDs. Existing `matched_patterns` and `pattern_ids`
remain as compatibility projections. Absence of v2 fields identifies a legacy v1
line; logs are never rewritten, and mixed v1/v2 querying and integrity verification
must work. The hash chain covers the actual serialized form of each entry.

Analysis provenance may persist language, source origin, rule ID, operation, file
path when applicable, source hash, line/column/byte span, Operand certainty, status,
and degradation reason. It must not persist script contents, full snippets, imported
source, variable values, or syntax trees. The TUI may render a short in-memory
snippet, but it does not write that snippet to the Audit log.

Aegis adds no automatic network telemetry. A local-only aggregation command may
summarize interpreter/language, invocation shape, status, latency, and size buckets;
export is an explicit user action. Real Audit logs are never automatically uploaded
or used as test fixtures.

### 11. Qualify behavior, not only parsers

Each language adapter must pass the same production gate: pinned grammar and license
evidence; all-target artifact parity; positive, negative, alias, literal, malformed,
and new-syntax corpora; worker crash/timeout/protocol tests; resource-limit and
recursive cross-language tests; adapter and protocol fuzzing; Audit v1/v2
compatibility; Shell, Watch, Hook, and CI integration; safe-hot-path benchmarks; and
slow-path latency, memory, and binary-size budgets.

The workspace test, clippy, format, audit, deny, review, re-review, release-build,
performance, live integration, and fuzz gates remain mandatory. A parser that merely
builds is not a supported language.

## Consequences

### Positive

- Dangerous operations visible in supported source syntax can produce the same
  typed Assessment and Policy behavior as shell-level detections.
- The existing fast path and shell semantics remain intact.
- Worker isolation contains parser crashes, separates parser address space, permits
  a parent-enforced timeout, and makes peak-memory budgets measurable without a
  resident daemon. It does not claim a portable hard memory cap.
- Shared Detected operations reduce semantic drift between language adapters.
- Honest degradation makes unsupported or dynamic cases visible instead of silently
  treating them as safe.
- Per-language qualification permits broad eventual coverage without coupling every
  adapter to the 1.0 release.

### Negative

- Official binaries grow because qualified grammars are statically linked.
- Builds gain a narrowly scoped native C toolchain requirement.
- A worker process adds latency to commands that expose analyzable source.
- Audit schema, TUI, config ratchets, CI, release packaging, and policy integration
  all require coordinated migration.
- The source reader handles sensitive local files, so metadata checks, privacy
  limits, and adversarial tests become part of the security-sensitive attack
  surface.
- Strict gains a narrow Analysis override for non-`Block` language-aware Matches
  and degradation; every unrelated Strict denial remains terminal.

### Residual limits

- This remains a heuristic guardrail, not a sandbox or proof of program behavior.
- Dynamic dispatch, runtime imports, generated or encoded payloads, native code,
  unresolved values, and TOCTOU changes can hide effects.
- Successful Script source inspection never makes Script-file execution trusted.
- Unsupported languages and unqualified adapters degrade rather than receiving a
  weaker best-effort parser.
- Package/build runner expansion and dependency traversal require later decisions.
