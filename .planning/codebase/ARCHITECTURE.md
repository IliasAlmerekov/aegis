# Architecture

**Analysis Date:** 2026-03-27

## Pattern Overview

**Overall:** Layered pipeline with separation of concerns (risk assessment → decision logic → audit logging → execution)

**Key Characteristics:**
- **Single-responsibility modules**: Each layer handles a discrete concern (parsing, scanning, decision-making, snapshot/rollback, audit)
- **Fail-closed defaults**: When subsystems fail (scanner, config, snapshots), the system requires confirmation rather than auto-approving
- **Lazy-cached patterns**: Built-in patterns compiled once at startup; custom patterns cached by content hash
- **Best-effort snapshots**: Snapshot failures don't block command execution; they're logged as warnings only
- **Immutable audit trail**: Append-only JSONL with optional rotation; never modifies existing records

## Layers

**Presentation (CLI/UI):**
- Purpose: Parse arguments, display confirmation dialogs, format output
- Location: `src/main.rs`, `src/ui/confirm.rs`
- Contains: Command-line parsing (clap), TUI rendering (crossterm), exit code contracts
- Depends on: All modules below
- Used by: Shell environment (user-facing entry point)

**Decision/Control:**
- Purpose: Orchestrate the full command lifecycle (assess → decide → audit → execute)
- Location: `src/main.rs` functions (`run_shell_wrapper`, `decide_command`)
- Contains: CI detection, allowlist matching, decision logic (CI policy, snapshot coordination)
- Depends on: interceptor, config, snapshot, audit, runtime
- Used by: main() entry point

**Runtime Context:**
- Purpose: Build and cache all dependencies once per CLI invocation
- Location: `src/runtime.rs`
- Contains: RuntimeContext struct holding scanner, snapshot registry, audit logger, config
- Depends on: config, interceptor, snapshot, audit
- Used by: main.rs for all downstream operations

**Interceptor (Risk Assessment):**
- Purpose: Parse shell commands and assess risk level via pattern matching
- Location: `src/interceptor/` (mod.rs, scanner.rs, parser.rs, patterns.rs)
- Contains:
  - `Parser`: Tokenizes raw command, extracts inline scripts, builds ParsedCommand
  - `Scanner`: Two-pass scanning (Aho-Corasick quick-scan → regex full-scan)
  - `PatternSet`: Loads built-in + custom patterns, validates uniqueness
  - `RiskLevel`: Enum (Safe < Warn < Danger < Block)
- Depends on: config (for custom patterns), error
- Used by: runtime, main.rs decision logic

**Snapshot/Rollback:**
- Purpose: Create and manage state snapshots (Git stash, Docker container state)
- Location: `src/snapshot/` (mod.rs, git.rs, docker.rs)
- Contains:
  - `SnapshotPlugin` trait (async): is_applicable, snapshot, rollback
  - `SnapshotRegistry`: Coordinates all plugins, runs snapshots in parallel
  - `GitPlugin`: Uses `git stash` for working tree state
  - `DockerPlugin`: Container state management
- Depends on: tokio (for async subprocess), error
- Used by: runtime decision logic to create pre-command snapshots

**Audit (Logging):**
- Purpose: Append-only audit trail of all commands, decisions, matches
- Location: `src/audit/` (mod.rs, logger.rs)
- Contains:
  - `AuditEntry`: Command, risk, matched patterns, decision, snapshots, timestamp
  - `AuditLogger`: Append to JSONL, query by risk/count, optional rotation
  - `AuditRotationPolicy`: Compression and retention settings
- Depends on: error, time, flate2 (optional compression)
- Used by: runtime to log every command assessment

**Config (Policy):**
- Purpose: Load and validate user preferences from TOML files
- Location: `src/config/` (mod.rs, model.rs, allowlist.rs)
- Contains:
  - `AegisConfig`: mode, ci_policy, allowlist, custom_patterns, snapshot settings, audit rotation
  - `Allowlist`: Regex-based command allowlist with caching
  - `UserPattern`: User-defined custom patterns
  - Config search order: project `.aegis.toml` → global `~/.config/aegis/config.toml` → defaults
- Depends on: error, serde, toml
- Used by: runtime to initialize all subsystems

**Error Handling:**
- Purpose: Typed error representation across all modules
- Location: `src/error.rs`
- Contains: `AegisError` enum via thiserror (Parse, Snapshot, RollbackConflict, Config, Io)
- Depends on: thiserror, std::io
- Used by: All modules for error propagation

## Data Flow

**Shell Wrapper Mode (normal interactive use):**

1. `main.rs` calls `run_shell_wrapper(cmd, verbose)`
2. `RuntimeContext::load(verbose)` → builds config, scanner, snapshot registry, audit logger (all once)
3. `context.assess(cmd)` → runs interceptor pipeline:
   - `Parser::parse(cmd)` → extracts executable, args, inline scripts
   - `Scanner::assess(cmd)` → Aho-Corasick quick-scan (returns false/true), optional full regex scan
   - Returns `Assessment { risk, matched, command }`
4. `context.allowlist_match(cmd)` → checks if command matches allowlist rules (exits early if matched and not Block)
5. `decide_command()` applies business logic:
   - If CI environment with CiPolicy::Block and non-safe: return Blocked
   - If allowlisted and not Block: return AutoApproved with snapshots if Danger
   - If not allowlisted: show TUI confirmation dialog, get user input
6. `context.create_snapshots(cwd, cmd, verbose)` → async snapshot creation (best-effort, failures logged only)
7. `context.append_audit_entry()` → appends one AuditEntry to JSONL
8. `exec_command(cmd)` → uses `CommandExt::exec` (Unix) or Command::status (non-Unix) to replace process
9. Return exit code (0 on success, 2 Denied, 3 Blocked, 4 Internal Error, or child's exit code)

**Audit Query Mode:**

1. `main.rs` calls `AuditLogger::query(last, risk_filter)` → reads JSONL, filters, returns Vec<AuditEntry>
2. Format output as text, JSON, or NDJSON
3. Print and exit 0

**Config Init Mode:**

1. `main.rs` calls `Config::init_in(cwd)` → writes `.aegis.toml` with template
2. Print path and exit 0

**State Management:**

- **Config state**: Loaded once per CLI invocation via `RuntimeContext::load()`. Immutable after that.
- **Scanner state**: Built-in patterns cached globally in `LazyLock<BUILTIN_SCANNER>`. Custom patterns cached by content hash in `LazyLock<Mutex<HashMap>>`.
- **Audit state**: Append-only; never modified. Reads/writes are synchronized via file locking (OS-level).
- **Snapshot state**: Created on-demand per dangerous command; identified by opaque string (e.g., git stash hash).

## Key Abstractions

**RiskLevel (Enum):**
- Purpose: Ordered severity classification (Safe < Warn < Danger < Block)
- Examples: `RiskLevel::Safe`, `RiskLevel::Danger`
- Pattern: `#[non_exhaustive]` for forward compatibility

**Pattern:**
- Purpose: Represents a single security pattern (builtin or user-defined)
- Location: `src/interceptor/patterns.rs`
- Fields: id, category, risk, pattern (regex), description, safe_alt, source
- Pattern: Uses `Cow<'static, str>` for zero-copy built-in + owned user-defined strings

**Assessment:**
- Purpose: Result of assessing a single command through the scanner pipeline
- Location: `src/interceptor/scanner.rs`
- Fields: risk (highest matched level), matched (Vec<MatchResult>), command (ParsedCommand)
- Pattern: Immutable after construction; passed through decision logic unchanged

**ParsedCommand:**
- Purpose: Parsed representation of a shell command
- Location: `src/interceptor/parser.rs`
- Fields: executable, args, inline_scripts, raw (original)
- Pattern: Extracted once during assess phase; used for pattern matching and audit logging

**AuditEntry:**
- Purpose: Single immutable record of a command + decision
- Location: `src/audit/logger.rs`
- Fields: sequence, timestamp, command, risk, matched_patterns, decision, snapshots, allowlist_pattern
- Pattern: Serialized as one line of JSONL; never modified after creation

**SnapshotPlugin (Trait):**
- Purpose: Plugin interface for state snapshots
- Location: `src/snapshot/mod.rs`
- Methods: name(), is_applicable(), snapshot() (async), rollback() (async)
- Pattern: `#[async_trait]` for vtable-compatible async functions

**Decision (Enum):**
- Purpose: Final user/system choice on whether to run the command
- Location: `src/audit/logger.rs`
- Variants: Safe (auto-approved), Warn (approved), Danger (approved), Denied (user said no), Blocked (hard-blocked), AutoApproved (allowlist or CI)
- Pattern: Deterministic given Assessment + Config + CI environment

## Entry Points

**CLI (Shell Wrapper Mode):**
- Location: `src/main.rs` main() → run_shell_wrapper()
- Triggers: User runs `aegis -c "command"` or as `$SHELL` proxy
- Responsibilities: Parse args, load runtime, assess, decide, audit, execute

**CLI (Audit Subcommand):**
- Location: `src/main.rs` main() → Commands::Audit handler
- Triggers: User runs `aegis audit [--last N] [--risk {safe|warn|danger|block}] [--format {text|json|ndjson}]`
- Responsibilities: Query audit log, format, print

**CLI (Config Subcommand):**
- Location: `src/main.rs` main() → Commands::Config handler
- Triggers: User runs `aegis config init` or `aegis config show`
- Responsibilities: Initialize template or print active config

**Library API:**
- Location: `src/lib.rs`
- Exports: `pub fn assess(cmd)`, `pub fn assess_with_custom_patterns(cmd, patterns)`, `pub fn scanner_for(patterns)`
- Used by: External tools, tests, integration harnesses

## Error Handling

**Strategy:** Typed errors via thiserror; fail-closed defaults

**Patterns:**

- **Config load failure** → falls back to `Config::default()` (safe defaults: Protect mode, Block CI policy)
- **Scanner init failure** → RuntimeContext.assess() returns `RiskLevel::Warn` (requires confirmation for every command)
- **Snapshot creation failure** → logged as warning; empty Vec returned; command still proceeds
- **Audit append failure** → logged as warning (if verbose); does not affect command execution
- **Pattern compilation error** → panics at startup (programming error; fail-fast)
- **Invalid regex in user pattern** → scanner gracefully skips that pattern (logged, not fatal)
- **Git stash pop conflict** → detailed error with recovery commands; stash preserved

## Cross-Cutting Concerns

**Logging:** Via `tracing` crate. Info/warn/error events used for:
- Snapshot operations (git stash success, docker state saved)
- Config loading (missing files, defaults applied)
- Pattern matching details (when verbose mode enabled)
- Audit rotation (file size thresholds, compression)

**Validation:**
- Config TOML parsing uses `serde` with `#[serde(deny_unknown_fields)]` for strictness
- Pattern ID uniqueness enforced at PatternSet construction
- Custom patterns validated for duplicate IDs before being added to scanner

**Authentication:** None built-in. Audit log access depends on filesystem permissions (~/.aegis/audit.jsonl).

**CI Detection:**
- Reads environment variables (AEGIS_CI, GITHUB_ACTIONS, GITLAB_CI, CIRCLECI, BUILDKITE, TRAVIS, JENKINS_URL, TF_BUILD)
- Explicit AEGIS_CI=1 override takes priority
- Applied per command in decide_command() to enforce CiPolicy::Block (no interactive TTY available)

---

*Architecture analysis: 2026-03-27*
