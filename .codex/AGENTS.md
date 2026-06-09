# AGENTS.md — Aegis Codex Instructions

---

## PROJECT CONTEXT

- **Crate**: single-package Rust crate `aegis` at repo root — not a Cargo workspace
- **Edition**: 2024 · MSRV: latest stable
- **Core mechanism**: `$SHELL` proxy — intercepts shell commands, classifies risk with a
  two-pass scanner (Aho-Corasick fast path + regex verification), prompts on `Warn`/`Danger`,
  hard-blocks `Block`, takes pre-execution snapshots for dangerous commands
- **Async runtime**: `tokio 1` (`process`, `fs`, `rt` features) — scanner/parser hot path stays **synchronous**
- **Key modules**:
  - `src/interceptor/` — parser, scanner, patterns
  - `src/config/` — layered TOML config, allowlist
  - `src/snapshot/` — Git/Docker snapshot plugins
  - `src/ui/confirm.rs` — crossterm confirmation dialog
  - `src/audit/logger.rs` — append-only JSONL audit log
  - `src/main.rs` — thin CLI entry point only
- **Test runner**: `rtk cargo test`
- **All shell commands**: must go through `rtk` — never bare `cargo`, `git`, `rg`, `sed`, etc.

---

## IMPLEMENT PIPELINE

Entry point for all feature and bug work: `/implement <task description>`.

```
iteration = 0
feedback  = <task description>

LOOP (max 3 iterations):
  iteration++

  1. red-tester   — writes failing tests          (skill: testing-patterns)
  2. green-tester — implements code to pass tests  (skill: rust-systems)
  3. reviewer     — reviews code                   (skill: rust-refactor-helper)

  If reviewer → APPROVED:
    done

  If reviewer → CHANGES_REQUESTED and iteration < 3:
    feedback = original task + reviewer issues
    repeat loop

  If iteration == 3 and not APPROVED:
    lead agent takes over directly:
    - read all changed files and reviewer's CHANGES_REQUESTED output
    - fix each issue following the same constraints as green-tester
    - run `rtk cargo test` to confirm green
```

### Subagents

| Agent        | File                              | Purpose                         |
|--------------|-----------------------------------|---------------------------------|
| red-tester   | `.codex/agents/red-tester.toml`   | Write failing (red) tests       |
| green-tester | `.codex/agents/green-tester.toml` | Implement code to pass tests    |
| reviewer     | `.codex/agents/reviewer.toml`     | Review architecture and quality |

---

## CONVENTIONS

### Error handling

- Library code (`interceptor/`, `snapshot/`, `audit/`, `config/`): typed errors via `thiserror` / `AegisError`
- `main.rs` and CLI glue: `anyhow` for propagation
- No `.unwrap()` / `.expect()` in production paths — use `?` or handle explicitly
- `.expect()` acceptable only in tests and startup initialization where a panic is the explicit contract

### Dependencies (approved)

| Purpose                 | Crate                            |
|-------------------------|----------------------------------|
| CLI                     | `clap 4.5` with derive API       |
| Terminal UI             | `crossterm 0.28`                 |
| Fast multi-pattern scan | `aho-corasick 1.1`               |
| Full regex scan         | `regex 1.11`                     |
| Config                  | `serde` + `toml 0.8`             |
| Error types (lib)       | `thiserror 1`                    |
| Error propagation (bin) | `anyhow 1`                       |
| Async subprocess        | `tokio` (features: process, fs)  |
| Async trait methods     | `async-trait 0.1`                |
| Structured logging      | `tracing` + `tracing-subscriber` |
| Benchmarks              | `criterion 0.5`                  |

**Prohibited**: `once_cell` — use `std::sync::LazyLock` (stable since Rust 1.80).
Do not add new dependencies without explicit human sign-off.

### Naming

- Types, traits, enums: `PascalCase`
- Functions, methods, variables, modules: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
- Pattern IDs: uppercase string literals — `"FS-001"`, `"GIT-003"`

### Performance

- Hot path (safe commands) must stay under 2ms
- Aho-Corasick first pass only — no regex in the quick scan
- Regex compiled once via `LazyLock<Regex>`, never inline
- No allocations or string clones in scanner hot path

---

## GLOBAL CONSTRAINTS

- All shell commands via `rtk`.
- No `unsafe {}` — flag and escalate.
- No `.unwrap()` / `.expect()` in non-test production paths.
- Never modify `Cargo.toml`, `Cargo.lock`, `deny.toml`, or CI workflow files without explicit human sign-off.
- Keep `src/main.rs` thin — orchestration only, no business logic.
- Keep `src/interceptor/` synchronous — no async in parser/scanner hot path.
- Preserve fail-closed behavior: errors in classify/policy path must never auto-approve.
- `Block`-level commands must never be silently bypassed by any path.
- Audit log is append-only — never overwrite `~/.aegis/audit.jsonl`.
- Do not suggest bypass paths for Aegis blocks; explain the risk and suggest safer alternatives.
- Do not write `async fn` in a trait without `#[async_trait]`.
- Do not use `&'static str` for user-provided config values — use `Cow<'static, str>` or `String`.

---

## SENSITIVE FILES

Every change to these files requires extra care — fail-closed behavior must be preserved:

```
src/main.rs
crates/aegis-parser/
src/interceptor/parser/
crates/aegis-scanner/
src/interceptor/scanner.rs
src/interceptor/patterns.rs
src/ui/confirm.rs
src/config/model.rs
src/config/allowlist.rs
src/snapshot/mod.rs
src/snapshot/git.rs
src/snapshot/docker.rs
src/audit/logger.rs
```

---

## VERIFICATION

Run after any change:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
rtk cargo audit
rtk cargo deny check
```

Benchmark-sensitive changes in `scanner.rs` or `parser.rs`:

```bash
rtk cargo bench --bench scanner_bench
```
