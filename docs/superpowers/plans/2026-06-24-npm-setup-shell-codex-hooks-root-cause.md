# Plan: fix npm `setup-shell` and Codex/Claude hook root causes

Date: 2026-06-24  
Scope: npm/Homebrew/curl installation, `aegis setup-shell`, Claude Code hooks, Codex hooks  
Status: Proposed implementation plan

## Problem statement

The reported failures are not one bug; they are three related integration
failures around how Aegis becomes the command interception layer after
installation:

1. `npm i -g @iliasalmerekov/aegis && aegis setup-shell` fails on WSL2 and
   macOS with:

   ```text
   error: real shell path contains unsafe characters
   ```

2. After npm installation, Claude/Codex still require the user to tell the model
   to run commands through Aegis. That is poor UX and weak enforcement.

3. After curl installation on macOS, Codex can show:

   ```text
   SessionStart hook (failed)
   error: hook returned invalid session start JSON output
   ```

This plan targets root causes, not symptom-specific workarounds.

## Root-cause analysis

### RC1 — npm scoped package path is rejected by `setup-shell`

The npm package is scoped:

```text
@iliasalmerekov/aegis
```

The installed binary path can therefore include `@`, for example:

```text
.../node_modules/@iliasalmerekov/aegis/vendor/aegis
```

`src/install/shell.rs` currently validates both the real shell path and the
resolved Aegis binary path with the same strict validator. That validator only
accepts ASCII alphanumeric characters plus `_`, `.`, `/`, `+`, and `-`.

Result: the npm-installed Aegis path is rejected even though it is a legitimate
package-manager path. The error text says `real shell path`, but the failing
path may actually be the Aegis binary path.

### RC2 — hook behavior is split between prompt injection, deny, and rewrite

Current behavior is inconsistent:

- `$SHELL` proxy works only for tools that honor `$SHELL -c`.
- Claude Code has a rewrite path via `aegis hook`.
- Codex `PreToolUse` currently denies unwrapped commands and tells the model how
  to retry through Aegis.
- Codex `SessionStart` injects instructions, but instruction injection is not
  enforcement.

This means Aegis is partially relying on the model to follow text instructions
instead of rewriting supported tool calls at the hook boundary.

### RC3 — Codex `SessionStart` output uses the wrong field

The current Codex session-start hook emits:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "SessionStart",
    "context": "..."
  }
}
```

Codex expects hook-specific context under `additionalContext` for `SessionStart`.
The syntactic JSON is valid, but the output shape is not the supported schema.

### RC4 — runtime hooks depend on shell tools and PATH

The current shell hook scripts depend on external runtime details:

- `jq`
- `python3`
- `aegis` being available on `PATH`
- agent-specific hook trust/enablement state

For a security guardrail, missing parser tooling or PATH drift must not silently
weaken interception. Hook parsing and rewriting should live in the Rust binary
where possible.

## Non-goals

- Do not claim that Aegis is a background daemon that can intercept every
  process on the machine.
- Do not add OS-level process monitoring or privilege-boundary claims.
- Do not suggest bypassing Aegis blocks.
- Do not add new dependencies unless explicitly approved.
- Do not modify dependency or CI policy files as part of this plan.

## Desired behavior

After a supported install:

1. `aegis setup-shell` accepts real package-manager paths, including scoped npm
   package paths.
2. Generated shell rc blocks are injection-safe.
3. Codex `SessionStart` emits schema-valid context.
4. Codex `PreToolUse` transparently rewrites supported Bash commands to:

   ```text
   aegis --command '<original command>'
   ```

   using `permissionDecision: "allow"` and `updatedInput`.

5. Hook installation prefers absolute Aegis binary paths over PATH-dependent
   commands.
6. Missing or malformed hook input fails closed where a blocking hook is expected.
7. Docs describe the honest boundary: Aegis intercepts through supported
   shell-proxy and hook integration paths, not through a universal daemon.

## Implementation phases

### Phase 1 — Red tests for `setup-shell` npm path handling

Files likely touched:

- `src/install/shell.rs`
- `tests/installer_platform.rs` or a focused setup-shell integration test

Add tests:

1. `setup_shell_accepts_scoped_npm_aegis_binary_path`
   - `--aegis-bin` contains `node_modules/@iliasalmerekov/aegis/vendor/aegis`.
   - `--shell /bin/zsh`.
   - Expected: managed block is written successfully.

2. `setup_shell_reports_aegis_binary_path_when_aegis_bin_is_invalid`
   - invalid `--aegis-bin` includes newline or control char.
   - Expected: error names `aegis binary path`, not `real shell path`.

3. `setup_shell_quotes_paths_in_managed_block`
   - path contains a safe but quote-sensitive character.
   - Expected: output uses POSIX-safe single-quote escaping.

Rust best-practice constraints:

- Keep path validation helpers small and typed by purpose.
- Prefer `&Path` / `&str` parameters.
- Return `Result<(), String>` only if staying consistent with current module;
  otherwise introduce typed errors only if broader refactor is already planned.
- No `unwrap()` / `expect()` in production paths.

### Phase 2 — Fix shell rc quoting and path validation

Implement:

1. Add a POSIX shell-quote helper:

   ```rust
   fn shell_quote(value: &str) -> String
   ```

   It should emit single-quoted strings and escape embedded single quotes as:

   ```text
   'foo'\''bar'
   ```

2. Change managed block generation from:

   ```text
   export AEGIS_REAL_SHELL="..."
   export SHELL="..."
   ```

   to:

   ```text
   export AEGIS_REAL_SHELL='...'
   export SHELL='...'
   ```

3. Split validation:

   - `validate_real_shell_path`
   - `validate_aegis_binary_path`
   - shared helper for control characters / empty path

4. Reject:

   - empty paths
   - control characters
   - newline / carriage return
   - self-recursive real shell

5. Allow legitimate package-manager paths, including `@`.

Verification:

```bash
rtk cargo test setup_shell
rtk cargo test installer_platform
```

### Phase 3 — Red tests for Codex `SessionStart`

Files likely touched:

- `scripts/hooks/codex-session-start.sh`
- `src/install/codex.rs`
- `tests/agent_hooks.rs`

Add tests:

1. `codex_session_start_emits_additional_context`
   - run the installed session-start hook.
   - parse stdout as JSON.
   - assert:

     ```text
     hookSpecificOutput.hookEventName == "SessionStart"
     hookSpecificOutput.additionalContext is string
     hookSpecificOutput.context is absent
     ```

2. `codex_session_start_noops_with_empty_stdout_when_disabled`
   - ensure disabled mode still emits no stdout and exits 0.

### Phase 4 — Fix Codex `SessionStart` schema

Change emitted JSON from:

```json
"context": "..."
```

to:

```json
"additionalContext": "..."
```

Keep the message content but make it less model-dependent after Phase 5 lands.
The context can explain that Aegis hooks transparently route supported Bash
commands and that denied Aegis decisions must not be bypassed.

Verification:

```bash
rtk cargo test agent_hooks
```

### Phase 5 — Red tests for transparent Codex `PreToolUse` rewrite

Files likely touched:

- `scripts/hooks/codex-pre-tool-use.sh`
- `src/install/hook.rs` or new hook submodule
- `src/main.rs`
- `src/cli_dispatch.rs`
- `tests/agent_hooks.rs`

Add tests:

1. `codex_pre_tool_use_rewrites_unwrapped_bash_command`
   - input command: `git status`.
   - expected JSON:

     ```json
     {
       "hookSpecificOutput": {
         "hookEventName": "PreToolUse",
         "permissionDecision": "allow",
         "updatedInput": {
           "command": "aegis --command 'git status'"
         }
       }
     }
     ```

2. `codex_pre_tool_use_noops_for_exact_aegis_command_wrapper`
   - command already equals canonical wrapper.
   - expected: no stdout or explicit allow without mutation, depending on chosen
     contract.

3. `codex_pre_tool_use_rejects_malformed_aegis_wrapper`
   - command starts with `aegis` but is not canonical.
   - expected: deny with clear reason.

4. `codex_pre_tool_use_handles_embedded_single_quotes`
   - input includes single quotes.
   - expected shell-quoted wrapper round-trips.

### Phase 6 — Move hook parsing/rewrite into Rust

Add a Rust-owned hook path, for example one of:

```text
aegis hook --agent codex --event pre-tool-use
aegis hook --agent codex --event session-start
aegis hook --agent claude --event pre-tool-use
```

or subcommands:

```text
aegis hook codex-pre-tool-use
aegis hook codex-session-start
aegis hook claude-pre-tool-use
```

Preferred implementation rules:

- Keep `src/main.rs` thin: only CLI shape and dispatch.
- Put hook protocol code under `src/install/hook.rs` or a focused
  `src/install/hook/` module.
- Parse JSON with existing `serde_json`.
- Use borrowing where possible; only clone `tool_input` when constructing
  `updatedInput`.
- Use small pure functions:
  - `parse_hook_input`
  - `extract_command`
  - `is_canonical_aegis_wrapper`
  - `rewrite_command`
  - `deny_output`
  - `allow_rewrite_output`
- Add tests near the hook code and integration tests under `tests/agent_hooks.rs`.

Security behavior:

- malformed JSON: deny for `PreToolUse`
- missing `tool_input.command`: deny or noop only if the Codex event contract
  proves the hook is not for Bash; otherwise fail closed
- unsupported tool input type: deny
- command beginning with `aegis` but not canonical wrapper: deny

### Phase 7 — Make installed hooks use absolute Aegis binary paths

Files likely touched:

- `src/install/claude.rs`
- `src/install/codex.rs`
- `scripts/agent-setup.sh`
- `scripts/install.sh`
- `packaging/npm/scripts/install.js`

Implement:

1. During hook installation, resolve the current Aegis executable once.
2. Register hook commands using that absolute path, shell-quoted where needed.
3. Avoid `"aegis hook"` in persisted hook config unless absolute resolution is
   impossible and explicitly reported.
4. Keep idempotency: reinstall should not duplicate hooks.

Tests:

- installed Claude settings contain the expected absolute command
- installed Codex hooks contain the expected absolute command
- reinstall is stable
- paths with `@` are supported

### Phase 8 — npm postinstall best-effort hook setup

Files likely touched:

- `packaging/npm/scripts/install.js`
- `packaging/npm/README.md`
- `tests/installer_live_release.rs` or npm packaging tests

Implement:

1. After binary download and checksum verification, run:

   ```text
   vendor/aegis install-hooks --all
   ```

   only when `~/.claude` or `~/.codex` exists.

2. If no supported agent directories exist, print clear next steps:

   ```text
   If you install Claude Code or Codex later, run:
     aegis install-hooks --all
   ```

3. Do not create agent directories solely to install hooks.
4. Do not fail npm installation if hook setup is skipped because agent dirs are
   absent.
5. Do fail npm installation if the binary download/checksum fails.

### Phase 9 — Add `aegis doctor hooks`

This can be a follow-up if the core fixes are large.

Diagnostics should check:

- `aegis` binary path
- npm wrapper target
- `~/.codex/config.toml [features].hooks = true`
- `~/.codex/hooks.json` shape
- `~/.claude/settings.json` shape
- hook command paths exist and are executable
- Codex SessionStart emits valid JSON
- Codex PreToolUse rewrites a safe sample command
- missing trust/action guidance for Codex `/hooks`

Output should be actionable and avoid bypass advice.

### Phase 10 — Documentation updates

Files likely touched:

- `README.md`
- `docs/troubleshooting.md`
- `packaging/npm/README.md`
- `docs/releases/current-line.md`
- `CHANGELOG.md`

Update docs to say:

- `aegis setup-shell` is explicit opt-in shell-proxy setup.
- `aegis install-hooks --all` installs supported Claude/Codex hooks.
- Codex users may need to review/trust hooks with `/hooks`.
- Aegis does not run as a universal background daemon.
- Aegis intercepts via supported integration points:
  - `$SHELL` proxy
  - supported agent hooks
  - explicit `aegis --command`

Add troubleshooting entries for:

- npm scoped package path rejected by older versions
- Codex `SessionStart hook failed`
- hooks installed but not trusted
- `jq`/`python3` dependency issues in older hook scripts

## Final verification

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
rtk cargo audit
rtk cargo deny check
```

Focused tests:

```bash
rtk cargo test setup_shell
rtk cargo test agent_hooks
rtk cargo test installer_platform
```

Manual smoke matrix:

```bash
npm i -g @iliasalmerekov/aegis
aegis setup-shell
aegis install-hooks --all
codex
claude
```

Run smoke checks on:

- WSL2 Ubuntu
- macOS zsh
- macOS bash if supported

## Acceptance criteria

- npm scoped install path no longer breaks `aegis setup-shell`.
- `setup-shell` writes injection-safe rc blocks.
- error messages identify whether the real shell path or Aegis binary path is
  invalid.
- Codex `SessionStart` no longer emits invalid output.
- Codex `PreToolUse` rewrites supported Bash commands automatically.
- Claude/Codex hook install paths are idempotent.
- missing hook trust is documented and diagnosable.
- no new production `unwrap()` / `expect()`.
- no new dependency without explicit approval.
- no weakening of fail-closed behavior.
