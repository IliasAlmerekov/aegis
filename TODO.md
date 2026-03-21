# Aegis — TODO

> Each phase blocks the next one. Do not move to P2 until the P1 gate condition is met.

---

## Progress

| Phase | Name                              | Status         |
| ----- | --------------------------------- | -------------- |
| P1    | Foundation                        | ⬜ not started |
| P2    | Command Parser                    | ⬜ blocked     |
| P3    | Pattern Engine + Risk Classifier  | ⬜ blocked     |
| P4    | Snapshot Engine + TUI             | ⬜ blocked     |
| P5    | Config System + Shell Integration | ⬜ blocked     |
| P6    | Polish and Public Release         | ⬜ blocked     |

---

## P1 — Foundation

> Repository, toolchain, CI, empty binary
> **Timeline:** 3 days

**🔒 Gate condition (required before P2):**
Binary compiles on macOS and Linux. CI is green. Version is printed by `aegis --version`.

---

### T1.1 — Repository and Cargo initialization

- [ ] Create GitHub repo: `aegis-dev/aegis` (public, MIT license)
- [ ] `cargo init --name aegis` — initialize the project
- [ ] Configure `.gitignore` (`target/`, `.env`, `*.log`)
- [ ] Add `LICENSE` (MIT) and `README.md` with a one-line description
- [ ] Create `CONTRIBUTING.md` and `CODE_OF_CONDUCT.md` as stubs
- [ ] First commit: `chore: init repository`

### T1.2 — Cargo.toml — dependencies and build profiles

- [ ] Add `clap 4.5` with `features = ["derive", "env"]`
- [ ] Add `thiserror 1` and `anyhow 1`
- [ ] Add `serde 1` with `features = ["derive"]` + `toml 0.8` + `serde_json 1`
- [ ] Add `tracing 0.1` + `tracing-subscriber 0.3` with `features = ["fmt", "env-filter"]`
- [ ] Add `async-trait 0.1` (required for `dyn SnapshotPlugin` with async methods)
- [ ] Add `tokio = { version = "1", features = ["process", "fs", "rt"] }`
- [ ] Add `crossterm 0.28`
- [ ] Add `regex 1.11` and `aho-corasick 1.1`
- [ ] Configure `[profile.release]`: `opt-level = 3`, `lto = "thin"`, `strip = "symbols"`, `codegen-units = 1`
- [ ] Add `[dev-dependencies]`: `criterion 0.5`, `tempfile 3`
- [ ] Run `cargo check` — confirm everything compiles

### T1.3 — Module structure — empty files

- [ ] Create `src/error.rs` — empty `AegisError` enum with `#[derive(thiserror::Error, Debug)]`
- [ ] Create `src/interceptor/mod.rs`, `scanner.rs`, `parser.rs`, `patterns.rs`
- [ ] Create `src/snapshot/mod.rs`, `git.rs`, `docker.rs`
- [ ] Create `src/ui/confirm.rs`
- [ ] Create `src/audit/logger.rs`
- [ ] Create `src/config/model.rs`
- [ ] Declare all modules in `main.rs` via `mod`
- [ ] Run `cargo check` — all modules visible to the compiler

### T1.4 — Basic CLI entry point

- [ ] Implement `Cli` struct in `main.rs` using clap derive API
- [ ] Add subcommands: `watch`, `audit`, `config` (empty stubs for now)
- [ ] Add flag `-c` / `--command` for shell wrapper mode
- [ ] Add `--version` flag (auto-populated from `Cargo.toml`)
- [ ] Add `--verbose` / `-v` flag for debug output
- [ ] Verify: `cargo run -- --version` prints `aegis 0.1.0`

### T1.5 — GitHub Actions CI

- [ ] Create `.github/workflows/ci.yml`
- [ ] Step: `cargo fmt --check`
- [ ] Step: `cargo clippy -- -D warnings`
- [ ] Step: `cargo test`
- [ ] Step: `cargo build --release` for `ubuntu-latest` and `macos-latest`
- [ ] Step: `cargo audit` (install `cargo-audit` first)
- [ ] Step: `cargo deny check` (create `deny.toml` with license and advisory rules)
- [ ] Verify CI is green on first push

---

## P2 — Command Parser

> Tokenizer for commands: simple, heredoc, and inline scripts
> **Timeline:** 4 days

**🔒 Gate condition (required before P3):**
`parse(cmd)` correctly extracts the executable and arguments from 50 test cases including heredoc and `bash -c "..."`.

---

### T2.1 — Basic tokenization

- [ ] Implement `split_tokens(cmd: &str) -> Vec<String>`
- [ ] Handle single quotes: `'rm -rf /'` as one token
- [ ] Handle double quotes: `"rm -rf /"` as one token
- [ ] Handle backslash escaping: `rm\ -rf\ /`
- [ ] Handle semicolons and `&&` as command separators
- [ ] Write 15 unit tests covering edge cases
- [ ] Run `cargo test interceptor::parser` — all green

### T2.2 — Unwrap nested `bash -c` commands

- [ ] Detect pattern: `bash -c '...'`, `sh -c '...'`
- [ ] Recursively extract the nested command string
- [ ] Handle: `bash -c "cmd1 && cmd2"` → `["cmd1", "cmd2"]`
- [ ] Handle: `bash -c $'escaped\nnewline'`
- [ ] Handle: `env VAR=val bash -c '...'` (env prefix before bash)
- [ ] Write 10 test cases for nested commands

### T2.3 — Heredoc and inline script scanning

- [ ] Detect heredoc syntax: `cmd <<EOF ... EOF`
- [ ] Extract heredoc body as a separate string for scanning
- [ ] Handle nowdoc: `<<'EOF'` (no variable substitution)
- [ ] Detect and extract inline Python: `python -c "..."`
- [ ] Detect and extract inline Node.js: `node -e "..."`
- [ ] Detect and extract inline Ruby: `ruby -e "..."`
- [ ] Write 8 test cases for heredoc and inline scripts

### T2.4 — `ParsedCommand` struct and public API

- [ ] Define `struct ParsedCommand { executable, args, inline_scripts, raw }`
- [ ] Implement `Parser::parse(cmd: &str) -> ParsedCommand`
- [ ] Implement `Display` for `ParsedCommand` (used in audit log output)
- [ ] Final performance test: parse all 50 cases in under 1ms total
- [ ] Run `cargo test` — all tests green

---

## P3 — Pattern Engine + Risk Classifier

> Aho-Corasick + Regex, 50+ patterns, RiskLevel enum
> **Timeline:** 5 days

**🔒 Gate condition (required before P4):**
`Scanner::assess(cmd)` returns the correct `RiskLevel` for all 70 test cases. p99 latency < 3ms.

---

### T3.1 — `RiskLevel` enum and `AegisError`

- [ ] Define `enum RiskLevel { Safe, Warn, Danger, Block }` with `#[non_exhaustive]`
- [ ] Implement `PartialOrd` for `RiskLevel` (`Safe < Warn < Danger < Block`)
- [ ] Define `AegisError` via `thiserror` with variants: `Parse`, `Snapshot`, `Config`, `Io`
- [ ] Implement human-readable `Display` messages for each `AegisError` variant
- [ ] Write 3 unit tests verifying `PartialOrd` ordering

### T3.2 — `Pattern` struct and TOML loading

- [ ] Define `struct Pattern` with fields: `id`, `category`, `risk`, `pattern`, `description`, `safe_alt`
- [ ] Use `Cow<'static, str>` for all string fields (supports both built-in and user-defined patterns)
- [ ] Define `enum Category { Filesystem, Git, Database, Cloud, Docker, Process, Package }`
- [ ] Implement `#[derive(Deserialize)]` on `Pattern` and `Category` for TOML loading
- [ ] Create `config/patterns.toml` with 50+ patterns across all 7 categories
- [ ] Implement `PatternSet::load() -> Result<PatternSet>`
- [ ] Test: load `patterns.toml`, verify all fields parsed without errors

### T3.3 — Aho-Corasick first pass (fast path)

- [ ] Build `AhoCorasick` automaton from keywords of all patterns at startup
- [ ] Implement `quick_scan(cmd: &str) -> bool` — check if any keyword matches at all
- [ ] If `quick_scan` returns `false` → return `Safe` immediately (zero-cost path, no regex)
- [ ] Benchmark: 10,000 safe commands through `quick_scan` in under 10ms total

### T3.4 — Regex full scan (slow path)

- [ ] Use `std::sync::LazyLock<Regex>` for each compiled pattern — **not** `once_cell` (deprecated since Rust 1.80)
- [ ] Implement `Scanner::full_scan(cmd: &str) -> Vec<&Pattern>`
- [ ] Return the maximum `RiskLevel` from all matched patterns
- [ ] Define `struct Assessment { risk, matched: Vec<&Pattern>, command: ParsedCommand }`
- [ ] Implement `Scanner::assess(cmd: &str) -> Assessment` (quick → full pipeline)
- [ ] Write 70 test cases, each asserting the expected `RiskLevel`

### T3.5 — Criterion benchmarks

- [ ] Create `benches/scanner_bench.rs`
- [ ] Benchmark: 1,000 safe commands (target: > 500k ops/sec)
- [ ] Benchmark: 100 dangerous commands with full regex scan
- [ ] Benchmark: worst-case heredoc command (long inline Python script)
- [ ] Run `cargo bench` — confirm p99 latency < 3ms

---

## P4 — Snapshot Engine + TUI

> Git checkpoint, Docker commit, terminal confirmation dialog, audit log
> **Timeline:** 5 days

**🔒 Gate condition (required before P5):**
On interception of a Danger command: snapshot is created, dialog is shown, user can approve or deny. Audit log is written in both cases.

---

### T4.1 — `SnapshotPlugin` trait

- [ ] Define `trait SnapshotPlugin: Send + Sync` with methods: `name`, `is_applicable`, `snapshot`, `rollback`
- [ ] Annotate trait with `#[async_trait]` from the `async-trait` crate
- [ ] Define `struct SnapshotRegistry` holding `Vec<Box<dyn SnapshotPlugin>>`
- [ ] Implement `SnapshotRegistry::default()` loading `GitPlugin` and `DockerPlugin`
- [ ] Implement `async fn snapshot_all(cwd, cmd) -> Vec<SnapshotRecord>`
- [ ] Write test with a mock plugin: verify registry only calls `is_applicable` plugins

### T4.2 — Git plugin

- [ ] `GitPlugin::is_applicable`: check for `.git/` directory in `cwd`
- [ ] `GitPlugin::snapshot`: run `git stash push --include-untracked -m "aegis-snap-<timestamp>"`
- [ ] Store stash ref as `snapshot_id`
- [ ] `GitPlugin::rollback`: run `git stash pop --index <stash_ref>`
- [ ] Graceful handling if working tree is clean (nothing to stash — log info, return ok)
- [ ] Integration test using `tempfile::TempDir` + `git init`

### T4.3 — Docker plugin

- [ ] `DockerPlugin::is_applicable`: check Docker CLI is available and containers are running
- [ ] `DockerPlugin::snapshot`: run `docker commit <container_id> aegis-snap-<timestamp>`
- [ ] `DockerPlugin::rollback`: restore from saved image via `docker run`
- [ ] Graceful skip if Docker is not installed or not running (log warning, continue)
- [ ] Test using a mock Docker CLI binary in a temp directory

### T4.4 — TUI confirmation dialog

- [ ] Implement `show_confirmation(assessment: &Assessment, snapshots: &[SnapshotRecord]) -> bool`
- [ ] Display: the full command with the dangerous fragment highlighted
- [ ] Display: list of matched patterns with human-readable descriptions
- [ ] Display: list of created snapshots (plugin name + snapshot id)
- [ ] Display: `safe_alt` suggestion when available
- [ ] `Danger` behavior: default = No, requires typing `yes` in full to proceed
- [ ] `Warn` behavior: default = Yes, Enter continues, typing `n` denies
- [ ] `Block` behavior: print reason and exit immediately, no prompt shown
- [ ] Test: simulate user input via a channel or stdin mock

### T4.5 — Audit logger

- [ ] Define `struct AuditEntry { timestamp, command, risk, matched_patterns, decision, snapshots }`
- [ ] Define `enum Decision { Approved, Denied, AutoApproved, Blocked }`
- [ ] Implement `AuditLogger::append(entry: AuditEntry) -> Result<()>`
- [ ] Write to `~/.aegis/audit.jsonl` (append-only, one JSON object per line)
- [ ] Implement `aegis audit --last N` — display last N entries formatted
- [ ] Implement `aegis audit --risk <level>` — filter entries by risk level
- [ ] Test: write 5 entries, read back, compare field-by-field

### T4.6 — Full pipeline integration

- [ ] In `main.rs`: receive command via the `-c` flag
- [ ] Pass through `Scanner::assess()`
- [ ] If `Danger`: call `snapshot_all()`, then call `show_confirmation()`
- [ ] If `Block`: print reason and exit with code `1` immediately (no snapshot needed)
- [ ] If user denied: exit with code `1` (do not execute the command)
- [ ] If approved, `Warn`, or `Safe`: `exec()` the original command transparently
- [ ] Write an audit log entry in all cases regardless of outcome
- [ ] Pass through original `stdout`, `stderr`, and exit code unchanged
- [ ] End-to-end test: `rm -rf /tmp/test_aegis` → intercepted → user denies → directory still exists

---

## P5 — Config System + Shell Integration

> `aegis.toml`, installation as `$SHELL`, agent compatibility tests
> **Timeline:** 4 days

**🔒 Gate condition (required before P6):**
`export SHELL=$(which aegis)` in `.bashrc`/`.zshrc` — Claude Code and Codex CLI are intercepted transparently. `aegis config init` generates a working `aegis.toml`.

---

### T5.1 — `AegisConfig` and `aegis.toml`

- [ ] Define `struct AegisConfig { mode, custom_patterns, allowlist, auto_snapshot_git, auto_snapshot_docker }`
- [ ] Define `enum Mode { Protect, Audit, Strict }` (`Audit` = log only, no blocking)
- [ ] Implement `Config::load()` — searches `.aegis.toml` → `~/.config/aegis/config.toml` → defaults
- [ ] Implement `Config::defaults()` for fully functional operation without any config file
- [ ] Implement `aegis config init` — generates `.aegis.toml` with inline comments explaining each field
- [ ] Implement `aegis config show` — prints the currently active config in TOML format
- [ ] Test: load a minimal config and a full config without errors

### T5.2 — Allowlist support

- [ ] Add `allowlist: Vec<String>` field to `AegisConfig`
- [ ] Implement `Allowlist::is_allowed(cmd: &str) -> bool`
- [ ] Support glob patterns in allowlist entries: `terraform destroy -target=module.test.*`
- [ ] If command matches allowlist → skip dialog and execute immediately (still log to audit)
- [ ] Test: allowlist a specific `terraform destroy -target=...`, block all other `terraform destroy`

### T5.3 — Shell wrapper mode

- [ ] Implement `aegis -c <cmd>` — the main interception mode, invoked as `$SHELL`
- [ ] Correctly forward the original command's exit code to the calling process
- [ ] Correctly forward `stdout` and `stderr` of the original command byte-for-byte
- [ ] Forward all environment variables and current working directory unchanged
- [ ] Test: `aegis -c 'echo hello'` → prints `hello`, exits `0`
- [ ] Test: `aegis -c 'exit 42'` → exits with code `42`
- [ ] Test: `aegis -c 'ls /nonexistent'` → forwards stderr, exits `2`

### T5.4 — `install.sh` and setup documentation

- [ ] Write `scripts/install.sh`: detect platform, download correct binary, place in `/usr/local/bin/aegis`
- [ ] Print post-install instructions for bash: `export SHELL=$(which aegis)` → `~/.bashrc`
- [ ] Print post-install instructions for zsh: same for `~/.zshrc`
- [ ] Add a note for Claude Code users: configure the shell path in claude settings
- [ ] Test full install on clean Ubuntu 22.04 in a Docker container
- [ ] Test full install on macOS 14 (both arm64 and x86_64)

### T5.5 — Agent compatibility tests

- [ ] Test: Claude Code executes a command through Aegis — interception works end-to-end
- [ ] Test: Codex CLI executes a command through Aegis — interception works end-to-end
- [ ] Test: Gemini CLI executes a command through Aegis — interception works end-to-end
- [ ] Verify interception latency for safe commands < 5ms (does not slow normal workflow)
- [ ] Run 1,000 safe commands sequentially — verify no performance degradation

---

## P6 — Polish and Public Release

> README, GitHub Release, binaries, changelog, community
> **Timeline:** 3 days

**🔒 Gate condition (this is the final phase):**
`v1.0.0` is published on GitHub Releases. `cargo install aegis` works. README includes a demo GIF.

---

### T6.1 — README and documentation

- [ ] Write README opening with 2–3 real incidents: DataTalks.Club, Replit, Prisma (with dates and impact numbers)
- [ ] Add one-liner install command prominently at the top of the README
- [ ] Record and embed a demo GIF or asciinema: agent attempts `terraform destroy` → Aegis intercepts → user sees dialog → denies
- [ ] Write Quick Start section: 5 steps from zero to first interception
- [ ] Write `aegis.toml` reference with all config options and their defaults
- [ ] Write full pattern list: all 50+ patterns with descriptions and safe alternatives
- [ ] Write Plugin architecture section: how to implement a custom snapshot backend
- [ ] Add badges: CI status, crates.io version, license, platform support

### T6.2 — GitHub Release pipeline

- [ ] Create `.github/workflows/release.yml` triggered on push of tag `v*`
- [ ] Build cross-compiled targets: `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`
- [ ] Generate `SHA256` checksums for each binary artifact
- [ ] Automatically create a GitHub Release with all binaries and checksums attached
- [ ] Update `install.sh` to download from GitHub Releases based on detected platform
- [ ] Test by creating tag `v1.0.0-rc1` — verify pipeline produces all four artifacts

### T6.3 — crates.io publication

- [ ] Fill in `Cargo.toml` metadata: `description`, `repository`, `homepage`, `keywords`, `categories`
- [ ] Run `cargo publish --dry-run` — verify all required files are included in the package
- [ ] Run `cargo publish` — publish `v1.0.0` to crates.io
- [ ] Verify `cargo install aegis` installs successfully and runs correctly

### T6.4 — Post-release announcement

- [ ] Post to Reddit: `r/ClaudeAI`, `r/rust`, `r/devops` — include the demo GIF
- [ ] Open a thread in the Anthropic Discord `#claude-code-lounge`
- [ ] Post on X/Twitter with the demo GIF and install one-liner
- [ ] Create a GitHub Discussion: `v2 Roadmap — what snapshot backends do you need?`
- [ ] Add `ROADMAP.md` to the repo: planned v2 features (Cloud plugin, Slack notify, Policy DSL, rollback command)

---

## Reference

### Architecture decisions

- Use `std::sync::LazyLock<Regex>` for pattern compilation — **not** `once_cell` (deprecated since Rust 1.80)
- Use `async-trait` crate for async methods on `dyn SnapshotPlugin` — `async fn` is not object-safe without it
- Use `Cow<'static, str>` in `Pattern` — supports built-in (`&'static str`) and user-defined (`String`) patterns
- Add `#[non_exhaustive]` to `RiskLevel` — forward compatibility when adding new levels in v2
- Single-crate structure for v1 — do not add a workspace until the project has 2+ crates with shared deps

### Key types

```
RiskLevel:     Safe < Warn < Danger < Block  (#[non_exhaustive])
AegisError:    Parse | Snapshot | Config | Io  (thiserror)
ParsedCommand: executable + args + inline_scripts + raw
Assessment:    risk + matched_patterns + parsed_command
AuditEntry:    timestamp + command + risk + decision + snapshots
Decision:      Approved | Denied | AutoApproved | Blocked
```

### Pattern categories

```
Filesystem  (FS-001..006)  rm -rf, find -delete, dd, shred, truncate
Git         (GIT-001..006) reset --hard, clean -f, push --force, filter-branch
Database    (DB-001..006)  DROP TABLE, DELETE without WHERE, --accept-data-loss, FLUSHALL
Cloud       (CL-001..009)  terraform destroy, aws terminate, kubectl delete, pulumi destroy
Docker      (DK-001..004)  system prune, volume prune, docker-compose down -v
Process     (PS-001..004)  kill -9 1, pkill production services, chmod 777
Package     (PKG-001..002) curl | bash, install without integrity check
```

### CI checklist (every push)

```
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo audit
cargo deny check
cargo build --release  (ubuntu-latest + macos-latest)
```
