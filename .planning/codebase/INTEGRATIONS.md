# Aegis Integrations Scan

Last refreshed: 2026-04-12
Focus: tech+arch

## User/runtime integration surfaces

### 1. Shell-wrapper execution

- Main entrypoint is `aegis -c '<cmd>'`.
- `src/main.rs` resolves the real shell from:
  1. `AEGIS_REAL_SHELL`
  2. `SHELL` if it does not point back to Aegis
  3. `/bin/sh` fallback
- Installer writes managed shell-wrapper blocks into `~/.bashrc` or `~/.zshrc`.

### 2. Watch mode

- `aegis watch` reads NDJSON frames from stdin and emits NDJSON results to stdout.
- Implemented in `src/watch.rs`.
- Human confirmation in watch mode goes through `/dev/tty`, so this surface is Unix-oriented.

### 3. Config integration

- Effective config layers:
  - built-in defaults
  - `~/.config/aegis/config.toml`
  - `.aegis.toml`
- Config commands:
  - `aegis config init`
  - `aegis config show`
  - `aegis config validate`
- Structured allowlist, custom patterns, snapshot policy, CI policy, audit policy are all wired into runtime.

## External tool / platform integrations

### Git

- `src/snapshot/git.rs`
- Used for:
  - repo detection via `git rev-parse --git-dir`
  - best-effort pre-danger snapshots via `git stash push --include-untracked`
  - rollback via `git stash pop --index`
- Coverage includes:
  - repo root
  - subdirectories
  - staged + unstaged changes
  - untracked files
  - worktrees
  - rollback conflict path

### Docker

- `src/snapshot/docker.rs`
- Used for:
  - container discovery via `docker ps -q`
  - metadata capture via `docker inspect`
  - filesystem snapshot via `docker commit`
  - rollback via `docker stop` / `docker rm` / `docker run`
- Supports scoped selection:
  - `Labeled`
  - `All`
  - `Names`

### Filesystem / local machine

- Audit log: `~/.aegis/audit.jsonl`
- Optional rotation archives: `audit.jsonl.N` or `audit.jsonl.N.gz`
- Lock file: companion `.lock`
- Installer target default: `/usr/local/bin/aegis`

## CI / GitHub integrations

- `.github/workflows/ci.yml`
  - fmt
  - clippy
  - tests
  - cargo-audit
  - cargo-deny
  - release builds
  - scanner benchmark policy
- `.github/workflows/release.yml`
  - tag-triggered release
  - Linux + macOS asset builds
  - SHA-256 checksum generation
  - GitHub Release publication

## Audit / machine-readable integrations

- `aegis audit`
  - text / json / ndjson output
  - filters by risk, decision, time, substring
  - summary mode
  - integrity verification mode
- `aegis --output json`
  - evaluation-only decision JSON
  - includes risk, decision, matched patterns, allowlist match, snapshot plan, CI state

## Public-repo hygiene signals

- `.gitignore` excludes `.env`, `.claude/`, `.codex/`, `.worktrees/`, `target/`.
- Quick regex scan found no obvious committed secret material.
- Public-facing governance files present:
  - `LICENSE`
  - `CONTRIBUTING.md`
  - `CODE_OF_CONDUCT.md`

## Integration gaps / caution areas

- README points to `AEGIS.md` for the full pattern table, but no `AEGIS.md` file exists in the repository.
- `docs/architecture-decisions.md` says README documents the security model and non-goals; repository scan did not find that section in README.
- Release workflow generates checksum files, but `scripts/install.sh` does not consume or verify them.
- No threat-model or limitations document was found under `docs/`.
- Windows is explicitly unsupported and the installer rejects it; this is honest, but it limits “general public” reach.

## Bottom line from integrations view

The repo is already well-integrated with shell setup, Git, Docker, CI, audit, release automation, and machine-readable outputs.  
The biggest public-release weakness is not missing integration plumbing; it is missing verification/honesty layers around those integrations in the public docs and install path.
