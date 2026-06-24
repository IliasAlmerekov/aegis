# ADR-011: Agent hooks rewrite transparently in Rust; setup-shell escapes paths

## Status

Accepted

## Context

Three integration failures appeared around how Aegis becomes the interception
layer after a package-manager install:

1. `aegis setup-shell` rejected scoped npm install paths
   (`.../node_modules/@iliasalmerekov/aegis/vendor/aegis`) with
   `error: real shell path contains unsafe characters`. The managed rc block
   interpolated paths into `export SHELL="..."` and a single strict
   allow-list validator (ASCII alnum plus `_ . / + -`) guarded both the real
   shell and the Aegis binary path, so the `@` in a scoped path was refused and
   the error always blamed the "real shell path" even when the Aegis binary
   path was the offender.

2. The Codex `PreToolUse` hook *denied* unwrapped Bash commands and instructed
   the model to retry through `aegis --command`, relying on the model to follow
   text. It also depended on `jq` and `python3` at hook-exec time. The Codex
   `SessionStart` hook emitted its guidance under `context`, but Codex expects
   `additionalContext`, producing `hook returned invalid session start JSON
   output` on some installs.

3. Enforcement strength differed across agents: Claude rewrote commands in Rust
   while Codex denied them in shell, and both leaned on `$SHELL`/PATH and
   external tooling that can drift.

## Decision

- **setup-shell escapes instead of rejecting.** Paths are written into the
  managed rc block as POSIX single-quoted values (`export SHELL='...'`), with
  embedded single quotes encoded as `'\''`. Validation is reduced to rejecting
  only empty paths and control characters (which could break the rc line). Two
  purpose-named validators, `validate_real_shell_path` and
  `validate_aegis_binary_path`, replace the shared one so errors name the path
  that actually failed. Legitimate package-manager paths (scoped npm `@`,
  spaces) are accepted and made inert by quoting.

- **Hooks rewrite transparently in Rust.** Both Claude and Codex `PreToolUse`
  hooks route through the Rust `aegis hook`, which emits
  `permissionDecision: "allow"` with `updatedInput.command =
  aegis --command '<original>'`. A command already in canonical wrapper form is
  passed through untouched; a command that begins with the bare `aegis` word but
  is not a canonical wrapper is denied (fail closed) rather than re-wrapped. The
  Codex hook script is a thin shim that preserves the CI/disabled toggle and
  then `exec`s the binary, removing the `jq`/`python3` runtime dependency.

- **Codex `SessionStart` uses `additionalContext`.** The legacy `context` field
  is gone; guidance now also describes that `PreToolUse` rewrites transparently.

- **Codex hook scripts embed an absolute binary path.** The pre-tool-use script
  has `__AEGIS_BIN__` replaced at install time with a shell-quoted absolute path
  to the running Aegis binary, so the hook works under a minimal hook-exec PATH.
  An explicit `AEGIS_BIN` in the environment still wins.

- **Claude's registered command stays `aegis hook` (PATH-based).** Migrating it
  to an absolute path is intentionally deferred: `scripts/uninstall.sh` prunes
  the Claude registration by matching the literal `aegis hook`, and a
  machine-specific absolute path cannot be reliably matched there without risking
  orphaned registrations. This is the one place where PATH resolution is retained.

## Consequences

- The reported npm/scoped-path and Codex `SessionStart` failures are fixed, and
  Codex now intercepts by transparent rewrite rather than model-followed text.
- Codex hooks no longer require `jq` or `python3`.
- Enforcement is no longer weaker for Codex than for Claude; both fail closed on
  malformed wrappers and on missing/malformed hook input for `PreToolUse`.
- Security posture is preserved: single-quote escaping neutralizes injection in
  the rc block, and the recursion guard (real shell may never resolve to the
  Aegis binary) is unchanged.
- Trade-off: Claude's hook still depends on `aegis` being on PATH at hook-exec
  time. Aegis is not a universal background daemon; it intercepts through the
  `$SHELL` proxy, supported agent hooks, and explicit `aegis --command`.
