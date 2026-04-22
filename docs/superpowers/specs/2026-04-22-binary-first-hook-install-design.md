# Binary-First Agent Hook Installation Design

Date: 2026-04-22
Status: Approved design
Scope: Claude Code and Codex hook installation UX for release installs and local installs

## Objective

Make Aegis hook installation work consistently after a standard release install, including:

- `curl -fsSL .../scripts/install.sh | sh`
- installs from a local checkout
- later reinstallation after Claude Code or Codex is installed

The desired user experience is:

1. the installer always sets up the shell-wrapper flow
2. the installer automatically attempts agent hook setup when supported agent directories already exist
3. if agent directories do not exist yet, the installer prints an explicit follow-up command
4. users can always rerun a single supported command later to install hooks explicitly

## Problem Statement

Current behavior is inconsistent between release installs and local-checkout installs.

Today:

- `scripts/install.sh` installs the binary and shell wrapper
- automatic hook setup is tied to `scripts/agent-setup.sh`
- `scripts/agent-setup.sh` depends on sibling files in `scripts/hooks/`
- `scripts/install.sh` only auto-runs that setup when it can resolve a real local checkout

As a result, `curl | sh` installs often finish without Claude Code or Codex hooks, even though:

- the installed binary is present
- the user expects Aegis integration to be complete
- the repository already contains binary-side hook installation logic in `src/install.rs`

This creates avoidable confusion, especially for Codex and Claude Code users who expect command interception to begin immediately.

## Goals

- Make hook installation behave the same for release installs and local installs
- Keep hook setup idempotent
- Preserve current Claude Code and Codex hook semantics
- Avoid silently creating agent configuration for tools the user has never launched
- Give users one explicit follow-up command for later hook setup
- Keep installation behavior honest and easy to debug

## Non-Goals

- Changing hook enforcement policy
- Changing the behavior of `aegis hook`
- Changing Codex `PreToolUse` or `SessionStart` hook semantics
- Introducing hook setup for agents beyond Claude Code and Codex in this change
- Auto-creating `~/.claude` or `~/.codex` purely to pre-seed future agent installs

## Recommended Approach

Adopt a binary-first installation model:

1. make the installed `aegis` binary the canonical mechanism for hook installation
2. expose an explicit user-facing command for hook installation
3. update `scripts/install.sh` to invoke the installed binary directly after installation
4. preserve idempotent behavior and honest skip messaging

This removes the dependency on a local checkout during normal release installation and makes hook setup repeatable at any time.

## User-Facing Contract

### New canonical command

Expose an explicit hook installation command:

```bash
aegis install-hooks --all
aegis install-hooks --claude-code
aegis install-hooks --codex
```

The command must be safe to rerun.

### Installer behavior

After `curl | sh`, the installer should:

1. install the `aegis` binary
2. install the shell-wrapper setup block
3. invoke the installed binary with:

```bash
<absolute-path-to-aegis> install-hooks --all
```

4. print a clear final status

### Automatic setup behavior

- If `~/.claude` exists, install Claude Code hooks
- If `~/.codex` exists, install Codex hooks
- If neither exists, do not create them just for future use
- If neither exists, print a follow-up instruction:

```text
Agent hook setup skipped; no supported agent directories were detected.
Run: aegis install-hooks --all
```

### Idempotence

Repeated runs must:

- not duplicate Claude Code hook entries in `~/.claude/settings.json`
- not duplicate Codex hook entries in `~/.codex/hooks.json`
- not accumulate duplicate hook files or conflicting registrations

## Internal Design

### Canonical implementation path

The installed binary becomes the source of truth for hook installation.

The existing installation logic in `src/install.rs` already covers most of the needed work:

- Claude Code settings patching
- Codex hook payload materialization
- Codex `hooks.json` patching
- idempotence checks

The design therefore standardizes the public entrypoint instead of inventing a second installation mechanism.

### CLI shape

Keep backward compatibility for existing flows while making the public UX explicit.

Recommended contract:

- preserve `aegis install` for compatibility
- add `aegis install-hooks` as the preferred user-facing name
- support:
  - `--all`
  - `--claude-code`
  - `--codex`
- retain `--local` only if still needed for Claude Code development/testing flows, and document that it applies only to the Claude local settings path

### Installer flow

`scripts/install.sh` should no longer rely on `scripts/agent-setup.sh` as the primary auto-install path.

Instead it should:

1. compute the installed binary path
2. invoke that absolute path directly
3. interpret the binary's result and print honest summary output

Using an absolute installed path is required so the installer does not depend on:

- the current shell session reloading PATH
- the new shell-wrapper activation block already being active

### Role of `scripts/agent-setup.sh`

For compatibility:

- keep `scripts/agent-setup.sh` temporarily
- convert it into a thin wrapper around the binary command where practical

This preserves existing documentation and scripts while moving the real contract into the binary.

## Error and Skip Semantics

### No supported agent directories

If neither `~/.claude` nor `~/.codex` exists:

- return a skip outcome, not an error
- do not create placeholder agent directories
- print the explicit follow-up command

### One agent present, one absent

If one supported directory exists and the other does not:

- install hooks for the detected agent
- skip the missing agent
- return overall success

### Existing registrations already present

If the expected hook registration already exists:

- do not duplicate it
- report `AlreadyPresent`
- do not rewrite files unnecessarily

### Invalid JSON or invalid config shape

If a detected agent config file exists but is malformed:

- fail the attempted install for that agent
- print a concrete, file-specific error
- return a non-zero exit code for the command

This is intentional fail-closed behavior for installation correctness. A detected-but-broken configuration should not silently degrade into a no-op.

### Permission failures

If hook files or config files cannot be written:

- surface the write error explicitly
- return a non-zero exit code
- do not claim success

## Testing Plan

### Unit tests

Extend `src/install.rs` coverage for:

- explicit selection logic for `--all`, `--claude-code`, and `--codex`
- Claude Code installation idempotence
- Codex hook materialization idempotence
- skip behavior when directories are absent
- failure behavior on malformed JSON
- no duplicate registrations after repeated runs

### Integration tests

Update installer-flow tests to verify that release-style installation:

- attempts hook installation through the installed binary
- installs hooks automatically when supported agent directories exist
- prints the skip message when no supported directories exist
- remains idempotent across repeated runs

### Compatibility tests

Keep regression coverage that confirms:

- compatibility entrypoints still work
- uninstall removes installed hook payloads and JSON registrations
- current Claude Code and Codex hook behavior remains unchanged

## Documentation Changes

Update:

- `README.md`
- installer post-install output
- troubleshooting documentation
- current release notes / changelog

Documentation must clearly say:

- release installs now auto-attempt hook setup through the installed binary
- hook setup does not require a local checkout
- if Claude Code or Codex is installed later, rerun:

```bash
aegis install-hooks --all
```

## Rollout Notes

This design intentionally limits scope to installation UX and contract cleanup.

It does not change:

- Aegis classification behavior
- enforcement model
- hook-side security semantics
- shell-wrapper semantics

That keeps the change low-risk while solving the main user-facing problem.

## Open Questions

None for the approved design.

The user-approved behavior is:

- auto-install hooks whenever supported agent directories already exist
- otherwise print a clear manual follow-up command for later explicit installation
