# External Integrations

**Analysis Date:** 2026-03-27

## APIs & External Services

**Git:**
- Git CLI - Used for snapshot creation (`src/snapshot/git.rs`)
  - Method: subprocess invocation via `tokio::process::Command`
  - Operations: `git rev-parse --git-dir` (detect repo), `git status --porcelain` (check clean), `git stash push --include-untracked` (snapshot), `git stash pop` (rollback)
  - Auth: Uses system git config (SSH keys, credentials already configured)

**Docker:**
- Docker CLI - Used for container snapshots (`src/snapshot/docker.rs`)
  - Method: subprocess invocation via `tokio::process::Command`
  - Operations: `docker ps -q` (list containers), `docker inspect` (capture config), `docker commit` (snapshot), `docker container rm` (cleanup), `docker run` (rollback)
  - Auth: Uses system docker socket and credentials (if configured)

## Data Storage

**Databases:**
- None. Aegis is stateless between invocations.

**File Storage:**
- Local filesystem only (no cloud storage integration)
  - Audit log: `~/.aegis/audit.jsonl` (append-only JSONL file)
  - Global config: `~/.config/aegis/config.toml` (TOML file)
  - Project config: `.aegis.toml` in project root (TOML file)
  - Snapshot data: managed by git and docker CLIs (no direct Aegis handling)

**Caching:**
- In-memory only (no Redis, Memcached, or persistent cache)
  - Pattern database: compiled at startup via `std::sync::LazyLock` in `src/interceptor/mod.rs`
  - Scanner instance: cached in `BUILTIN_SCANNER` static
  - Custom patterns: cached per config fingerprint in `CUSTOM_SCANNER_CACHE`

## Authentication & Identity

**Auth Provider:**
- Custom — no external auth service
- Aegis requires no authentication or login

**Human Authorization:**
- Interactive shell confirmation via stdin/stderr (`src/ui/confirm.rs`)
  - Risk level `Block`: auto-blocked, no prompt
  - Risk level `Danger`: requires user type `yes` exactly to proceed
  - Risk level `Warn`: requires user not type `n` to proceed
  - Risk level `Safe`: auto-approved

**CI Detection:**
- Detects non-interactive environments via `io::stdin().is_terminal()`
- Policy enforcement (`src/config/model.rs`):
  - `CiPolicy::Block` (default): hard-blocks all non-safe commands in CI
  - `CiPolicy::Allow` (opt-in): passes through commands without prompting in CI

## Monitoring & Observability

**Error Tracking:**
- None. Aegis does not report to external error tracking services.

**Logs:**
- Structured logging via `tracing` framework (`src/audit/logger.rs`)
- Append-only JSONL audit log at `~/.aegis/audit.jsonl`
  - Format: One JSON object per line (RFC 3339 timestamp, risk level, decision, matched patterns, snapshot records)
  - Rotation (optional): Enabled via `[audit]` config section
    - Supported: file size limits, retention count, gzip compression of rotated files
  - Backward compatibility: Deserializer accepts both RFC 3339 and Unix timestamp formats for migrations

**Internal Logging (development):**
- `tracing-subscriber` with `fmt` formatter and `env-filter` support
- Controlled by `RUST_LOG` environment variable (default: off in production)
- Output: stderr only (never stdout, which is reserved for command passthrough)

## CI/CD & Deployment

**Hosting:**
- GitHub Releases - Binary distribution
  - Builds published for: Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64
  - Artifacts include SHA256 checksums

**CI Pipeline:**
- GitHub Actions (`.github/workflows/ci.yml`)
  - Runs on every push to any branch and all pull requests
  - Jobs:
    1. `check`: fmt, clippy, test, cargo-audit, cargo-deny
    2. `build`: release builds on Ubuntu and macOS

**Release Pipeline:**
- GitHub Actions (`.github/workflows/release.yml`)
  - Triggered on: tag push matching `v*` pattern
  - Builds native binaries for all 4 platforms
  - Uses `cross` crate for aarch64 cross-compilation
  - Publishes to GitHub Releases with generated release notes

**Installation:**
- Shell script installer (`scripts/install.sh`) - detects platform and downloads pre-built binary from GitHub Releases
- Alternative: `cargo install aegis` from crates.io

## Environment Configuration

**Required env vars:**
- None for normal operation

**Optional env vars:**
- `RUST_LOG` - Control tracing verbosity (format: `module::path=level`, e.g., `debug`, `info`, `warn`)
- `AEGIS_FORCE_INTERACTIVE=1` - Force interactive mode even when stdin is piped (testing only; must never be set in production)
- `CARGO_TERM_COLOR=always` - CI/CD helper (set in workflows)
- `RUST_BACKTRACE=1` - CI/CD helper (set in workflows)

**Secrets location:**
- No secrets are stored by Aegis
- Git and Docker credentials use system configuration (not managed by Aegis)
- Audit log may contain sensitive data (e.g., command arguments); should be protected with filesystem permissions

## Webhooks & Callbacks

**Incoming:**
- None. Aegis is not a service and does not expose any HTTP endpoints.

**Outgoing:**
- None. Aegis does not make HTTP requests or call external APIs.

## Subprocess Execution

**Direct subprocess calls:**
1. `git rev-parse --git-dir` - Detect if cwd is a git repository
2. `git status --porcelain` - Check if git working tree is clean
3. `git stash push --include-untracked -m <message>` - Create git snapshot
4. `git stash pop <stash-id>` - Rollback git snapshot
5. `docker ps -q` - List running containers
6. `docker inspect <container-id>` - Inspect container configuration
7. `docker commit <container-id> <image-name>` - Create docker snapshot
8. `docker run ...` - Rollback docker snapshot
9. `docker container rm ...` - Clean up snapshot images

All subprocess calls use `tokio::process::Command` for async non-blocking execution. Failures in snapshot creation are logged as warnings and do not prevent command execution.

---

*Integration audit: 2026-03-27*
