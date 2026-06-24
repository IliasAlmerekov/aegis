# ADR-012: Claude Code hook uses an absolute shim, at parity with Codex

## Status

Accepted

## Context

Claude Code is intercepted **only** through its `PreToolUse` hook: its Bash
tool ignores a non-bash/zsh `$SHELL`, so the `$SHELL` proxy never sees the
agent's own commands. ADR-011 made the Codex hook PATH-independent by embedding
an absolute `__AEGIS_BIN__` in a thin shim, but intentionally deferred the same
treatment for Claude because `scripts/uninstall.sh` pruned the Claude
registration by matching the literal `aegis hook`.

The result was three inconsistencies:

1. `src/install/claude.rs` registered the **bare** `command: "aegis hook"`, so
   whether `aegis` resolved at hook-exec time depended on Claude's PATH — the
   one place PATH resolution was retained.
2. The legacy jq-based `scripts/hooks/claude-code.sh` (version 1) targeted
   `~/.claude/hooks/aegis-rewrite.sh`; the binary-first installer never
   materialized it, yet `scripts/uninstall.sh` still cleaned it up. The on-disk
   story was inconsistent.
3. Docs/UX suggested pointing Claude Code's shell at Aegis, which does nothing
   for the Bash tool.

This was **not** a jq problem for the active path — `aegis hook` was already
jq-free. The active fragility was the PATH dependency of the bare command.

## Decision

- **Materialize an absolute shim.** `aegis install-hooks --claude-code` (and
  `--all`) writes `~/.claude/hooks/aegis-pre-tool-use.sh` with `__AEGIS_BIN__`
  replaced by a shell-quoted absolute path to the Aegis binary, and registers
  that absolute path in `settings.json` `PreToolUse` / matcher `Bash`. The shim
  is a silent no-op when disabled outside CI, identical to Codex. Local
  (`--local`) installs place the shim next to the project settings file and
  register its absolute path on the same code path.

- **Migrate, not append.** `apply_installation` is now prune-then-add. An
  aegis-managed Bash hook is identified by **basename** — exactly `aegis hook`,
  `aegis-rewrite.sh`, or `aegis-pre-tool-use.sh` — so a moved/renamed home
  directory still migrates. Legacy managed registrations are pruned (and
  entries emptied by pruning are dropped), the canonical entry is preserved,
  and unrelated user hooks (including commands that merely mention `aegis`) are
  kept. Reinstall is idempotent.

- **Share the installer machinery.** `write_executable`, `resolved_aegis_bin`,
  and `combine_outcomes` live in `install::mod` and are reused by both the
  Claude and Codex installers, eliminating the duplication ADR-011 left behind.

- **Cross-compatible deny reason.** `hook_deny_output` emits a top-level
  `reason` mirroring `hookSpecificOutput.permissionDecisionReason`, because
  Claude reads the top-level `reason` while Codex reads
  `permissionDecisionReason`. A top-level legacy `decision` field is
  deliberately not emitted.

- **Uninstall follows.** `scripts/uninstall.sh` removes the new shim and prunes
  its absolute-path registration, alongside the existing legacy `aegis hook` /
  `aegis-rewrite.sh` cleanup.

## Consequences

- Claude Code interception is PATH-independent and jq-free, at parity with
  Codex; both fail closed on malformed hook input and on non-canonical `aegis`
  wrappers.
- The two hook shims (`codex-pre-tool-use.sh`, `claude-code.sh`) are now
  byte-identical except for the header comment. Unifying them into one templated
  script is a tracked follow-up, kept out of this change to limit blast radius.
- Uninstall still prunes only the global `~/.claude` path; project-local Claude
  installs remain out of uninstall's scope (documented in
  `docs/troubleshooting.md`).
- The ADR-011 caveat that Claude's hook stays PATH-based is superseded; that
  paragraph is retained in ADR-011 as historical record.