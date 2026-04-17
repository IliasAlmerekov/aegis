# INTEGRATIONS

Generated: 2026-04-17
Focus: tech+arch

## External runtime integrations

### Shell / process execution
- Uses the real shell resolved from:
  1. `AEGIS_REAL_SHELL`
  2. `SHELL`
  3. `/bin/sh` fallback
- Install/uninstall scripts modify shell rc files for `bash` / `zsh`
- Watch mode uses NDJSON framing over stdin/stdout and `/dev/tty` for confirmations

### Git
- Command interception patterns include destructive Git operations
- Snapshot provider integrates with `git` for stash-based snapshots/rollback
- Installer helper script `setup-git-hooks.sh` configures local hooks path

### Docker
- Snapshot provider integrates with `docker`
- Docker provider scope is configurable (`docker_scope`)

### Databases / data snapshots
Built-in snapshot providers:
- PostgreSQL (`pg_dump`, `pg_restore`)
- MySQL / MariaDB (`mysqldump`, `mysql`)
- SQLite (file-copy snapshots)
- Supabase (currently DB-focused, built around PostgreSQL transport + manifest artifacts)

This is a notably broad integration surface for a pre-1.0 CLI.

## Filesystem and environment contracts
- Config layering:
  - project `.aegis.toml`
  - global `~/.config/aegis/config.toml`
  - defaults
- Audit log default path: `~/.aegis/audit.jsonl`
- Snapshot storage default path: `~/.aegis/snapshots`
- Optional audit path override: `AEGIS_AUDIT_PATH`
- CI detection override: `AEGIS_CI`
- Test-only interactive override appears in UI/tests: `AEGIS_FORCE_INTERACTIVE`

## Release / distribution integrations
- GitHub Actions CI and Release workflows
- GitHub Release assets with checksum sidecars
- Convenience installer downloads from GitHub Releases
- Manual verification-first install path documented in `docs/release-readiness.md`

## Documentation/test integrations
- Multiple doc-contract tests ensure README / threat model / release docs stay aligned
- Installer flow is covered by integration tests with stubbed `curl`, `sha256sum`, `shasum`
- Platform support policy is tested directly

## Supply-chain posture
- `cargo audit` in CI
- `cargo deny` policy for licenses, banned crates, registry/source policy
- No SBOM, signing, provenance, or attestations yet
