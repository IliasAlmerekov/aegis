# Binary-First Agent Hook Installation Implementation Plan

Date: 2026-04-22
Status: Drafted after approved design
Design input: `docs/superpowers/specs/2026-04-22-binary-first-hook-install-design.md`

## Milestones

### Milestone 1 — CLI contract and installer orchestration

Add an explicit binary-side hook-installation entrypoint and route installer auto-setup through the installed binary instead of local checkout scripts.

### Milestone 2 — Compatibility and edge-case handling

Preserve existing installation flows, keep behavior idempotent, and ensure the installer reports skip/error states honestly.

### Milestone 3 — Documentation and regression protection

Update user-facing docs and extend tests so release installs, local installs, repeated installs, and skip cases all remain covered.

## Task Graph

1. **Define the binary-side CLI contract**
   - Add or expose `install-hooks` as the preferred public entrypoint
   - Preserve `install` compatibility behavior
2. **Refactor install dispatch**
   - Centralize hook-target selection and per-agent result reporting
3. **Update release installer**
   - Replace local-checkout-only hook auto-install path with an absolute-path binary invocation
4. **Adapt compatibility wrapper**
   - Keep `scripts/agent-setup.sh` working, ideally as a thin wrapper
5. **Expand tests**
   - Unit coverage for selection logic and outcomes
   - Integration coverage for installer flow and idempotence
6. **Update docs**
   - README, troubleshooting, installer messages, and changelog/release notes

Dependencies:

- Task 2 depends on Task 1
- Task 3 depends on Task 2
- Task 4 depends on Task 2
- Task 5 depends on Tasks 2 and 3
- Task 6 depends on Tasks 2 and 3 so docs match final behavior

## Task Details

### Task 1 — Define the CLI contract

**Owner:** coder

**Scope**

- Extend the CLI so users can invoke hook installation explicitly with a clear name
- Keep backwards compatibility with existing `aegis install`
- Support target selection:
  - `--all`
  - `--claude-code`
  - `--codex`

**Likely files**

- `src/main.rs`
- `src/cli_dispatch.rs`
- `src/install.rs`

**Implementation notes**

- Prefer routing both `install` and `install-hooks` into the same implementation path
- If `--local` remains, document and keep its scope narrow to Claude local settings behavior
- Keep `src/main.rs` thin by pushing argument interpretation into `src/install.rs` or adjacent dispatch glue

**Verification**

- CLI parsing tests for the new public entrypoint
- Existing install command behavior still works

**Rollback**

- Revert the new entrypoint while preserving current `install` behavior

### Task 2 — Centralize binary-side install behavior

**Owner:** coder

**Scope**

- Refine `src/install.rs` into the canonical installation path for:
  - Claude Code hook registration
  - Codex hook file materialization
  - Codex hooks.json registration
- Make per-agent outcomes explicit:
  - Installed
  - AlreadyPresent
  - Skipped
  - Error

**Likely files**

- `src/install.rs`

**Implementation notes**

- Reuse the existing include-str hook payload model for Codex
- Preserve current Claude hook rewrite behavior and Codex SessionStart/PreToolUse semantics
- Avoid silent success when a detected agent config is malformed
- Keep idempotence guarantees intact

**Verification**

- Unit tests for:
  - missing agent directories -> skipped
  - malformed JSON -> error
  - repeated install -> already present / no duplicates
  - per-target selection behavior

**Rollback**

- Revert to the current install flow if target selection or result reporting introduces regressions

### Task 3 — Update `scripts/install.sh`

**Owner:** coder

**Scope**

- Remove the local-checkout-only auto-install assumption from the main release installer
- Invoke the installed binary directly using its absolute path

**Likely files**

- `scripts/install.sh`

**Implementation notes**

- Use the resolved install target path rather than relying on PATH refresh in the current shell
- Preserve current shell-wrapper installation flow
- Print honest post-install messaging:
  - hooks installed automatically
  - no supported agent directories detected
  - hook setup failed, with actionable next step

**Verification**

- Installer integration tests for:
  - no agent directories
  - Codex only
  - Claude only
  - both present
  - repeated install

**Rollback**

- Restore the previous installer messaging and local-checkout guidance if binary invocation proves unstable

### Task 4 — Preserve compatibility wrapper behavior

**Owner:** coder

**Scope**

- Keep `scripts/agent-setup.sh` usable for existing workflows
- Where practical, reduce it to a thin wrapper around the binary command

**Likely files**

- `scripts/agent-setup.sh`
- possibly `README.md` references

**Implementation notes**

- Avoid introducing a second source of truth for hook installation rules
- Preserve supported flags if existing tests or docs rely on them

**Verification**

- Existing agent-setup tests continue to pass or are updated to validate equivalent behavior

**Rollback**

- Keep the previous script implementation if wrapper conversion creates brittle behavior

### Task 5 — Extend regression coverage

**Owner:** tester

**Scope**

- Add or update tests for:
  - CLI parsing of the new command name
  - per-target selection
  - skipped installs when directories are absent
  - malformed JSON failures
  - idempotent repeated installation
  - installer auto-attempt behavior through the installed binary

**Likely files**

- `src/main.rs` tests
- `src/install.rs` tests
- `tests/installer_flow.rs`
- `tests/agent_hooks.rs`

**Verification**

- Relevant tests pass for touched areas

**Rollback**

- Revert only newly added regression tests if they prove to be asserting the wrong contract during iteration

### Task 6 — Update documentation

**Owner:** coder

**Scope**

- Update docs to reflect binary-first hook setup
- Replace local-checkout-only guidance in release-install paths

**Likely files**

- `README.md`
- `docs/troubleshooting.md`
- `CHANGELOG.md`
- possibly `docs/releases/current-line.md`
- `scripts/install.sh` post-install output text

**Implementation notes**

- Clearly document:
  - auto-attempt during install
  - skip behavior when agent directories do not exist
  - follow-up command: `aegis install-hooks --all`
- Keep claims aligned with actual tested behavior

**Verification**

- Docs contract tests still pass
- Text matches the final CLI contract

**Rollback**

- Revert doc-only changes independently if implementation wording changes during review

## Verification Plan

Run the relevant validation for touched areas via `rtk`:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
```

Additionally, target the existing installer and hook coverage if iteration speed matters:

```bash
rtk cargo test --test installer_flow
rtk cargo test --test agent_hooks
```

If command-parsing tests or contracts materially change, include the relevant focused test runs as part of the implementation notes.

## Rollback Plan

If the rollout introduces regressions:

1. revert installer-script changes first to restore the last known good release-install path
2. keep binary-side compatibility for `aegis install` if possible
3. revert new CLI aliasing separately if parsing or help text causes user confusion
4. revert doc updates last, after code behavior is restored

This staged rollback preserves the current shipping behavior while allowing the binary-first path to be introduced incrementally.

## Risks

- CLI naming changes may affect existing tests or user expectations if aliasing is not handled carefully
- installer-script changes must not regress shell-wrapper setup
- malformed existing agent configs must fail honestly without producing partial success messaging
- compatibility behavior in `scripts/agent-setup.sh` may be trickier than expected if current tests depend on exact script output

## Confirmation

Approved design source:

- `docs/superpowers/specs/2026-04-22-binary-first-hook-install-design.md`

Implementation should begin only against the plan above, with reviewer and security review attention on:

- `src/main.rs`
- `src/install.rs`
- installer messaging and release-install behavior
