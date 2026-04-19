# Global Install and Dynamic Full-Disable Toggle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Aegis install globally by default, install shell + Claude/Codex integrations automatically, and implement `aegis on` / `aegis off` as a dynamic full-disable that feels like Aegis is absent outside CI.

**Architecture:** Keep one global toggle source of truth at `~/.aegis/disabled`, but make every enforcement point consult that state consistently. Rust runtime code owns the CLI toggle commands and shell-wrapper short-circuit; installed shell hooks reuse one shared helper so Claude/Codex behavior matches Rust semantics exactly. Installer scripts become global-first and non-interactive about mode selection.

**Tech Stack:** Rust 2024, clap, tokio, existing audit/config modules, POSIX shell scripts, jq-based JSON editing, Cargo integration tests, shell installer tests.

---

## File Structure

- `src/runtime_gate.rs` — single Rust source of truth for CI override semantics.
- `src/toggle.rs` — disabled-flag path, enable/disable/status helpers, audit helper, config status helper.
- `src/main.rs` — CLI dispatch, shell-wrapper runtime gate, `status` output behavior.
- `src/watch.rs` — watch-mode disabled passthrough gate, aligned with shell-wrapper semantics.
- `scripts/hooks/toggle-state.sh` — source template for the shared shell helper.
- `~/.aegis/lib/toggle-state.sh` — single installed shell helper path used by all installed hooks.
- `scripts/hooks/claude-code.sh` — rewrite only when enabled outside CI; silent no-op when disabled.
- `scripts/hooks/codex-pre-tool-use.sh` — deny only when enabled outside CI; silent no-op when disabled.
- `scripts/hooks/codex-session-start.sh` — emit SessionStart context only when enabled outside CI.
- `scripts/agent-setup.sh` — install shared helper together with hook payloads.
- `scripts/uninstall.sh` — remove binary, shell wiring, installed hooks, and shared helper.
- `scripts/install.sh` — always-global install flow, automatic hook setup, no mode prompt.
- `tests/agent_hooks.rs` — regression coverage for enabled/disabled/CI hook behavior.
- `tests/installer_flow.rs` — installer flow expectations for always-global behavior.
- `tests/toggle_cli.rs` — new CLI integration coverage for `aegis on/off/status`.
- `README.md` — public-facing install/toggle behavior summary.

## Task Graph

1. Rust gate contract (`src/runtime_gate.rs`, `src/toggle.rs`, `src/main.rs`, `src/watch.rs`)
2. Hook-side dynamic full-disable (`scripts/hooks/*`, `scripts/agent-setup.sh`, `tests/agent_hooks.rs`)
3. Always-global installer flow (`scripts/install.sh`, `tests/installer_flow.rs`, `README.md`)
4. End-to-end verification (`tests/toggle_cli.rs`, targeted Cargo tests, final verification commands)

Tasks 1–3 can overlap once the CI/toggle contract is fixed, but Task 4 should run after the code and scripts are settled.

## Task Details

### Task 1: Finalize Rust-side toggle and CI gate contract

**Files:**
- Modify: `src/runtime_gate.rs`
- Modify: `src/toggle.rs`
- Modify: `src/main.rs`
- Modify: `src/watch.rs`
- Modify: `src/lib.rs`
- Test: `tests/toggle_cli.rs`

- [ ] **Step 1: Write the failing CLI integration tests for `on`, `off`, and `status`**

Create `tests/toggle_cli.rs` with these first assertions:

```rust
use std::fs;
use std::process::Command;

use tempfile::TempDir;

fn aegis_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aegis")
}

#[test]
fn off_creates_disabled_flag_and_status_reports_disabled() {
    let home = TempDir::new().unwrap();
    let output = Command::new(aegis_bin())
        .env("HOME", home.path())
        .args(["off"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(home.path().join(".aegis").join("disabled").exists());

    let status = Command::new(aegis_bin())
        .env("HOME", home.path())
        .args(["status"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(status.stdout).unwrap();
    assert!(stdout.contains("toggle: disabled"));
}

#[test]
fn status_reports_disabled_but_ci_override_active() {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(home.path().join(".aegis").join("disabled"), "timestamp=x\npid=1\n").unwrap();

    let status = Command::new(aegis_bin())
        .env("HOME", home.path())
        .env("CI", "true")
        .args(["status"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(status.stdout).unwrap();
    assert_eq!(status.status.code(), Some(0));
    assert!(stdout.contains("toggle: disabled"));
    assert!(stdout.contains("effective mode: enforcing (CI override)"));
}

#[test]
fn status_returns_zero_when_enabled_or_disabled() {
    let home = TempDir::new().unwrap();

    let enabled = Command::new(aegis_bin())
        .env("HOME", home.path())
        .args(["status"])
        .output()
        .unwrap();
    assert_eq!(enabled.status.code(), Some(0));

    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(home.path().join(".aegis").join("disabled"), "timestamp=x\npid=1\n").unwrap();

    let disabled = Command::new(aegis_bin())
        .env("HOME", home.path())
        .args(["status"])
        .output()
        .unwrap();
    assert_eq!(disabled.status.code(), Some(0));
}
```

- [ ] **Step 2: Run the new CLI tests to verify they fail**

Run:

```bash
rtk cargo test --test toggle_cli
```

Expected: FAIL because `tests/toggle_cli.rs` does not exist yet and/or `status` output does not yet match the new contract.

- [ ] **Step 3: Tighten `src/runtime_gate.rs` into the spec-approved CI override contract**

Replace the simple helper with explicit truthiness parsing:

```rust
fn truthy_env(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
}

fn falsy_env(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "no")
}

pub fn is_ci_environment() -> bool {
    if let Ok(value) = env::var("AEGIS_CI") {
        if falsy_env(&value) {
            return false;
        }
        if truthy_env(&value) {
            return true;
        }
    }

    for key in ["CI", "GITHUB_ACTIONS", "GITLAB_CI", "CIRCLECI", "BUILDKITE", "TRAVIS", "TF_BUILD"] {
        if let Ok(value) = env::var(key) {
            if truthy_env(&value) {
                return true;
            }
        }
    }

    env::var("JENKINS_URL")
        .ok()
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}
```

- [ ] **Step 4: Make `src/toggle.rs` match the approved command/status semantics**

Adjust the module to keep best-effort last-write-wins behavior and expose richer status text helpers:

```rust
pub struct ToggleStatusView {
    pub state: ToggleState,
    pub flag_path: PathBuf,
    pub config_status: String,
    pub ci_override_active: bool,
}

pub fn status_view(ci_override_active: bool) -> Result<ToggleStatusView> {
    Ok(ToggleStatusView {
        state: status()?,
        flag_path: disabled_flag_path()?,
        config_status: config_status()?,
        ci_override_active,
    })
}
```

Keep `disable()` / `enable()` as filesystem operations without pretending to be cross-process atomic. Do **not** add file-content parsing to the hot path.

- [ ] **Step 5: Update `src/main.rs` and `src/watch.rs` to use the shared Rust gate with zero-noise passthrough**

In `src/main.rs`, switch from the local CI helper to `aegis::runtime_gate::is_ci_environment`, remove the verbose disabled stderr notice, add an inline comment that the toggle snapshot point is intentional, and make `status` print the new lines:

```rust
// Snapshot toggle state exactly once before any enforcement-related I/O.
println!("toggle: {toggle_label}");
println!("flag: {}", view.flag_path.display());
if view.ci_override_active {
    println!("effective mode: enforcing (CI override)");
} else {
    println!("effective mode: {}", if matches!(view.state, toggle::ToggleState::Disabled) {
        "disabled passthrough"
    } else {
        "enforcing"
    });
}
println!("config: {}", view.config_status);
```

In `src/watch.rs`, mirror the shell-wrapper behavior and add the same snapshot-point comment:

```rust
// Snapshot toggle state exactly once at the command-boundary gate.
if !ci_detected {
    match crate::toggle::status() {
        Ok(crate::toggle::ToggleState::Disabled) => return run_disabled().await,
        Ok(crate::toggle::ToggleState::Enabled) => {}
        Err(err) => {
            eprintln!("error: failed to read toggle state: {err}");
            return 4;
        }
    }
}
```

- [ ] **Step 6: Run the focused tests until they pass**

Run:

```bash
rtk cargo test --test toggle_cli
rtk cargo test toggle:: --lib
```

Expected: PASS for the new CLI tests and the `src/toggle.rs` unit tests.

- [ ] **Step 7: Commit Task 1**

```bash
rtk git add src/runtime_gate.rs src/toggle.rs src/main.rs src/watch.rs src/lib.rs tests/toggle_cli.rs
rtk git commit -m "feat: add dynamic toggle runtime gate"
```

### Task 2: Make Claude and Codex hooks honor disabled mode silently

**Files:**
- Create: `scripts/hooks/toggle-state.sh`
- Modify: `scripts/hooks/claude-code.sh`
- Modify: `scripts/hooks/codex-pre-tool-use.sh`
- Modify: `scripts/hooks/codex-session-start.sh`
- Modify: `scripts/agent-setup.sh`
- Modify: `scripts/uninstall.sh`
- Test: `tests/agent_hooks.rs`

- [ ] **Step 1: Write failing hook tests for disabled-mode no-op and CI override**

Extend `tests/agent_hooks.rs` with these cases:

```rust
#[test]
fn codex_pre_tool_use_is_noop_when_disabled_outside_ci() {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(home.path().join(".aegis").join("disabled"), "timestamp=x\npid=1\n").unwrap();

    let output = run_codex_pre_tool_use(home.path(), "echo hi");
    assert!(output.status.success());
    assert!(output.stdout.is_empty(), "disabled hook must be silent noop");
    assert!(output.stderr.is_empty(), "disabled hook must be silent noop");
}

#[test]
fn codex_session_start_is_noop_when_disabled_outside_ci() {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(home.path().join(".aegis").join("disabled"), "timestamp=x\npid=1\n").unwrap();

    let output = run_script("hooks/codex-session-start.sh", home.path(), &[], None);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}
```

- [ ] **Step 2: Run the hook tests to verify they fail**

Run:

```bash
rtk cargo test --test agent_hooks
```

Expected: FAIL because the current hooks still enforce when disabled and there is no shared toggle helper yet.

- [ ] **Step 3: Add one shared shell helper for CI + disabled detection**

Create `scripts/hooks/toggle-state.sh` as the source template for the installed helper. The installed destination must be a **single shared path**:

```text
${HOME}/.aegis/lib/toggle-state.sh
```

The hook payloads installed into `~/.claude/hooks/` and `~/.codex/hooks/` must source that single shared helper path rather than keeping per-directory copies.

Template:

```sh
#!/usr/bin/env sh

aegis_truthy() {
  case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
    1|true|yes) return 0 ;;
    *) return 1 ;;
  esac
}

aegis_falsy() {
  case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
    0|false|no) return 0 ;;
    *) return 1 ;;
  esac
}

aegis_ci_active() {
  if [ -n "${AEGIS_CI:-}" ]; then
    aegis_falsy "${AEGIS_CI}" && return 1
    aegis_truthy "${AEGIS_CI}" && return 0
  fi

  for key in CI GITHUB_ACTIONS GITLAB_CI CIRCLECI BUILDKITE TRAVIS TF_BUILD; do
    value="$(printenv "$key" 2>/dev/null || true)"
    if [ -n "${value}" ] && aegis_truthy "${value}"; then
      return 0
    fi
  done

  [ -n "${JENKINS_URL:-}" ]
}

aegis_disabled_locally() {
  [ -f "${HOME}/.aegis/disabled" ]
}

aegis_enforcement_enabled() {
  if aegis_ci_active; then
    return 0
  fi

  if aegis_disabled_locally; then
    return 1
  fi

  return 0
}
```

When reading environment variables by key, do **not** use `eval`. Use a safe pattern such as:

```sh
value="$(printenv "$key" 2>/dev/null || true)"
```

- [ ] **Step 4: Source the helper from all installed hooks and make disabled mode a silent no-op**

At the top of each hook, source the one installed helper path:

```sh
AEGIS_TOGGLE_HELPER="${HOME}/.aegis/lib/toggle-state.sh"
[ -r "${AEGIS_TOGGLE_HELPER}" ] || exit 0
. "${AEGIS_TOGGLE_HELPER}"

if ! aegis_enforcement_enabled; then
  exit 0
fi
```

For `scripts/hooks/claude-code.sh`, preserve existing warnings only for genuine missing-dependency cases when enforcement is enabled. For `scripts/hooks/codex-session-start.sh`, keep the current JSON body but emit nothing when disabled.

- [ ] **Step 5: Install the helper together with the hooks**

Update `scripts/agent-setup.sh` so it installs the helper once into the shared Aegis-managed path:

```sh
COMMON_TOGGLE_SOURCE="${HOOKS_DIR}/toggle-state.sh"
COMMON_TOGGLE_DEST="${HOME}/.aegis/lib/toggle-state.sh"

need_file "${COMMON_TOGGLE_SOURCE}"
mkdir -p "$(dirname "${COMMON_TOGGLE_DEST}")"
install -m 0755 "${COMMON_TOGGLE_SOURCE}" "${COMMON_TOGGLE_DEST}"
```

Also update `scripts/uninstall.sh` so full uninstall explicitly removes:

- installed Claude hook payloads
- installed Codex hook payloads
- the shared helper at `~/.aegis/lib/toggle-state.sh`

No conditional “if already managed” wording — uninstall must either clean these paths or fail honestly.

- [ ] **Step 6: Expand hook tests to cover CI override**

Add one explicit CI-override test:

```rust
#[test]
fn codex_pre_tool_use_still_denies_when_disabled_file_exists_but_ci_override_is_forced() {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(home.path().join(".aegis").join("disabled"), "timestamp=x\npid=1\n").unwrap();

    let input = serde_json::json!({ "tool_input": { "command": "echo hi" } }).to_string();
    let output = run_script(
        "hooks/codex-pre-tool-use.sh",
        home.path(),
        &[],
        Some(input.as_str()),
    );

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
}
```

Run it with `AEGIS_CI=1` in the helper-aware test harness.

- [ ] **Step 7: Run hook tests until they pass**

Run:

```bash
rtk cargo test --test agent_hooks
```

Expected: PASS, including enabled-mode regressions and disabled/CI behaviors.

- [ ] **Step 8: Add an explicit agent-setup idempotency assertion for the shared helper path**

Extend `tests/agent_hooks.rs` so a second `agent-setup.sh` run keeps the helper stable:

```rust
let helper = home.path().join(".aegis").join("lib").join("toggle-state.sh");
let before = fs::read_to_string(&helper).unwrap();
let second_output = run_script("agent-setup.sh", home.path(), &["--all"], None);
assert!(second_output.status.success());
assert_eq!(fs::read_to_string(&helper).unwrap(), before);
```

- [ ] **Step 9: Commit Task 2**

```bash
rtk git add scripts/hooks/toggle-state.sh scripts/hooks/claude-code.sh scripts/hooks/codex-pre-tool-use.sh scripts/hooks/codex-session-start.sh scripts/agent-setup.sh scripts/uninstall.sh tests/agent_hooks.rs
rtk git commit -m "feat: add dynamic full-disable hook gating"
```

### Task 3: Make the installer always-global and auto-run hook setup

**Files:**
- Modify: `scripts/install.sh`
- Modify: `tests/installer_flow.rs`
- Modify: `README.md`

- [ ] **Step 1: Write failing installer-flow assertions for the new global-first UX**

Update `tests/installer_flow.rs` with a case that no longer expects the setup-mode menu and explicitly preserves idempotent auto-hook setup:

```rust
assert!(
    !stdout.contains("How would you like to set up Aegis?"),
    "global-first installer must not prompt for Local/Global/Binary; stdout=\n{stdout}"
);
assert!(
    stdout.contains("Aegis installed globally."),
    "installer should confirm the global default; stdout=\n{stdout}"
);
assert!(
    stdout.contains("Use `aegis off` to disable temporarily."),
    "installer should advertise the new toggle flow; stdout=\n{stdout}"
);
assert!(
    stdout.contains("Agent hooks installed automatically.")
        || stdout.contains("Agent hook setup is only available from a local checkout"),
    "installer must either auto-install hooks or print honest local-checkout guidance; stdout=\n{stdout}"
);
```

- [ ] **Step 2: Run the installer tests to verify they fail**

Run:

```bash
rtk cargo test --test installer_flow
```

Expected: FAIL because the current script still calls `prompt_setup_mode` and branches on `global/local/binary`.

- [ ] **Step 3: Simplify `scripts/install.sh` to a single global path**

Delete the setup-mode prompt flow and inline the global path inside `main()`:

```sh
real_shell="$(detect_real_shell)"
rc_file="$(resolve_rc_file "${real_shell}")"
write_shell_setup "${rc_file}" "${real_shell}" "$(target_path)"
print_post_install "${rc_file}"
offer_agent_setup
printf 'Aegis installed globally.\n'
printf 'Use `aegis off` to disable temporarily.\n'
printf 'Use `aegis on` to re-enable enforcement.\n'
```

Remove the now-unused `prompt_setup_mode`, `setup_local_project`, `enter_local_shell`, and `print_local_post_install` helpers only if no remaining tests or paths require them. If those helpers are still referenced elsewhere, delete those references in the same task.

- [ ] **Step 4: Make `offer_agent_setup` automatic instead of interactive**

Change it from a yes/no prompt into an auto-attempt with honest fallback:

```sh
offer_agent_setup() {
    agent_setup_script=""

    if agent_setup_script="$(resolve_local_agent_setup)"; then
        if /bin/sh "${agent_setup_script}"; then
            printf 'Agent hooks installed automatically.\n'
        else
            printf 'Agent hook setup failed.\n'
            print_agent_setup_next_steps
        fi
    else
        print_agent_setup_next_steps
    fi
}
```

This keeps the “two-click” product behavior while avoiding a broken remote path.

- [ ] **Step 5: Update README install/toggle documentation**

Add a short user-facing section like:

```md
## Install behavior

The installer performs a global setup by default:

- installs the `aegis` binary
- enables shell integration
- installs Claude Code / Codex hooks when available from a local checkout

Use `aegis off` to temporarily disable enforcement and `aegis on` to restore it.
```

- [ ] **Step 6: Run installer tests until they pass**

Run:

```bash
rtk cargo test --test installer_flow
```

Expected: PASS with the new global-first flow.

- [ ] **Step 7: Commit Task 3**

```bash
rtk git add scripts/install.sh tests/installer_flow.rs README.md
rtk git commit -m "feat: make installer global by default"
```

### Task 4: Run full verification and capture the final behavior contract

**Files:**
- Modify: `tests/toggle_cli.rs`
- Modify: `README.md`

- [ ] **Step 1: Add one regression for zero-noise disabled shell-wrapper behavior**

Extend `tests/toggle_cli.rs` with a direct shell-wrapper passthrough check:

```rust
#[test]
fn disabled_shell_wrapper_passthrough_stays_quiet() {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(home.path().join(".aegis").join("disabled"), "timestamp=x\npid=1\n").unwrap();

    let output = Command::new(aegis_bin())
        .env("HOME", home.path())
        .env("AEGIS_REAL_SHELL", "/bin/sh")
        .args(["--command", "printf test"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "test");
    assert!(output.stderr.is_empty(), "disabled mode should stay quiet");
}
```

- [ ] **Step 2: Run the focused regression suites**

Run:

```bash
rtk cargo test --test toggle_cli
rtk cargo test --test agent_hooks
rtk cargo test --test installer_flow
```

Expected: all three suites PASS.

- [ ] **Step 3: Run repository verification for touched areas**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
```

Expected: all commands exit 0.

- [ ] **Step 4: Benchmark the hot path**

Capture a before/after benchmark number for the safe path instead of relying on a subjective read.

Run before the implementation branch is rebased away, then after the implementation is complete:

```bash
rtk sh -lc 'rtk cargo bench --bench scanner_bench | tee /tmp/aegis-toggle-bench-before.txt'
rtk sh -lc 'rtk cargo bench --bench scanner_bench | tee /tmp/aegis-toggle-bench-after.txt'
```

Expected: benchmark completes successfully, and the safe-path benchmark regression stays within an agreed bound (target: no more than +10% vs the captured before-number). Save the before/after output files or paste the compared figures into the final task report.

- [ ] **Step 5: Update README one last time if verification reveals wording drift**

If needed, tighten the wording to match the actual runtime behavior:

```md
When disabled, Aegis behaves as though it is not installed for ordinary local shell and supported agent usage. CI ignores the local disabled flag and continues enforcing policy.
```

- [ ] **Step 6: Commit Task 4**

```bash
rtk git add tests/toggle_cli.rs README.md
rtk git commit -m "test: verify global toggle and installer flow"
```

## Verification Plan

- Targeted red/green loops:
  - `rtk cargo test --test toggle_cli`
  - `rtk cargo test --test agent_hooks`
  - `rtk cargo test --test installer_flow`
- Broad verification:
  - `rtk cargo fmt --check`
  - `rtk cargo clippy -- -D warnings`
  - `rtk cargo test`
  - `rtk cargo bench --bench scanner_bench`

## Rollback Plan

- Revert the task commits in reverse order if behavior regresses:
  1. verification/docs commit
  2. installer global-default commit
  3. hook gating commit
  4. runtime gate commit
- If script-hook behavior regresses independently, revert only the hook-gating commit and keep the Rust toggle commands intact.
- If the installer UX regresses, revert only the installer commit and retain the toggle runtime behavior.

## Self-Review

**Spec coverage:**
- Global-first installer flow: covered by Task 3
- Automatic hook installation and honest fallback: covered by Task 3
- Dynamic full-disable in Rust runtime: covered by Task 1
- Dynamic full-disable in Claude/Codex hooks: covered by Task 2
- Zero-noise local disabled mode: covered by Tasks 1, 2, and 4
- CI override: covered by Tasks 1, 2, and 4
- `status` behavior and exit code: covered by Task 1
- Shared shell helper requirement: covered by Task 2

**Placeholder scan:** none; every task has explicit files, code snippets, commands, and expected outcomes.

**Type consistency:**
- Rust uses `runtime_gate::is_ci_environment()` and `toggle::ToggleState`
- Shell hooks use `aegis_enforcement_enabled()` from `scripts/hooks/toggle-state.sh`
- CLI contract stays `aegis on` / `aegis off` / `aegis status`

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-04-19-aegis-on-off-toggle.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**
