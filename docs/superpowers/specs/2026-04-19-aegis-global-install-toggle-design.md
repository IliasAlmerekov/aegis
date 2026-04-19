# Aegis Global-First Install and Dynamic Full-Disable Toggle

## Objective

Redesign Aegis onboarding and runtime toggle behavior around a single default
mode:

- installation is always global
- shell integration is enabled automatically
- Claude Code and Codex hooks are installed automatically
- `aegis off` makes Aegis behave as though it is not installed
- `aegis on` restores enforcement without reinstalling hooks or shell wiring
- CI ignores the local toggle and keeps enforcement active

This spec replaces the earlier local-vs-global installer choice with a simpler
global-first product model.

## Product Decisions

The following behavior is explicitly approved:

1. **Installer is always global**
   - No Local / Global / Binary prompt.
   - The installer performs the global setup path automatically.

2. **Automatic integration**
   - The installer sets up shell integration automatically.
   - The installer also installs Claude Code and Codex hooks automatically when
     the required local files are available.

3. **Dynamic full-disable**
   - `aegis off` does not uninstall anything.
   - Instead, installed shell wiring and hooks dynamically self-disable.
   - The user experience must feel like Aegis is absent.

4. **Zero-noise disabled mode**
   - When disabled, Aegis must not emit extra informational text in ordinary
     local agent or shell usage.
   - Disabled mode should be operationally invisible.

5. **CI override remains active**
   - In CI environments, the toggle is ignored.
   - Aegis keeps enforcing normal policy even when `~/.aegis/disabled` exists.

6. **Removal remains available**
   - Users can still fully uninstall Aegis with the uninstall flow.
   - Toggle is for temporary disable / enable, not permanent removal.

## User Experience

### Install Flow

The desired install experience is:

1. user downloads and runs the installer
2. installer installs the binary
3. installer configures global shell integration
4. installer installs supported agent hooks automatically
5. installer prints the resulting state and how to:
   - disable temporarily: `aegis off`
   - re-enable: `aegis on`
   - remove completely: uninstall flow

There is no setup-mode prompt in this design.

### Disabled Experience

After `aegis off`, a user should experience the machine as if Aegis were not
installed:

- shell commands should not be intercepted
- Claude Code should not rewrite commands through `aegis`
- Codex should not deny raw Bash commands
- Codex should not inject a SessionStart routing instruction
- no extra local diagnostic text should appear

The installation remains present, but runtime behavior is effectively absent.

### Re-enable Experience

After `aegis on`:

- no reinstall is required
- shell enforcement resumes
- Claude Code hook rewrite resumes
- Codex guard behavior resumes

## Architecture

### Toggle State

Use a single global state file:

```text
~/.aegis/disabled
```

Semantics:

- file exists => Aegis is disabled
- file absent => Aegis is enabled

The file is the source of truth for both Rust runtime code and installed shell
/ hook scripts.

### Hot-Path Contract

Toggle detection must remain cheap enough for the command-interception hot path.

Requirements:

- disabled-state detection must be a single metadata existence check
- no file contents are read on the hot path
- no TOML parsing or config reload is allowed for toggle detection
- the implementation target is equivalent to one `Path::exists()` / `stat` /
  `faccessat`-style check per command boundary

This keeps the toggle compatible with the project performance budget.

### Installer Behavior

The installer becomes global-first and non-interactive with respect to mode
selection.

Installer responsibilities:

1. install `aegis` binary
2. set global shell wiring in the user shell rc file
3. install Claude Code hooks
4. install Codex hooks
5. print a concise summary including:
   - install target
   - toggle commands
   - uninstall command

If agent hook setup cannot run because the installer is not executing from a
local checkout with the required sibling hook files, the installer must fail
softly and print exact next steps rather than advertising a broken remote flow.

### Runtime Enforcement Model

Dynamic full-disable requires every integration point to consult the same toggle
state.

#### 1. Shell wrapper

When disabled and not in CI:

- `run_shell_wrapper` must short-circuit before planning / scanning
- the command is passed directly to the real shell
- no user-facing disabled message is printed in normal operation
- the toggle snapshot is taken immediately before the first enforcement-related
  I/O in `run_shell_wrapper`

When enabled:

- current behavior remains

When in CI:

- the disabled file is ignored
- normal planning / enforcement proceeds

Race semantics:

- each command invocation snapshots the toggle state once at the start of the
  wrapper path
- `aegis off` is guaranteed to affect subsequent commands, not commands already
  past the gate
- `aegis on` is likewise guaranteed to affect subsequent commands, not commands
  already committed to disabled passthrough

This is acceptable and should be documented as command-boundary behavior rather
than instantaneous global cancellation.

#### 2. Claude Code hook

When disabled and not in CI:

- the hook exits as a silent no-op
- it does not rewrite `tool_input.command`
- it does not emit warning text
- the toggle snapshot is taken immediately before building the hook JSON response

When enabled:

- existing rewrite behavior remains

When in CI:

- disabled state is ignored and normal Aegis routing remains active

#### 3. Codex `PreToolUse` hook

When disabled and not in CI:

- the hook exits as a silent no-op
- it does not deny raw Bash commands
- it does not require `aegis --command ...`
- the toggle snapshot is taken immediately before building the hook JSON response

When enabled:

- existing deny / reroute behavior remains

When in CI:

- disabled state is ignored and normal enforcement remains active

#### 4. Codex `SessionStart` hook

When disabled and not in CI:

- the hook emits no Aegis routing context
- it behaves as a silent no-op
- the toggle snapshot is taken immediately before building the hook JSON response

When enabled:

- existing SessionStart context remains

When in CI:

- disabled state is ignored and normal enforcement remains active

## Commands

The CLI surface remains:

- `aegis off`
- `aegis on`
- `aegis status`

### `aegis off`

Behavior:

1. create or update `~/.aegis/disabled`
2. make the operation idempotent
3. record an audit entry for the toggle event
4. print a short success message

Suggested user-facing text:

```text
Aegis disabled.
```

No claim should imply uninstall; this is a runtime disable.

### `aegis on`

Behavior:

1. remove `~/.aegis/disabled` if present
2. make the operation idempotent
3. record an audit entry for the toggle event
4. print a short success message

Suggested user-facing text:

```text
Aegis enabled.
```

### `aegis status`

Behavior:

1. report enabled vs disabled
2. report that CI overrides the toggle when relevant to documentation / status
3. show the flag-file path for clarity

Suggested text should stay short and unambiguous.

Exit code:

- `aegis status` returns `0` in both enabled and disabled states
- state is communicated via stdout / structured output, not via non-zero exit
  codes

## Toggle Detection API

We need one reusable toggle-state resolver shared across Rust code and hook
scripts.

### Rust side

Provide helpers under a focused module, e.g. `src/toggle.rs`:

- `disabled_flag_path() -> PathBuf`
- `is_disabled() -> Result<bool>`
- `set_disabled(bool) -> Result<()>`
- `status() -> Result<ToggleState>`

`set_disabled(bool)` is best-effort and command-oriented, not a multi-process
transaction primitive:

- it does not promise atomic cross-process on/off serialization
- concurrent `on` / `off` races are resolved by last-writer-wins behavior
- this is acceptable because toggle semantics are defined at command boundaries,
  not as a globally linearizable control plane

### Script side

Provide a tiny portable detection rule for shell scripts:

- check `AEGIS_CI` / CI vars first
- if CI is active, behave as enabled
- otherwise check for `"$HOME/.aegis/disabled"`

The shell-side logic must stay portable and fast; no heavy subprocesses for the
common case.

The CI detection contract must exactly match Rust behavior. Use this precedence:

1. `AEGIS_CI=0`, `AEGIS_CI=false`, or `AEGIS_CI=no` => force non-CI
2. `AEGIS_CI=1`, `AEGIS_CI=true`, or `AEGIS_CI=yes` => force CI
3. truthy value for one of:
   - `CI`
   - `GITHUB_ACTIONS`
   - `GITLAB_CI`
   - `CIRCLECI`
   - `BUILDKITE`
   - `TRAVIS`
   - `TF_BUILD`
4. non-empty `JENKINS_URL` => CI

Rust runtime code and shell hooks must share the same list and truthiness
semantics.

Truthiness rules:

- for `AEGIS_CI`, `CI`, `GITHUB_ACTIONS`, `GITLAB_CI`, `CIRCLECI`, `BUILDKITE`,
  `TRAVIS`, and `TF_BUILD`, accepted true values are `{1, true, yes}`
  case-insensitively
- accepted false values for the explicit `AEGIS_CI` override are
  `{0, false, no}` case-insensitively
- `JENKINS_URL` is considered CI when it is a non-empty string

To avoid drift, the shell side should use one shared helper implementation for
toggle / CI detection rather than duplicating the logic separately in every
hook.

That helper should live in a stable, installer-managed location that uninstall
also knows how to remove. The exact final path is an implementation detail, but
the spec requires:

- one installed helper source of truth for shell-side CI / disabled detection
- all installed hooks consume that helper rather than embedding divergent logic
- uninstall removes the helper together with the installed hook payloads

## Audit and Observability

Toggle actions are user-meaningful security events and should be audited.

Requirements:

- `aegis off` writes an audit record
- `aegis on` writes an audit record
- the audit trail should distinguish toggle events from ordinary command
  decisions

The exact representation can be:

- a dedicated event kind, or
- an intentionally structured reuse of an existing audit schema

The important requirement is that the actions are queryable and not silently
lost.

Disabled runtime passthrough commands do **not** need extra disabled-mode noise
just to prove the toggle worked; the toggle commands themselves are the primary
observable events.

This spec intentionally does **not** require one audit entry per disabled
passthrough command. The reason is product-driven:

- disabled mode should feel absent
- per-command disabled logging would reintroduce noise and a second behavioral
  mode for ordinary usage
- the primary forensic signal is the explicit `on` / `off` toggle history

If deeper disabled-mode telemetry is later needed, it should be introduced as a
separate follow-up with explicit product approval.

## Testing

### Installer tests

Cover:

- installer no longer asks Local / Global / Binary
- installer performs global shell setup automatically
- installer attempts automatic agent hook installation from local checkout
- installer prints correct fallback guidance when local hook payloads are
  unavailable

### Toggle command tests

Cover:

- `aegis off` creates the disabled file
- `aegis on` removes it
- both commands are idempotent
- `aegis status` reports correct state
- `aegis status` reports both local disabled state and effective CI-enforced
  behavior when CI override is active

### Shell wrapper tests

Cover:

- disabled + non-CI => passthrough without scanner / planner enforcement
- enabled => existing enforcement still works
- disabled + CI => toggle ignored

### Hook tests

Cover:

- Claude hook does nothing when disabled
- Codex `PreToolUse` does nothing when disabled
- Codex `SessionStart` emits no routing context when disabled
- all three resume enforcement behavior when re-enabled
- disabled state is ignored in CI for all hooks

### Known Limitation

Codex `SessionStart` context is injected at session creation time. If a user
runs `aegis off` in the middle of an already-running Codex session, the
previously injected SessionStart context may still remain in the agent's
conversation memory.

This is acceptable for the initial implementation because:

- runtime behavior still changes immediately for future hook invocations
- `PreToolUse` and shell wrapper behavior control actual enforcement
- a fresh agent session reflects the new disabled state cleanly

The spec therefore guarantees full-disable semantics at runtime, while treating
already-issued SessionStart guidance as a known session-lifecycle limitation.

### Regression tests

Preserve:

- current hook idempotency
- current valid `SessionStart` JSON
- current strict `PreToolUse` wrapper validation when enabled

## Error Handling

### Toggle commands

- failure to create / remove the disabled file must be explicit
- do not silently pretend the toggle succeeded

### Installer

- failure to install hooks should not claim success for those hooks
- shell setup and hook setup outcomes should be reported honestly

### Disabled-mode hook behavior

In local non-CI disabled mode, hooks should be silent no-ops.

That silent behavior applies only to the deliberate disabled case, not to
unexpected internal failures. Actual hook failures should still be handled
honestly and fail in the manner expected by the specific integration.

## Security and Scope Notes

- This toggle is **global**, not per-project.
- This is a runtime control, not an uninstall mechanism.
- CI remains authoritative and cannot be disabled by the local toggle.
- Dynamic full-disable is chosen over config rewriting because it is safer and
  more reversible.

## Out of Scope

- TTL-based disable (`aegis off --for 30m`)
- per-project toggle files
- mutating user shell / agent config on every `on` / `off`
- new installer mode prompts
- non-global install modes in the main onboarding flow

## Implementation Direction

Recommended sequence:

1. land the toggle-state module and CLI commands
2. add shell-wrapper dynamic full-disable behavior
3. update Claude and Codex hooks to honor disabled state silently
4. simplify installer flow to always-global + automatic hook setup
5. add regression coverage for enabled / disabled / CI combinations

Steps 2–4 are loosely parallelizable once the toggle-state contract is fixed,
but the implementation should preserve a single shared definition of CI and
disabled-state detection across Rust and shell code.
