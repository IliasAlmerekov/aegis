# Aegis — Transformation Roadmap

This document describes the path from Aegis's current state to a production-grade,
architecturally strong security tool on par with the best-in-class agents in this
space (e.g. OpenAI Codex). Each phase has a clear goal, concrete deliverables, and
a definition of done. Phases are sequential: each one builds on the foundation laid
by the previous.

The north star: Aegis intercepts every shell command, classifies it with zero false
negatives, persists every human decision as a machine-readable rule, and can
optionally sandbox execution at the OS level — all in under 2 ms on the hot path.

---

## Phase 0 — Foundation Repair

**Goal:** eliminate the critical defects that undermine Aegis's core value
proposition. Nothing in Phase 1+ is safe to build until these are resolved.
These are not cleanups — they are open security and correctness bugs.

### 0.1 Async correctness in snapshot plugins

- Replace `std::thread::sleep` with `tokio::time::sleep` everywhere in retry loops
  (`docker.rs`, `postgres.rs`, `mysql.rs`).
- Change `SnapshotPlugin::is_applicable` trait signature from `fn` to
  `async fn` — or remove blocking I/O from all implementations entirely.
  Either decision must be consistent across the trait and all six plugins.
- Remove `spawn_blocking` workarounds that exist only because `is_applicable`
  wasn't async.

**Done when:** `cargo clippy` reports no `blocking_in_async_context` equivalents;
`tokio::test` with `#[timeout]` passes for all snapshot plugins.

### 0.2 Audit log is a security artifact — treat it as one

- Emit a hard error (not a `tracing::warn`) on every audit write failure,
  regardless of the `verbose` flag.
- Change the default for `AuditIntegrityMode` from `Off` to `ChainSha256`.
  SHA-256 hash chaining is Aegis's only tamper-detection mechanism; it must be
  opt-out, not opt-in.
- Add `#[must_use]` to `AuditLogger::append` and every `Result`-returning
  public function in the audit module.

**Done when:** deliberately breaking the audit file path causes a non-zero exit
with a user-visible error; a new install's default config has `integrity_mode =
"ChainSha256"`.

### 0.3 Eliminate dead code in security-critical paths

- Remove `#[allow(dead_code)]` from `src/error.rs` and `src/interceptor/parser/mod.rs`.
  Either use the flagged code or delete it. There is no middle ground for dead
  code in a hot-path security parser.
- Remove `#[allow(clippy::too_many_arguments)]` from `append_watch_audit_entry`
  by introducing a `WatchAuditContext` value type that aggregates its 11 parameters.

**Done when:** `cargo clippy -- -D dead_code` passes with no suppressions in
`src/interceptor/` and `src/audit/`.

### 0.4 Harden config loading

- Fix `detect_effective_user_from_id_command`: resolve `id` via `PATH` lookup,
  not the hardcoded `/usr/bin/id` path.
- Fix `default_snapshots_dir`: replace the `HOME` fallback of `"."` with an
  explicit error when `HOME` is unset.
- Fix `custom_pattern_cache_key`: introduce a typed `CacheKey` newtype; validate
  that pattern fields do not contain the separator characters at construction time.
- Add a config migration path: `deserialize_config_version` must emit a
  structured migration error (not a parse failure) when `config_version > 1`,
  and must document the upgrade procedure.

**Done when:** all four issues have regression tests; CI passes on a config file
with `HOME` unset.

### 0.5 Declare MSRV and add Windows to CI

- Add `rust-version = "1.80"` to `Cargo.toml` (minimum for `std::sync::LazyLock`).
- Add a `windows-latest` job to `.github/workflows/ci.yml` that runs `cargo test`
  on all `#[cfg(windows)]` paths.

**Done when:** CI has a green Windows job; a PR that uses `once_cell` fails CI.

---

## Phase 1 — Scanner Modernization

**Goal:** replace the current regex-on-raw-string scanner with a token-prefix
engine that is semantically correct, faster on the hot path, and extensible.
Inspired by codex's `execpolicy` crate.

### 1.1 Command tokenizer as the single source of truth (Done)

The scanner currently applies patterns to the raw command string. This produces
false positives (`echo "rm -rf"` triggers `rm -rf` patterns) and false negatives
(quoting tricks bypass substring matches).

Introduce a dedicated tokenizer that always runs first:

```
raw command string
    → tokenize (shlex)
    → ParsedCommand { program: &str, argv: &[&str] }
    → pattern matching on tokens
```

`ParsedCommand` becomes the canonical representation throughout the codebase.
The raw string is only used for display and audit logging.

### 1.2 Replace `HashMap<id, pattern>` with `MultiMap<program, Rule>`

Index rules by the first token of the command (the program name). This gives O(1)
lookup per command instead of scanning every pattern.

```rust
// Before: scan all N patterns for every command
patterns.iter().filter(|p| p.regex.is_match(raw_cmd))

// After: fetch only patterns relevant to this program
rules_by_program.get_vec("git")  // returns only git-* rules
```

For commands where the program cannot be determined (e.g. variable expansion),
fall back to a small set of universal patterns.

### 1.3 `PrefixRule` — token-level pattern matching

Replace free-form regex with token-prefix rules:

```rust
pub struct PrefixRule {
    pub pattern: PrefixPattern,   // ["git", "push", Alts(["--force", "-f"])]
    pub risk:    RiskLevel,
    pub justification: Option<Cow<'static, str>>,
}

pub enum PatternToken {
    Single(Cow<'static, str>),
    Alts(Vec<Cow<'static, str>>),
}
```

`Alts` lets one rule cover semantic equivalents (`--force` / `-f`) without
duplicating entries.

### 1.4 `justification` surfaces in the TUI

Every rule gains an optional human-readable explanation of _why_ it is risky:

```
⚠  git push --force

This command rewrites remote history. Collaborators with local copies
will have diverged refs and will need to force-pull or re-clone.
Consider --force-with-lease to at least detect concurrent pushes.

[A]llow  [D]eny  [Always allow]  [Always deny]
```

The `justification` field is shown in the confirmation dialog. It is set for
all built-in rules and can be added to user-defined rules in config.

### 1.5 `match_examples` / `not_match_examples` as first-class rule fields

Rules self-document and self-test:

```rust
pub struct PrefixRule {
    // ...
    pub match_examples:     &'static [&'static str],
    pub not_match_examples: &'static [&'static str],
}
```

At startup (in debug builds and tests), the scanner validates all built-in rules
against their examples. A rule that fails its own examples is a compile-time
error.

**Done when:** `cargo test` exercises all 70+ built-in rules against their
examples; the TUI shows `justification` text for all built-in `Warn` and `Danger`
rules; `cargo criterion` shows hot-path latency unchanged or improved.

---

## Phase 2 — Decision Persistence

**Goal:** when a human makes a decision about a command, that decision is
automatically persisted as a rule. The user never sees the same prompt twice for
the same command pattern. Inspired by codex's `amend.rs`.

### 2.1 "Always allow" writes a rule to config

When the user chooses "Always allow" in the TUI, Aegis:

1. Tokenizes the command into its prefix (program + meaningful flags, stripping
   variable arguments like file paths).
2. Calls `amend::append_allow_rule(config_path, &prefix)` which appends a new
   rule to `~/.aegis/aegis.toml` (or the active project config).
3. Invalidates the scanner cache so the new rule takes effect immediately.

The appended rule is human-readable TOML:

```toml
[[allow]]
pattern = ["git", "push", "--force-with-lease"]
reason  = "Approved by user on 2025-05-22"
```

### 2.2 "Always deny" writes a block rule

Same mechanism for the "Always deny" choice:

```toml
[[block]]
pattern = ["rm", "-rf", "/"]
reason  = "Blocked by user on 2025-05-22"
```

### 2.3 Rule deduplication on write

`amend` checks whether an equivalent rule already exists before appending.
A duplicate rule is silently skipped; a conflicting rule (same pattern, different
decision) produces a warning with the existing rule's location.

### 2.4 Allowlist merges into the unified rule system

The current `allowlist` config field (`allowed_commands`, `allowed_patterns`) is
deprecated and replaced by the `[[allow]]` rule table. A migration function reads
the old format on first load and writes the equivalent `[[allow]]` entries. The
old field is accepted but emits a deprecation warning.

**Done when:** after one interactive session, `~/.aegis/aegis.toml` contains the
user's allow/block decisions as typed rules; those decisions are respected on the
next run without re-prompting; the old allowlist format still loads with a warning.

---

## Phase 3 — Module Architecture

**Goal:** enforce strict module boundaries through the type system and directory
structure. Eliminate the monolithic files that have grown beyond 800 lines. Update
all documentation to match the actual module layout.

### 3.1 File size budget — hard limit 800 LoC

The following files exceed the 800-line limit and must be split:

| File             | Current size | Target                                                  |
| ---------------- | ------------ | ------------------------------------------------------- |
| `runtime.rs`     | ~1100 lines  | `runtime/context.rs` + `runtime/user.rs`                |
| `decision.rs`    | ~900 lines   | `decision/engine.rs` + `decision/types.rs`              |
| `explanation.rs` | ~800 lines   | `explanation/formatter.rs` + `explanation/templates.rs` |
| `install.rs`     | ~1600 lines  | `install/` submodule (3–4 files)                        |
| `watch.rs`       | ~1100 lines  | `watch/` submodule (loop + protocol)                    |

Rule: when extracting a file, move its tests and type docs into the new file.
Never leave tests behind in the old file for code that moved.

### 3.2 Update `ARCHITECTURE.md` to match reality

`ARCHITECTURE.md` describes seven layers. The actual code has grown to include
`planning/`, `toggle.rs`, `runtime_gate.rs`, `shell_flow.rs`, and five additional
snapshot backends. Update the document to be authoritative again:

- Add `planning/` to the policy engine layer.
- Add `toggle.rs` and `runtime_gate.rs` to the entrypoint layer.
- Add all six snapshot backends to the snapshot layer description.
- Document the `watch` mode NDJSON protocol as a first-class protocol (currently
  only mentioned in passing).

### 3.3 `AuditEntry` — typed variant instead of flat struct

Replace the 18-field flat struct with a typed enum:

```rust
pub enum AuditEntry {
    Decision(DecisionEntry),   // always-present fields + decision outcome
    Watch(WatchEntry),         // watch-mode source, cwd, exit code
}

pub struct DecisionEntry {
    pub timestamp: DateTime<Utc>,
    pub command:   String,
    pub risk:      RiskLevel,
    pub decision:  Decision,
    // ... 4-5 always-present fields, no Option<T>
}
```

This makes it impossible to construct a watch entry without a `cwd`, and
impossible to construct a decision entry without a `risk` level.

### 3.4 `AegisConfig` — remove type alias ambiguity

Remove `pub type Config = AegisConfig`. All code uses `AegisConfig` directly.
Generate a JSON schema from the type (`just write-config-schema`) so editors can
validate `aegis.toml` files with autocompletion.

**Done when:** no file in `src/` exceeds 800 lines; `ARCHITECTURE.md` matches the
actual module tree with no undocumented modules; `cargo doc --no-deps` produces
zero `missing_docs` warnings.

---

## Phase 4 — Multi-Crate Workspace

**Goal:** split the single-crate monolith into focused library crates with
enforced dependency boundaries. This is the structural prerequisite for the policy
DSL in Phase 5 and the sandboxing layer in Phase 6.

### 4.1 Crate extraction order

Extract in this order — each crate must compile and pass its tests before the
next extraction begins:

```
aegis/                          (workspace root)
  crates/
    aegis-types/                RiskLevel, Decision, Pattern, Assessment — zero deps
    aegis-parser/               command tokenizer, PrefixPattern matching
    aegis-scanner/              Scanner, PatternSet — depends on aegis-types, aegis-parser
    aegis-policy/               PolicyEngine, PrefixRule, amend — depends on aegis-scanner
    aegis-audit/                AuditLogger, AuditEntry — depends on aegis-types
    aegis-snapshot/             SnapshotPlugin trait + 6 backends — depends on aegis-types
    aegis-tui/                  crossterm confirmation dialog — depends on aegis-types
    aegis-config/               AegisConfig, loader, schema — depends on aegis-types
  src/                          binary — thin wiring, depends on all crates above
```

Each `crates/X/Cargo.toml` must not depend on `aegis-binary` or any other
application crate. Dependency arrows flow inward toward `aegis-types`.

### 4.2 Dependency rule enforcement via `cargo deny`

Extend `deny.toml` to ban cycles and enforce the dependency DAG:

```toml
[[bans.deny]]
# aegis-parser must not depend on aegis-audit
name = "aegis-audit"
wrappers = ["aegis-parser"]
```

CI fails if any crate violates the dependency boundary.

### 4.3 `aegis-parser` becomes a fuzz target

With the parser in its own crate, `fuzz/` can target it directly. Increase CI
fuzz iterations from 2000 to 100 000. Add the corpus from production runs
(sanitized command strings) to the fuzz corpus directory.

**Done when:** `cargo build --workspace` succeeds; `cargo test --workspace` passes;
a PR that adds a dependency from `aegis-parser` to `aegis-audit` fails CI via
`cargo deny`.

---

## Phase 5 — Policy DSL

**Goal:** replace TOML pattern tables with a typed, programmable policy language.
Users can express rules that require conditional logic, environment context, or
programmatic construction — without modifying Rust source. Inspired by codex's
Starlark-based `execpolicy` parser.

### 5.1 Evaluate DSL options

Before committing to an implementation, benchmark three approaches against Aegis's
2 ms hot-path constraint:

| Option          | Expressiveness | Binary size impact | Startup cost |
| --------------- | -------------- | ------------------ | ------------ |
| Starlark (rhaï) | High           | +3–5 MB            | ~1 ms warmup |
| Lua (mlua)      | Medium         | +1–2 MB            | < 0.5 ms     |
| Typed TOML DSL  | Low–Medium     | Zero               | Zero         |

Recommended starting point: **typed TOML DSL** (Phase 5.1), with Starlark/Lua as
an opt-in power-user feature (Phase 5.2). The typed DSL covers 95% of real use
cases without embedding an interpreter.

### 5.2 Typed TOML policy DSL

Extend `aegis.toml` with a richer rule type:

```toml
[[rules]]
pattern     = ["git", "push", ["--force", "-f"]]
decision    = "prompt"
justification = "Force-push rewrites remote history."
match_examples     = ["git push --force origin main"]
not_match_examples = ["git push origin main"]

[[rules]]
pattern  = ["rm", "-rf", "/"]
decision = "block"

[[rules]]
pattern  = ["docker", "run"]
decision = "prompt"
when     = { env = "CI", value = "true", then = "allow" }
```

The `when` clause adds environment-conditional decisions. The rule is validated
at load time (not at match time) — invalid rules are a startup error.

### 5.3 Starlark policy DSL (power-user tier)

For users who need programmatic rules, offer an opt-in Starlark policy file
(`~/.aegis/policy.star`):

```python
prefix_rule(
    pattern = ["kubectl", "delete"],
    decision = "prompt",
    justification = "Deleting Kubernetes resources is irreversible.",
    match_examples = [["kubectl", "delete", "pod", "mypod"]],
)

def on_command(cmd):
    if cmd[0] == "git" and "--force" in cmd:
        return "prompt"
    return "allow"
```

Starlark is evaluated at startup and the resulting rule set is compiled to the
same `MultiMap<program, PrefixRule>` used by the typed DSL. There is no runtime
Starlark evaluation on the hot path.

**Done when:** a user can write `~/.aegis/aegis.toml` with `[[rules]]` entries
using `Alts`, `when`, `justification`, and `match_examples`; invalid rules produce
a human-readable error with line numbers; the hot path shows no regression on
`cargo criterion`.

---

## Phase 6 — Sandboxing Layer

**Goal:** move Aegis from "block risky decisions" to "restrict capabilities at the
OS level." An approved command runs, but within a sandbox that limits what it can
actually do. This is the architecture used by codex for all tool executions.

### 6.1 Linux — bubblewrap + Landlock

- `bwrap` (bubblewrap): namespace-based sandbox. Approved commands run in a
  new mount namespace with a read-only view of the filesystem except for
  explicitly allowed write paths.
- `landlock`: Linux Security Module for fine-grained filesystem access control.
  Applied in addition to bwrap for defense in depth.

```toml
[sandbox]
enabled = true
allow_write = [".", "/tmp"]
allow_network = false
```

### 6.2 macOS — Seatbelt (`sandbox-exec`)

Apply a `.sbpl` sandbox profile via `/usr/bin/sandbox-exec` before exec'ing the
approved command. Profile templates live in `crates/aegis-sandbox/profiles/`.

### 6.3 Windows — Job Objects

Use Windows Job Objects to restrict the child process's ability to create new
processes, modify the filesystem outside allowed paths, or open network sockets.

### 6.4 Sandbox bypass is an audit event

If the sandbox cannot be applied (kernel version too old, missing capabilities,
unsupported platform), Aegis logs a `SandboxUnavailable` audit entry and proceeds
without sandboxing. The user can configure `sandbox.required = true` to turn
unavailability into a hard block.

**Done when:** `cargo test --workspace` passes with sandbox enabled on
`ubuntu-latest` and `macos-latest`; a command that attempts to write outside the
allowed paths is killed by the sandbox; the audit log records the sandbox profile
applied for every executed command.

---

## Phase 7 — Release Readiness

**Goal:** complete the launch checklist in `docs/release-readiness.md` and ship
a 1.0 release.

- [ ] README and docs accurately describe all features through Phase 4.
- [ ] Convenience installer documented and tested (`curl | sh` or package manager).
- [ ] Release workflow exercised on a real tag; artifacts include checksum sidecars.
- [ ] Supported platforms (Linux x86_64/aarch64, macOS arm64/x86_64, Windows x86_64)
      stated clearly with notes on sandboxing availability per platform.
- [ ] CI includes ARM cross-compilation jobs (`aarch64-unknown-linux-musl`).
- [ ] Threat model and known limitations visible on the project README.
- [ ] Snapshot rollback integration tests run in CI against real Docker / SQLite daemons.
- [ ] Fuzz corpus in CI at ≥ 100 000 iterations per target.
- [ ] `cargo audit` and `cargo deny check` both pass with zero findings.
- [ ] CHANGELOG.md updated for every release via `git-cliff` or equivalent.

---

## Summary

| Phase | Name                  | Key deliverable                                    |
| ----- | --------------------- | -------------------------------------------------- |
| 0     | Foundation Repair     | No silent failures; async correct; CI on Windows   |
| 1     | Scanner Modernization | Token-prefix matching; `justification` in TUI      |
| 2     | Decision Persistence  | "Always allow/block" writes rules to config        |
| 3     | Module Architecture   | No file > 800 lines; typed `AuditEntry`; live docs |
| 4     | Multi-Crate Workspace | 8 focused crates; enforced dependency DAG          |
| 5     | Policy DSL            | Typed TOML rules + optional Starlark               |
| 6     | Sandboxing Layer      | bwrap/Landlock/Seatbelt on approved commands       |
| 7     | Release Readiness     | 1.0 ships                                          |
