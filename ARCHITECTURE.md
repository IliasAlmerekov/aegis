# ARCHITECTURE.md — Aegis System Architecture

> **⚠ Partially stale (flagged 2026-07-09 checkup).** Several concrete paths in
> this document predate the multi-crate extraction and no longer exist as
> written: `src/decision/engine.rs`, the `src/interceptor/scanner/*` and
> `src/interceptor/parser/*.rs` trees, and `src/config/*.rs` are now thin shims
> whose real code lives under `crates/` (see `CONVENTION.md` for the current
> map). §7's "Current breaches" table cites `src/snapshot/*.rs` files that were
> moved into `crates/aegis-snapshot` and split. The per-file LoC budget quoted
> here (1500/2000) is superseded by the **800**-line budget in `ROADMAP.md`
> §3.1 / `CONVENTION.md`. Treat crate-level boundaries and
> invariants as authoritative; treat `src/...` file paths as needing
> verification against the tree until this document is revised.
>
> **Status:** authoritative for structural contracts. When code and this
> document disagree, one of them is a bug — fix whichever is wrong, do not let
> them drift.
>
> **Scope:** structural contracts (module boundaries, invariants, request
> lifecycles, extension points). For _why_ decisions were made, see
> `docs/adr/README.md`. For code style and dependency policy, see
> `CONVENTION.md`.

---

## Table of Contents

1. [System Boundary](#1-system-boundary)
2. [The Seven Layers](#2-the-seven-layers)
3. [Request Lifecycles](#3-request-lifecycles)
4. [Module Boundaries (who may call whom)](#4-module-boundaries-who-may-call-whom)
5. [Invariants](#5-invariants)
6. [Extension Points](#6-extension-points)
7. [File Size Budgets](#7-file-size-budgets)
8. [Public API Surface](#8-public-api-surface)
9. [Glossary](#9-glossary)

---

## 1. System Boundary

Aegis is a **local Rust CLI centered on the `aegis` binary** that sits between
an AI agent (or human) and a real shell. It receives a candidate shell
command, classifies its risk, optionally prompts for human confirmation,
optionally creates a rollback snapshot, and finally executes the command — or
refuses to.

The repository also contains two auxiliary binaries:
- `src/bin/aegis_benchcheck.rs` — benchmark-policy checking.
- `src/bin/aegis_schema.rs` — generates `aegis-schema.json` from `AegisConfig`.

Neither is part of the interception path described in this document.

### What Aegis is

- A **local, synchronous** policy gate. Every decision is made on the same
  machine, in-process, before the underlying command starts.
- A **shared policy evaluator**: shell-wrapper execution, watch mode, and
  evaluation-only JSON all converge on the same planning and policy path; hook
  integrations may only route commands into that path, never fork policy.
- An **append-only audit recorder**. When `[audit].integrity_mode =
"ChainSha256"`, audit segments are linked by an integrity chain that detects
corruption and inconsistent edits.

### What Aegis is NOT

- The command guardrail is not a confidentiality boundary. An optional Sandbox
  write/network guardrail can constrain approved commands, but does not promise
  to hide readable files or secrets.
- Not a network service or resident control plane. There is no server and no
  long-lived daemon; integrations are direct local CLI invocations plus
  stdin/stdout protocols.
- Not a linter or static analyzer. It sees the exact command about to run, not
  source code.
- Not a retry or scheduling layer. The command either runs once or it doesn't.

---

## 2. The Seven Layers

```
┌─────────────────────────────────────────────────────────────────────────┐
│ 1. Entrypoint        src/main.rs + CLI/shell glue                       │
│                      (cli_dispatch.rs, cli_commands.rs,                 │
│                       shell_compat.rs, shell_wrapper.rs, rollback.rs,   │
│                       policy_output.rs, toggle.rs, runtime_gate.rs)     │
├─────────────────────────────────────────────────────────────────────────┤
│ 2. Policy Engine     src/decision/  +  src/planning/                  │
│                      (pure function: PolicyInput → PolicyDecision)      │
├─────────────────────────────────────────────────────────────────────────┤
│ 3. Scanner           src/interceptor/                                   │
│                      (command → Assessment: RiskLevel + matched)        │
├─────────────────────────────────────────────────────────────────────────┤
│ 4. Approval Flow     src/shell_flow.rs, src/watch/, src/install/    │
│                      + src/ui/confirm.rs (TUI)                          │
├─────────────────────────────────────────────────────────────────────────┤
│ 5. Snapshot Layer    crates/aegis-snapshot/ (plugin trait + 6 built-ins) │
├─────────────────────────────────────────────────────────────────────────┤
│ 6. Audit Layer       src/audit/ (append-only JSONL + optional hash chain)│
├─────────────────────────────────────────────────────────────────────────┤
│ 7. Agent Protocols   watch (NDJSON stdin/stdout)                        │
│                      hook  (Claude Code PreToolUse JSON)                │
└─────────────────────────────────────────────────────────────────────────┘
     Support:  src/config/   src/runtime/   src/explanation/   src/error.rs
```

### 2.1 Entrypoint — `src/main.rs` + CLI/shell glue

**Responsibility:** parse CLI, build the Tokio runtime, route to one mode, and
render command-oriented surfaces without reimplementing policy.
**Must NOT contain:** business logic, policy evaluation, duplicated planning,
or transport-specific policy forks.

- `main.rs` — clap definitions, `InvocationMode` dispatch, exit-code contract,
  one process-wide `tokio::Runtime`.
- `cli_dispatch.rs` — maps `Commands::{Watch, Audit, On, Off, Status,
Rollback, Config, Hook, Install}` and `--command <cmd>` to their handlers.
- `cli_commands.rs` — focused handlers for audit/config/toggle/status flows.
- `shell_compat.rs` — handles the three invocation modes (`Cli`,
  `ShellCompatCommand`, `ShellCompatSession`) so Aegis can be set as `$SHELL`.
- `shell_wrapper.rs` — bridges shell-wrapper invocations into planning and
  output rendering.
- `rollback.rs` — rollback CLI path (`aegis rollback <snapshot-id>`).
- `policy_output.rs` — evaluation-only JSON rendering for `--output json`.
- `toggle.rs` — file-based on/off switch (`~/.aegis/disabled`); handles
  `aegis on`, `aegis off`, `aegis status`.
- `runtime_gate.rs` — `is_ci_environment()`; used by the entrypoint and
  planning to gate CI policy without duplicating detection logic.

Three invocation modes (`src/shell_compat.rs`):

1. `Cli` — regular `aegis <subcommand>`.
2. `ShellCompatCommand { command, launch }` — parent invoked
   `aegis -c "<cmd>"`. Goes to `shell_wrapper::run_shell_wrapper`.
3. `ShellCompatSession { launch }` — parent invoked `aegis` as `$SHELL` with no
   command. Aegis execs an interactive shell (cannot intercept, but preserves
   the session).

Exit-code contract — **stable public API**:

| Code | Meaning                                                                             |
| ---- | ----------------------------------------------------------------------------------- |
| 0    | Success — command was approved and exited 0, or a maintenance subcommand succeeded. |
| 1-N  | Pass-through — the wrapped shell/child ran and returned this code.                  |
| 2    | `EXIT_DENIED` — user pressed 'n' at the confirmation dialog.                        |
| 3    | `EXIT_BLOCKED` — matched a `Block`-level pattern, no dialog.                        |
| 4    | `EXIT_INTERNAL` — Aegis/config error or validation failure.                         |

Codes 2, 3, 4 are the values **Aegis itself** emits for deny/block/internal
outcomes. Approved commands still propagate the wrapped shell/child exit code,
so a child may numerically return the same values. Callers that need
collision-free decision data must use `--output json` or watch-mode result
frames. Changing Aegis' own 2/3/4 mapping is a breaking change.

### 2.2 Policy Engine — `src/decision/` + `src/planning/`

**Responsibility:** given a fully collected input (assessment, mode, CI,
allowlist, transport), produce a decision. Nothing else.

**`src/decision/engine.rs` (`evaluate_policy`) is a pure function.**
No I/O, no filesystem, no process spawning, no logging, no global state.

```rust
pub fn evaluate_policy(input: PolicyInput<'_>) -> PolicyDecision
```

- `PolicyInput` carries: `&Assessment`, `Mode`, `PolicyCiState`,
  `PolicyAllowlistResult`, `PolicyConfigFlags`, `PolicyExecutionContext`.
- `PolicyDecision` carries: `PolicyAction ∈ {AutoApprove, Prompt, Block}`,
  `rationale`, `snapshots_required`, `allowlist_effective`,
  `requires_confirmation`.
- `BlockReason ∈ {IntrinsicRiskBlock, ProtectCiPolicy, StrictPolicy}`.
- `ExecutionTransport ∈ {Shell, Watch, Evaluation}` — explicit in policy input
  so transports cannot silently fork policy, even though current allow/deny
  semantics are shared across these transports.

**`src/planning/` is the orchestrator that wraps the pure engine.**
It is the _single_ entry point every transport uses.

```
src/planning/
├── mod.rs       public API: plan_with_context, prepare_and_plan, PreparedPlanner
├── core.rs      plan_with_context: assess → allowlist → snapshot-plugins →
│                evaluate_policy → InterceptionPlan
├── prepare.rs   prepare_and_plan / prepare_planner: lazy RuntimeContext
└── types.rs     PlanningOutcome, InterceptionPlan, DecisionContext, CwdState,
                 ApprovalRequirement, ExecutionDisposition, SnapshotPlan,
                 SetupFailureKind, SetupFailurePlan
```

**Rule:** shell-wrapper execution, watch mode, and evaluation-only JSON all go
through `planning::*`. `install::run_hook` is **not** a policy surface; it may
only rewrite supported Bash invocations back into shell-wrapper flow. No
transport may reimplement any part of the decision.

### 2.3 Scanner / Interceptor — `src/interceptor/`

**Responsibility:** parse a raw shell command and compute an `Assessment`
(`RiskLevel` + matched patterns + highlights).

Two-stage engine (hot path must stay ≤ 2 ms for safe commands):

1. **Quick scan** — Aho-Corasick automaton over literal keywords. One linear
   pass, allocation-free. `false` → immediate `Safe`. Relative to the full
   regex set, quick scan must not introduce false negatives; false positives
   are fine.
2. **Full scan** — compiled `regex::Regex` per pattern, run only if quick scan
   matched something.

```
src/interceptor/
├── mod.rs              RiskLevel, assess(), scanner_for(), global caches
├── patterns.rs         Pattern, BuiltinPattern, UserPattern, PatternSet::load
├── nested.rs           recursive scanning of nested scripts (bounded depth)
├── scanner/
│   ├── mod.rs                   Scanner (AC + regex), quick_scan → full_scan
│   ├── assessment.rs            Assessment, DecisionSource, MatchResult
│   ├── keywords.rs              literal-keyword extraction from regex
│   ├── pipeline_semantics.rs    | && ; handling
│   ├── recursive.rs             nested-script wrapper
│   ├── highlighting.rs          HighlightRange (for UI)
│   └── prefix_rule.rs           PrefixRule token matching + example validation
└── parser/
    ├── mod.rs                   ParsedCommand, PipelineSegment, public API
    ├── tokenizer.rs             split_tokens
    ├── segmentation.rs          logical_segments, top_level_pipelines
    ├── embedded_scripts.rs      heredoc, python -c, node -e, eval, $( … )
    └── nested_shells.rs         extract_nested_commands
```

Built-in patterns live in `crates/aegis-scanner/patterns.toml` (embedded at
compile time and loaded via `PatternSet::load`). User patterns come from `aegis.toml` and are merged per
effective config; the merged scanner is cached by content hash in
`CUSTOM_SCANNER_CACHE` (`src/interceptor/mod.rs`).

`MAX_SCAN_COMMAND_LEN = 64 KiB` and `MAX_INLINE_SCRIPT_LEN = 16 KiB` cap
scanner input to bound worst-case work.

### 2.4 Approval Flow — `src/shell_flow.rs`, `src/watch/`, `src/install/`

**Responsibility:** execute an `InterceptionPlan`. This is where side effects
happen — snapshots, TUI, exec, audit append.

Three transports share the `InterceptionPlan` shape but differ in how they
collect input and emit output:

- **`shell_flow::run_planned_shell_command`** — shell-wrapper execution path.
  - `Execute` → snapshot → Sandbox `prepare_for_exec` → audit → optional warning
    → process replacement.
  - `RequiresApproval` → `ui::confirm::show_confirmation` → snapshot → any
    Recovery override → Sandbox preparation → audit → optional warning → exec,
    or `EXIT_DENIED`.
  - `Block` → `ui::confirm::show_policy_block` or `show_confirmation` depending
    on `BlockReason` → audit → `EXIT_BLOCKED`.
- **`watch::run`** — `aegis watch` NDJSON loop.
  - Reads `InputFrame { cmd, cwd?, interactive?, source?, id? }` from stdin.
  - Writes `OutputFrame ∈ {Warning, Stdout, Stderr, Result, Error}` to stdout.
  - Sandbox `prepare_for_spawn` never applies Landlock to the persistent Watch
    parent. Optional unavailability emits `sandbox_status = "unavailable"`
    before child output; `sandbox.required = true` produces a blocked result.
  - Prompts are drawn on **TTY directly** (not stdout — stdout is the frame
    channel) via `ui::confirm::show_*_via_tty`.
  - `MAX_FRAME_BYTES = 1 MiB`, `CHANNEL_CAPACITY = 64`.
- **`install::run_hook`** — Claude Code `PreToolUse` hook.
  - Reads Claude's JSON, rewrites supported Bash commands to
    `aegis --command '<cmd>'`, and lets the shell-wrapper path handle the rest.
  - `install::run_install(--local?)` patches Claude Code settings and also
    installs Codex hook scripts when `~/.codex/` is present.

  ```
  src/install/
  ├── mod.rs    public API: run_install, run_hook
  ├── hook.rs   JSON rewriter for PreToolUse payloads
  ├── claude.rs settings.json patching for Claude Code
  └── codex.rs  hook registration for ~/.codex/
  ```

**TUI — `src/ui/confirm/`:**

- `confirm_screen.rs` — `show_confirmation(assessment, explanation, &snapshots)`
  and `show_confirmation_with_input` (approve/deny dialog with highlighted
  pattern matches).
- `block_screen.rs` — non-interactive block screen for `ProtectCiPolicy` /
  `StrictPolicy`.
- `stdout_renderer.rs` — renders confirmations and blocks to stdout (used by the
  shell-wrapper path).
- `tty_renderer.rs` — `show_*_via_tty` variants that open `/dev/tty` directly
  (used by the watch transport so stdout remains the NDJSON frame channel).
- `shared.rs` — common rendering helpers and prompt-decision types shared by
  both renderers.

### 2.5 Snapshot Layer — `crates/aegis-snapshot/` (re-exported via `src/snapshot/`)

**Responsibility:** create and roll back state snapshots. Plugin-based so new
backends can be added without changing the policy engine.

```rust
#[async_trait]
pub trait SnapshotPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    async fn is_applicable(&self, cwd: &Path) -> bool;
    async fn snapshot(&self, cwd: &Path, cmd: &str) -> Result<String>;
    async fn rollback(&self, snapshot_id: &str) -> Result<()>;
}
```

Six built-in providers: `git`, `docker`, `postgres`, `mysql`, `sqlite`,
`supabase`. Registered by name in `BUILTIN_SNAPSHOT_PROVIDER_NAMES`.

**Lazy materialization is an invariant.** `RuntimeContext` holds the registry
in `OnceLock<SnapshotRegistry>` and only materializes it when a command is
actually `Danger` _and_ `snapshot_policy != None`. Safe and Warn commands must
not build the registry. This is verified by tests in `src/planning/core.rs`
(`safe_command_plan_does_not_materialize_snapshot_registry` and siblings).

Two config paths:

- `SnapshotRegistry::from_config` — honors per-plugin `auto_snapshot_*` flags
  and `SnapshotPolicy ∈ {None, Selective, Full}`.
- `SnapshotRegistryConfig::for_rollback_from_config` — forces all built-ins
  on so `aegis rollback <id>` can restore snapshots made under an older
  config.

### 2.6 Audit Layer — `src/audit/`

**Responsibility:** record every decision to an append-only log, with an optional
integrity chain when integrity mode is enabled.

- Format: JSONL at `~/.aegis/audit.jsonl` (plus rotated segments).
- Integrity: when `[audit].integrity_mode = "ChainSha256"`, each segment
  carries the SHA-256 of the previous segment's terminal hash;
  `AuditLogger::verify_integrity` walks the whole chain.
- `AuditEntry` fields include: timestamp, command, matched patterns,
  `Decision ∈ {AutoApproved, Approved, Denied, Blocked}`, `transport`,
  snapshots, allowlist match, CI flag, and the full `CommandExplanation`.

**Rule:** audit is append-only. The file is never rewritten, only appended to
or rotated. Rotation creates a new segment; old segments are immutable.

CLI (`aegis audit`): filters by `--risk`, `--since`, `--until`,
`--command-contains`, `--decision`; formats `text | json | ndjson`;
`--summary`; `--verify-integrity`.

### 2.7 Agent Protocols — watch + hook

Two stable protocols let agents integrate with Aegis.

**Claude Code PreToolUse hook** (`aegis hook`, `aegis install`):

- Installed into `~/.claude/settings.json` or `./.claude/settings.json`.
- Reads Claude's JSON tool-call payload, rewrites `Bash` commands to go
  through `aegis --command`, passes everything else through untouched.

`aegis install` also manages Codex hook registration when `~/.codex/` exists,
but the Rust hook command itself remains the same PreToolUse JSON rewriter.

**NDJSON watch mode** (`aegis watch`):

One `InputFrame` per stdin line; one or more `OutputFrame`s per command on
stdout. Both streams are newline-delimited JSON (NDJSON). This is a
**first-class public protocol** — changing field names, removing fields, or
changing the set of `type` values is a breaking change (see invariant I13).

`InputFrame` fields:

| Field         | Type   | Required | Description                                              |
|---------------|--------|----------|----------------------------------------------------------|
| `cmd`         | string | yes      | The shell command to intercept.                          |
| `cwd`         | string | no       | Working directory for the command.                       |
| `id`          | string | no       | Caller-assigned correlation token; echoed in all output frames for this command. |
| `source`      | string | no       | Agent / tool name for audit attribution.                 |
| `interactive` | bool   | no       | Reserved; ignored in v1.                                 |

`OutputFrame` variants (discriminated by the `type` field):

| `type`   | Fields                                        | Description                                                     |
|----------|-----------------------------------------------|-----------------------------------------------------------------|
| `stdout` | `id?`, `data_b64: string`                     | Child stdout chunk; `data_b64` is standard Base64-encoded bytes. |
| `stderr` | `id?`, `data_b64: string`                     | Child stderr chunk; same encoding.                              |
| `result` | `id?`, `decision`, `exit_code: i32`           | Terminal frame. `decision ∈ {approved, denied, blocked, error}`. |
| `error`  | `id?`, `exit_code: i32`, `message: string`    | Protocol-level error (bad JSON, oversized frame, internal fault). |

`decision: error` in a `result` frame signals that Aegis could not execute
the command due to an internal error after the policy decision was made.
`exit_code` in an `error` frame is always `4` (`EXIT_INTERNAL`).

`MAX_FRAME_BYTES = 1 MiB`. Frames that exceed this limit are rejected
**before allocation**; Aegis emits an `error` frame and reads the next line.

Both protocols are public contracts. Changing them is a breaking change.

### 2.8 Support Layers

- **`src/config/`** — layered config loading. Precedence: built-in defaults →
  `~/.config/aegis/config.toml` (user) → `./.aegis.toml` (project).
  `AegisConfig` +
  `Allowlist` + `validate_config_layers`. All new fields must be optional with
  `#[serde(default)]`.
  ```
  src/config/
  ├── mod.rs            public API: load, effective_config, config_path helpers
  ├── model.rs          AegisConfig + layered loading + merge logic
  ├── model/
  │   ├── enums.rs      Mode, CiPolicy, AuditIntegrityMode, SnapshotPolicy,
  │   │                 AllowlistOverrideLevel — all with schemars::JsonSchema
  │   ├── rules.rs      UserPattern, AllowlistRule, BlockRule, AuditConfig
  │   │                 — all with schemars::JsonSchema and field docs
  │   └── tests.rs      config model unit tests
  ├── snapshot.rs       snapshot-provider config structs (Postgres, MySQL, …)
  ├── allowlist.rs      allowlist / blocklist public API and rule compilation
  ├── allowlist/
  │   ├── analysis.rs   allowlist match analysis and overlap detection
  │   └── compile.rs    compiled trie / prefix-map building
  ├── amend.rs          append-only config editing (allow / block rule insertion)
  │   └── amend/
  │       ├── formatting.rs  TOML serialisation helpers
  │       └── validation.rs  amend pre-condition checks
  └── validate.rs       cross-layer validation (e.g. blocklist vs allowlist overlap)
  ```
- **`src/runtime/`** — `RuntimeContext`: scanner, allowlist, snapshot
  registry (lazy), audit logger, async handle, effective `RuntimeConfig`.
  Built exactly **once per CLI invocation**.
  ```
  src/runtime/
  ├── mod.rs       re-exports
  ├── context.rs   RuntimeContext, RuntimeConfig, WatchAuditContext
  └── user.rs      effective-user detection for scoped allowlist / audit
  ```
- **`src/explanation/`** — `CommandExplanation { scan, policy, context,
  outcome }`. Deterministic, serializable; consumed by UI, audit, and
  `--output json`.
  ```
  src/explanation/
  ├── mod.rs       public API: explain(), CommandExplanation
  ├── formatter.rs plain-text / JSON formatting
  └── templates.rs message templates for each PolicyRationale variant
  ```
- **`src/policy_output.rs`** — evaluation-only mode
  (`aegis --command "<cmd>" --output json`), emits policy decision without
  executing.
- **`src/error.rs`** — `AegisError` via `thiserror`. Library modules return
  `Result<T, AegisError>`; CLI glue may use `anyhow`.

### 2.9 Language-aware analysis boundary — `crates/aegis-language/`

The focused workspace boundary that owns Tree-sitter parsing, the grammar
manifest, and (in later iterations) language adapters that emit
language-neutral detected operations. Per ADR-022 it is an **additive slow
path**: it never replaces the shell `Scanner` and never regresses the
no-source safe-command hot path. Parsing runs in an ephemeral worker process
(no daemon, no plugin loader, no network service). It is the only crate
permitted to pull in the narrowly scoped native C toolchain — the pinned
Tree-sitter runtime plus the four production-qualified L1 grammars
(Python/JavaScript/TypeScript/Shell-Bash) — and it must **not** be depended on
by `aegis-types` or any other workspace member (ADR-022 §4; enforced by
`tests/architecture_boundaries.rs`).
```
crates/aegis-language/
├── src/
│   ├── lib.rs        crate root + public re-exports
│   ├── language.rs   SourceLanguage enum + parse() (Tree-sitter host smoke)
│   ├── manifest.rs   grammar-manifest qualification contract (Iteration 0 RED #1)
│   ├── router.rs     prototype inline-source detector (no filesystem access)
│   └── worker.rs     parse-only worker experiment (Outcome::NotStarted / Parsed)
├── tests/no_source.rs   no-source contract (Iteration 0 RED #3)
└── benches/no_source_bench.rs  bench harness that fails if a no-source command
                                 starts the worker (Iteration 0 RED #3)
```
Iteration 0 ships only the grammar-manifest qualification contract, the host
parse smoke, the 4-target cross-compile release matrix, and the no-source
contract. The bounded ephemeral worker, file/heredoc targets, adapters, and
policy integration land in later iterations of the L1 plan. Until then the
crate is not wired into `src/` orchestration.

---

## 3. Request Lifecycles

### 3.1 Shell wrapper — `aegis -c "<cmd>"` / `aegis --command "<cmd>"`

```
main ─▶ shell_compat::parse_invocation_mode
     ├─▶ InvocationMode::ShellCompatCommand ─▶ shell_wrapper::run_shell_wrapper
     └─▶ InvocationMode::Cli(--command) ─▶ cli_dispatch::run_cli
                                         ─▶ shell_wrapper::run_shell_wrapper
                                             ├─▶ planning::prepare_and_plan
                                             │   (builds RuntimeContext,
                                             │   assesses, resolves allowlist,
                                             │   checks snapshot applicability,
                                             │   calls decision::evaluate_policy)
                                             │   → PlanningOutcome::Planned(InterceptionPlan)
                                             └─▶ shell_flow::run_planned_shell_command(plan)
                                                 ├─▶ snapshot (if SnapshotPlan::Required)
                                                 ├─▶ ui::confirm::show_confirmation (if RequiresApproval)
                                                 ├─▶ Sandbox prepare_for_exec
                                                 ├─▶ audit append via RuntimeContext
                                                 ├─▶ optional active-channel warning
                                                 └─▶ prepared exec  OR  exit 2/3/4
```

### 3.2 Watch mode — `aegis watch`

```
main ─▶ cli_dispatch::run_cli
     ─▶ watch::run(prepared, in_ci)
         loop per stdin frame:
         ├─▶ parse InputFrame (reject if > MAX_FRAME_BYTES)
         ├─▶ planning::prepare_and_plan (transport = Watch)
         ├─▶ prompt via ui::confirm::show_*_via_tty (TTY, not stdout)
         ├─▶ snapshot + Sandbox prepare_for_spawn + audit
         ├─▶ optional OutputFrame::Warning
         ├─▶ spawn prepared child
         ├─▶ pump child stdout/stderr as OutputFrame::{Stdout,Stderr}
         └─▶ emit OutputFrame::Result { decision, exit_code }
```

### 3.3 Claude hook — `aegis hook`

```
Claude Code ─▶ aegis hook  (stdin = PreToolUse JSON)
            ─▶ install::run_hook
                ├─ for Bash-matched hook invocations: rewrite command to
                │  `aegis --command '<cmd>'`
                └─ emit modified JSON on stdout
Claude Code then spawns the rewritten command, which re-enters Aegis via §3.1.
```

---

## 4. Module Boundaries (who may call whom)

Allowed dependency directions. Arrows point from caller → callee.

```
entrypoint (main, cli_dispatch, shell_compat, cli_commands, shell_wrapper,
            shell_flow, rollback, install, policy_output,
            toggle, runtime_gate)
    │
    ├──▶ planning ──▶ decision           (pure; no I/O)
    │       │
    │       └──▶ runtime
    │              ├──▶ interceptor      (assess; no I/O)
    │              ├──▶ config           (effective config + allowlist)
    │              ├──▶ snapshot         (I/O; lazy)
    │              └──▶ audit            (append-only I/O)
    │
    └──▶ ui                              (TUI only)
```

### Forbidden edges (enforced by tests in `tests/main_architecture_slices.rs`

and `tests/main_thin_entrypoint.rs`, to be extended):

| Forbidden                                                                     | Why                                                                                         |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `decision.rs` → any I/O (`std::fs`, `std::process`, `tokio::*`)               | Policy must stay a pure function — testable in isolation, reusable across transports.       |
| `interceptor/**` → `audit/**`                                                 | Scanner knows nothing about logging.                                                        |
| `interceptor/**` → `snapshot/**`                                              | Scanner knows nothing about recovery.                                                       |
| `interceptor/**` → `ui/**`                                                    | Scanner has no UI concerns.                                                                 |
| `ui/**` → `snapshot/**` **business logic**                                    | UI may depend on `SnapshotRecord` (display struct) but must not call `snapshot`/`rollback`. |
| `ui/**` → `audit/**`                                                          | UI does not write audit entries.                                                            |
| `config/**` → `runtime/**`, `planning/**`, `ui/**`, `snapshot/**`, `audit/**` | Config may define types and validate inputs, but must not own runtime orchestration.        |
| `snapshot/**` → `ui/**`, `audit/**`, `interceptor/**`                         | Plugins are leaves under runtime.                                                           |
| Any library module → `main.rs` or `src/bin/**`                                | Binaries depend on lib, not the other way.                                                  |
| Any transport surface duplicating policy                                      | Must go through `planning::*` or route back into shell-wrapper flow.                        |

### Allowed leaks (explicitly documented):

- `ui::confirm` imports `snapshot::SnapshotRecord`, `interceptor::{RiskLevel,
patterns, scanner}`, `explanation::CommandExplanation`. These are **data
  types** for rendering, not behavior. UI must never call `.snapshot()` or
  `.rollback()`.
- `src/runtime/context.rs` is the one place that stitches scanner + allowlist + snapshot
  registry + audit logger together. Nothing else is allowed to.

---

## 5. Invariants

These are non-negotiable. A PR that breaks any of these is wrong regardless
of what problem it solves.

### 5.1 Correctness

- **I1. Policy is a pure function.** `decision::evaluate_policy` has no I/O
  and no global state. Same input → same output, always.
- **I2. `Block` is never bypassable.** A pattern at `RiskLevel::Block`
  produces `PolicyAction::Block` regardless of mode, allowlist, or CI state.
- **I3. Quick scan introduces no false negatives relative to full scan.**
  Quick scan may over-match (fine), but it must never under-match the full
  regex set. Patterns without extractable keywords force the uncovered path so
  full scan still runs.
- **I4. Transport does not loosen policy.** Watch mode and evaluation-only JSON
  must reuse the same planning/policy path as shell execution. Hook
  integrations may only rewrite into that path. A transport may be stricter in
  presentation or execution, never more permissive in policy.

### 5.2 Performance

- **I5. Scanner hot path ≤ 2 ms.** Safe-command classification (`scanner.assess`
  and anything layered on top of it) must stay within the project target on a
  modern laptop. Benchmarked by `benches/scanner_bench.rs`.
- **I6. Snapshot registry is lazy.** Safe and Warn commands must not
  materialize `SnapshotRegistry`. Verified by
  `safe_command_plan_does_not_materialize_snapshot_registry` and
  `warn_command_plan_keeps_snapshot_registry_unmaterialized`.
- **I7. Quick scan is allocation-free.** `Scanner::quick_scan` may not
  allocate on the heap.
- **I8. Regex is compiled once.** All built-in patterns compile at
  `Scanner::new`, never per-call.

### 5.3 Durability

- **I9. Audit is append-only.** `~/.aegis/audit.jsonl` and rotated segments
  are never rewritten or truncated in place. Only append or rotate.
- **I10. Audit chain is verifiable when enabled.** Under
  `[audit].integrity_mode = "ChainSha256"`, every rotation preserves the
  SHA-256 hash of the prior segment's tail. `aegis audit --verify-integrity`
  must pass on valid chained histories.
- **I11. Snapshot IDs are opaque rollback handles.** Callers must treat the
  full string as an opaque token copied from audit output. Providers may
  version or re-encode internal formats, but rollback must continue to accept
  issued IDs for as long as the referenced artifact still exists and the
  provider remains available.

### 5.4 Interface stability

- **I12. Exit codes 2, 3, 4 are frozen.** See §2.1. Adding a new reserved
  code requires a major version bump.
- **I13. Watch frame shapes are frozen.** Adding optional fields is allowed.
  Removing or renaming fields is a breaking change.
- **I14. Config fields are additive.** New fields must be `#[serde(default)]`.
  Removing a field requires a deprecation cycle.
- **I15. `#[non_exhaustive]` on `RiskLevel`.** External matches must use a
  wildcard arm. Adding a variant is non-breaking.

### 5.5 Rust/toolchain

- **I16. No `unwrap`/`expect` in production paths.** Acceptable only in
  tests and in _startup_ where panic is the correct behavior (e.g., malformed
  embedded `patterns.toml`).
- **I17. Library modules use `thiserror`, binary glue uses `anyhow`.**
- **I18. `async fn` in traits requires `#[async_trait]`** for object safety.
- **I19. No `once_cell`.** Use `std::sync::LazyLock` (stable since Rust 1.80).
- **I20. No new C-building deps.** The binary must stay portable.

---

## 6. Extension Points

Three things will likely be added often. Each has a fixed shape.

### 6.1 Add a built-in pattern

1. Edit `crates/aegis-scanner/patterns.toml`. Add an entry with a unique `id`
   of the form `CAT-NNN` (e.g. `FS-042`, `GIT-008`).
2. Choose `RiskLevel` from `{Safe, Warn, Danger, Block}`. For `Block`-level
   patterns, include a comment explaining why bypass is not acceptable.
3. Prefer a literal keyword in the regex — it lets Aho-Corasick short-circuit
   the quick scan. If no literal keyword exists, `PatternSet` will route
   through the "uncovered" path (slower, but correct).
4. Add a unit test in `crates/aegis-scanner/src/scanner/tests/` asserting one
   positive match and one negative match.
5. Run `rtk cargo bench --bench scanner_bench` if you touched a hot pattern.

### 6.2 Add a snapshot provider

1. Create `src/snapshot/<name>.rs`. Implement `SnapshotPlugin`:
   - `name()` returns a `&'static str` that will appear in audit logs.
   - `is_applicable(&Path)` must be cheap and side-effect-free.
   - `snapshot()` may spawn processes via the async handle. Return an opaque
     `String` ID.
   - `rollback(&str)` must be idempotent where possible.
2. Register the new name in `BUILTIN_SNAPSHOT_PROVIDER_NAMES` in
   `src/snapshot/mod.rs`.
3. Extend `materialize_builtin_plugin` with the new arm.
4. Add a config field `auto_snapshot_<name>: bool` and (if needed) a typed
   sub-config in `crates/aegis-config/src/model.rs`, both with `#[serde(default)]`.
5. Add an integration test in `tests/` that exercises snapshot + rollback.
6. Update `docs/config-schema.md`.

### 6.3 Add a transport

1. Do **not** duplicate policy. Use `planning::prepare_and_plan` or
   `plan_with_context` to get an `InterceptionPlan`.
2. Add a new variant to `ExecutionTransport` (this is an allowed additive
   change).
3. Implement input parsing and output emission in a new `src/<transport>.rs`.
4. Reuse `ui::confirm::show_*_via_tty` if stdout/stdin are reserved for
   protocol use.
5. Always append to audit through `RuntimeContext` audit helpers
   (`append_audit_entry` / `append_watch_audit_entry`), never by writing to
   `AuditLogger` directly from the transport.

---

## 7. File Size Budgets

Large files are a symptom, not a problem in themselves — but they correlate
strongly with blurred responsibilities. We set budgets to force the
conversation to happen.

| Scope                    | Soft limit | Hard limit | Action on breach                                        |
| ------------------------ | ---------- | ---------- | ------------------------------------------------------- |
| `src/main.rs`            | 300        | 500        | Move logic to `cli_*` / `shell_*` modules.              |
| Any entrypoint glue file | 400        | 600        | Split by subcommand.                                    |
| Any policy/engine file   | 600        | 900        | Split by concern, not by line count.                    |
| Any `mod.rs`             | 400        | 800        | Move impls into sibling files; keep `mod.rs` as façade. |
| Any single `.rs`         | 1 500      | 2 000      | Require explicit allowlist entry with rationale.        |

### Current breaches (2026-05-28) — known debt, not blockers

| File                                     | Lines | Plan                                                                 |
| ---------------------------------------- | ----- | -------------------------------------------------------------------- |
| `src/snapshot/supabase.rs`               | 1 638 | Acceptable — isolates one CLI integration, no mixed responsibilities.|
| `src/snapshot/docker.rs`                 | 1 302 | Acceptable — complete plugin impl with snapshot + rollback logic.    |
| `src/snapshot/mysql.rs`                  | 1 206 | Acceptable — mirrors postgres.rs structure.                         |
| `src/snapshot/postgres.rs`               | 1 025 | Acceptable — complete plugin impl.                                   |

The former `src/interceptor/parser/mod.rs` breach (≈1 041 lines) is resolved:
the parser moved to the `aegis-parser` crate (largest file ≈625 lines, within
budget), and `src/interceptor/parser/mod.rs` is now a thin re-export shim.

The former `src/interceptor/patterns.rs` (≈1 270) and `scanner/tests.rs`
(≈1 338) breaches are likewise resolved: both moved to the `aegis-scanner`
crate and split (largest resulting file ≈501 lines), with `patterns.rs` /
`scanner.rs` left as thin re-export shims.

The former `src/config/model/tests.rs` (≈1 464) breach is resolved: config
moved to the `aegis-config` crate and its tests are split under
`crates/aegis-config/src/model/tests/` (largest ≈512 lines); `src/config/` is a
thin re-export shim.

The former `src/ui/confirm/` module (decision + render tests, ≈1 500 lines) is
resolved: the crossterm dialog and all tests moved to the `aegis-tui` crate;
`CommandExplanation` and related explanation types moved to the new
`aegis-explanation` crate; `src/ui/confirm.rs` and `src/explanation/templates.rs`
are thin re-export shims.

Budgets are enforced by `tests/main_thin_entrypoint.rs` for `main.rs`. Extend
to other files as they are brought into compliance.

---

## 8. Public API Surface

`src/lib.rs` currently re-exports these modules:

```rust
pub mod analysis;
pub mod audit;
pub mod config;
pub mod decision;
pub mod error;
pub mod explanation;
pub mod interceptor;
pub mod planning;
pub mod runtime;
pub mod runtime_gate;
pub mod snapshot;
pub mod toggle;
pub mod ui;
pub mod watch;
```

**Rule.** Aegis is primarily a binary. The library surface exists for tests
and for future embedders. Changes to any type exported from these modules
require a corresponding ADR note. Prefer narrowing exports to broadening them.

`analysis` (added L1 Iteration 3, ADR-022) is the parent-side language-worker
client: it spawns the ephemeral `aegis --internal-language-worker` subprocess
and frames requests/responses over pipes. The worker logic and Tree-sitter
runtime live in the `aegis-language` crate; this module owns only async
orchestration and the `WorkerError → DegradationReason::WorkerFailure`
mapping. Wiring its results into an `Assessment` lands with the Iteration 1
monotonic merge and Iteration 4 source routing.

---

## 9. Glossary

| Term                           | Definition                                                                                                                        |
| ------------------------------ | --------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- | --------------------------------------------------- | --------- |
| `RiskLevel`                    | `Safe < Warn < Danger < Block`. `#[non_exhaustive]`. Ordered by severity.                                                         |
| `Assessment`                   | Scanner output: `{ risk, matched: Vec<Arc<Pattern>>, command: ParsedCommand, decision_source, highlights }`.                      |
| `Pattern`                      | `{ id, category, risk, pattern, description, safe_alt, source }`. Built-in or user.                                               |
| `PolicyInput`/`PolicyDecision` | Pure function input/output for `decision::evaluate_policy`.                                                                       |
| `PolicyAction`                 | `AutoApprove                                                                                                                      | Prompt                                               | Block`.                                             |
| `BlockReason`                  | `IntrinsicRiskBlock                                                                                                               | ProtectCiPolicy                                      | StrictPolicy`. Only set when `PolicyAction::Block`. |
| `ExecutionTransport`           | `Shell                                                                                                                            | Watch                                                | Evaluation`.                                        |
| `InterceptionPlan`             | Output of `planning::plan_with_context`. Fully resolved: execution disposition, approval requirement, snapshot plan, explanation. |
| `ExecutionDisposition`         | `Execute                                                                                                                          | RequiresApproval                                     | Block`.                                             |
| `SnapshotPlan`                 | `NotRequired                                                                                                                      | Required { applicable_plugins: Vec<&'static str> }`. |
| `PreparedPlanner`              | `Ready(Box<RuntimeContext>)                                                                                                       | SetupFailure(SetupFailurePlan)`.                     |
| `CwdState`                     | `Resolved(PathBuf)                                                                                                                | Unavailable`.                                        |
| `RuntimeContext`               | Per-invocation shared deps: scanner, allowlist, audit logger, snapshot registry (lazy), async handle.                             |
| `SnapshotPlugin`               | Trait; six built-ins: git, docker, postgres, mysql, sqlite, supabase.                                                             |
| `SnapshotRecord`               | `{ plugin: &'static str, snapshot_id: String }`.                                                                                  |
| `Decision`                     | Audit-level decision: `AutoApproved                                                                                               | Approved                                             | Denied                                              | Blocked`. |
| `AuditEntry`                   | One JSONL record written to `~/.aegis/audit.jsonl`.                                                                               |
| `CommandExplanation`           | `{ scan, policy, context, outcome }`. Deterministic, serializable.                                                                |
| `Mode`                         | Operating mode: `Protect                                                                                                          | Audit                                                | Strict`.                                            |
| `CiPolicy`                     | Behavior under CI detection: `Block                                                                                               | Allow`.                                              |
| `SnapshotPolicy`               | `None                                                                                                                             | Selective                                            | Full`.                                              |
| `AllowlistOverrideLevel`       | Ceiling on what the allowlist may auto-approve under Protect/Strict mode.                                                         |

---

_Last reviewed: 2026-07-20. When editing this file, update the review date
and note any invariants you added, removed, or changed._
