# M4.1 Remove Native Windows Scope Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Align the repository with PRD §5.5/§8 by making native Windows explicitly unsupported for Aegis 1.0 while preserving Linux, macOS, and WSL2-as-Linux support.

**Architecture:** Remove the Windows Job Object sandbox implementation from the active crate dispatch and route native Windows to the same unsupported sandbox behavior as other non-Linux/non-macOS targets. Remove native Windows CI so the support matrix matches the PRD, and add source-level regression tests that prevent Windows support claims or Job Object code from silently returning. Keep runtime/product docs honest: Windows users must run inside WSL2, where Aegis behaves as Linux.

**Tech Stack:** Rust 2024, `thiserror`, `tracing`, platform `cfg`, GitHub Actions YAML, Markdown docs, existing integration tests.

---

## Scope and non-goals

### In scope

- Remove native Windows Job Object sandbox code from `crates/aegis-sandbox`.
- Remove/gate the `windows-latest` CI job in `.github/workflows/ci.yml`.
- Make native Windows command execution fail explicitly rather than falling through to `cmd.exe`.
- Keep WSL2 guidance as Linux-environment support.
- Add regression tests that enforce:
  - no native Windows CI job,
  - no `windows.rs` platform module dispatch in `aegis-sandbox`,
  - no Job Object implementation file,
  - docs still distinguish WSL2 from native Windows.
- Update `TASKS.md` after verification.

### Out of scope

- No PowerShell parser.
- No `cmd.exe` shell-compat support.
- No AppContainer/WFP sandbox.
- No dependency changes.
- No scanner/parser changes and therefore no scanner benchmark required.

### Human checkpoint

`Cargo.toml` files are protected by repository instructions. `crates/aegis-sandbox/Cargo.toml` currently says the crate supports “Job Objects (Windows)”. If the implementation owner changes that metadata, they must get explicit human approval before editing that `Cargo.toml`. If approval is not granted, leave `Cargo.toml` untouched and record the remaining metadata drift in the handoff.

## Current state to verify before coding

Run:

```bash
rtk git status --short --branch
rtk git grep -n "Windows\\|windows-latest\\|Job Object\\|PowerShell\\|cmd.exe" README.md docs .github/workflows crates/aegis-sandbox src tests
rtk sed -n '150,270p' src/shell_compat.rs
rtk sed -n '185,230p' .github/workflows/ci.yml
```

Expected current signals:

- `.github/workflows/ci.yml` contains a `windows:` job with `runs-on: windows-latest`.
- `crates/aegis-sandbox/src/lib.rs` dispatches `#[cfg(windows)]` to `windows.rs`.
- `crates/aegis-sandbox/src/windows.rs` contains the Job Object implementation.
- `src/shell_compat.rs` has a native Windows `SandboxExecutor::run(cmd)` path and then can spawn the resolved shell.
- `README.md` and `docs/platform-support.md` already document WSL2 vs native Windows; keep that behavior covered.

---

## File structure

### Create

- `tests/platform_scope.rs`
  - Source-level contract tests for M4.1 platform scope.

### Modify

- `.github/workflows/ci.yml`
  - Remove the native `windows` job.

- `crates/aegis-sandbox/src/lib.rs`
  - Remove Windows Job Object docs.
  - Remove `#[cfg(windows)] #[path = "windows.rs"] mod platform;`.
  - Include Windows in the unsupported platform alias.
  - Update `sandbox_available_for` docs so Windows returns unsupported, not Job Objects available.

- `crates/aegis-sandbox/src/unsupported.rs`
  - Update module docs to say unsupported targets include native Windows.

- `src/shell_compat.rs`
  - Add an explicit native Windows unsupported branch in `exec_command`.
  - Do not run `cmd.exe`, PowerShell, or `aegis_sandbox::SandboxExecutor` on native Windows.

- `README.md`
  - Verify existing wording is still accurate; tighten only if needed.

- `docs/platform-support.md`
  - Verify existing wording is still accurate; tighten only if needed.

- `docs/ci.md`
  - Remove native Windows CI claims if present.

- `docs/release-readiness.md`
  - Remove native Windows CI/release claims if present.

- `docs/releases/v1.0.0.md`
  - Keep or update native Windows unsupported wording.

- `TASKS.md`
  - Mark M4.1 complete after code, docs, and gates pass.

### Delete

- `crates/aegis-sandbox/src/windows.rs`
  - Remove native Windows Job Object code and tests.

### Conditional modify with explicit human approval

- `crates/aegis-sandbox/Cargo.toml`
  - Change description from “Job Objects (Windows)” to “unsupported on native Windows; use WSL2/Linux” only after explicit approval.

---

## Iteration 1: Add platform-scope regression tests

**Files:**

- Create: `tests/platform_scope.rs`

- [ ] **Step 1: Write failing tests**

Create `tests/platform_scope.rs`:

```rust
use std::fs;
use std::path::{Path, PathBuf};

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read(relative: &str) -> String {
    fs::read_to_string(repo_path(relative))
        .unwrap_or_else(|err| panic!("failed to read {relative}: {err}"))
}

#[test]
fn ci_should_not_run_native_windows_job_for_1_0() {
    let ci = read(".github/workflows/ci.yml");

    assert!(
        !ci.contains("runs-on: windows-latest"),
        "PRD §8 supports Windows only through WSL2/Linux; native windows-latest CI must be removed"
    );
    assert!(
        !ci.contains("name: Windows (compile + unit tests)"),
        "native Windows compile/unit-test job contradicts the 1.0 platform matrix"
    );
}

#[test]
fn sandbox_crate_should_not_dispatch_to_native_windows_module() {
    let lib = read("crates/aegis-sandbox/src/lib.rs");

    assert!(
        !lib.contains("#[path = \"windows.rs\"]"),
        "aegis-sandbox must not dispatch to a native Windows Job Object module in 1.0"
    );
    assert!(
        !repo_path("crates/aegis-sandbox/src/windows.rs").exists(),
        "native Windows Job Object implementation must be removed for M4.1"
    );
    assert!(
        lib.contains("target_os = \"windows\"") || lib.contains("windows"),
        "lib.rs should still document or cfg-route native Windows as unsupported"
    );
}

#[test]
fn shell_compat_should_not_execute_native_windows_shells() {
    let shell_compat = read("src/shell_compat.rs");

    assert!(
        shell_compat.contains("native Windows is unsupported")
            || shell_compat.contains("Native Windows is unsupported"),
        "native Windows shell execution should fail with an explicit unsupported message"
    );
    assert!(
        !shell_compat.contains("executor.run(cmd)"),
        "native Windows must not run commands through the removed sandbox executor path"
    );
}

#[test]
fn docs_should_keep_wsl2_as_linux_and_native_windows_unsupported() {
    let platform_doc = read("docs/platform-support.md");
    let readme = read("README.md");

    for contents in [&platform_doc, &readme] {
        assert!(
            contents.contains("WSL2"),
            "docs must keep the supported Windows-host path: WSL2/Linux"
        );
        assert!(
            contents.contains("PowerShell") && contents.contains("cmd.exe"),
            "docs must explicitly say native Windows shells are unsupported"
        );
    }
}
```

- [ ] **Step 2: Run tests to verify red state**

Run:

```bash
rtk cargo test --test platform_scope
```

Expected before implementation:

- `ci_should_not_run_native_windows_job_for_1_0` fails because `ci.yml` contains `windows-latest`.
- `sandbox_crate_should_not_dispatch_to_native_windows_module` fails because `lib.rs` dispatches to `windows.rs` and the file exists.
- `shell_compat_should_not_execute_native_windows_shells` fails because the native Windows branch uses `executor.run(cmd)`.

- [ ] **Step 3: Commit only if your workflow commits red tests separately**

If using strict TDD commits:

```bash
rtk git add tests/platform_scope.rs
rtk git commit -m "test: lock native windows out of platform scope"
```

If the repo prefers one commit per task, do not commit yet; continue to Iteration 2.

---

## Iteration 2: Remove native Windows sandbox dispatch and Job Object module

**Files:**

- Modify: `crates/aegis-sandbox/src/lib.rs`
- Modify: `crates/aegis-sandbox/src/unsupported.rs`
- Delete: `crates/aegis-sandbox/src/windows.rs`

- [ ] **Step 1: Update `crates/aegis-sandbox/src/lib.rs` module docs**

Replace the platform list at the top of `lib.rs`:

```rust
//! sandbox on supported platforms:
//! - **Linux**: bwrap + Landlock
//! - **macOS**: Seatbelt (`sandbox-exec`)
//!
//! Native Windows is intentionally unsupported for Aegis 1.0. Windows users
//! should run Aegis inside WSL2, where it behaves as Linux.
```

Replace the platform-module paragraph:

```rust
//! Platform-specific implementation lives in a private `platform` module alias
//! that resolves to `linux.rs`, `macos.rs`, or `unsupported.rs` depending on the
//! build target. Shared helpers (forced-unavailable test hook, bypass warning)
//! live in `support.rs`.
```

- [ ] **Step 2: Update `lib.rs` platform aliases**

Replace:

```rust
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
#[path = "unsupported.rs"]
mod platform;
```

with:

```rust
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[path = "unsupported.rs"]
mod platform;
```

- [ ] **Step 3: Update `sandbox_available_for` docs**

Replace:

```rust
/// without forking. On Windows, always returns `true` (Job Objects are
/// available on all Vista+ systems). On other non-Linux/non-macOS targets
/// always returns `false`.
```

with:

```rust
/// without forking. Native Windows and other non-Linux/non-macOS targets
/// always return `false`; Windows users should run Aegis inside WSL2/Linux.
```

- [ ] **Step 4: Update `unsupported.rs` docs**

Replace the module docs with:

```rust
//! Fallback sandbox implementation for unsupported targets.
//!
//! Native Windows is intentionally unsupported for Aegis 1.0. Windows users
//! should run Aegis inside WSL2, where this crate is compiled for Linux and uses
//! the Linux sandbox implementation. On native Windows and any other target
//! that is not Linux or macOS, the sandbox is always unavailable.
```

Keep the implementation:

```rust
use crate::support::run_unavailable_result;
use crate::{SandboxConfig, SandboxError, SandboxResult};

pub(crate) fn sandbox_available_for(config: &SandboxConfig) -> bool {
    let _ = config;
    false
}

pub(crate) fn run(config: &SandboxConfig, _cmd: &str) -> Result<SandboxResult, SandboxError> {
    run_unavailable_result(config.required)
}
```

- [ ] **Step 5: Delete the native Windows module**

Delete:

```text
crates/aegis-sandbox/src/windows.rs
```

- [ ] **Step 6: Run focused tests**

Run:

```bash
rtk cargo test -p aegis-sandbox
rtk cargo test --test platform_scope
```

Expected:

- `aegis-sandbox` tests pass on Linux.
- `platform_scope` still fails only on CI/shell-compat checks until Iterations 3 and 4 are complete.

- [ ] **Step 7: Commit if using per-iteration commits**

```bash
rtk git add crates/aegis-sandbox/src/lib.rs crates/aegis-sandbox/src/unsupported.rs tests/platform_scope.rs
rtk git add -u crates/aegis-sandbox/src/windows.rs
rtk git commit -m "refactor: remove native windows sandbox module"
```

---

## Iteration 3: Make native Windows shell execution explicitly unsupported

**Files:**

- Modify: `src/shell_compat.rs`

- [ ] **Step 1: Replace the native Windows sandbox executor branch**

In `exec_command`, inside the `#[cfg(not(unix))]` block, replace the current `#[cfg(windows)]` sandbox handling and generic command spawning with explicit unsupported behavior.

Use this structure:

```rust
    #[cfg(not(unix))]
    {
        #[cfg(windows)]
        {
            let _ = (cmd, launch, sandbox);
            eprintln!(
                "error: native Windows is unsupported; run Aegis inside WSL2/Linux instead"
            );
            return EXIT_INTERNAL;
        }

        #[cfg(not(windows))]
        {
            let _ = sandbox;

            let mut command = Command::new(&shell);
            command
                .arg(launch.command_flag(&shell))
                .arg(cmd)
                .args(&launch.positional_args)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            match command.status() {
                Ok(status) => status.code().unwrap_or(EXIT_INTERNAL),
                Err(err) => {
                    eprintln!("error: failed to spawn shell {}: {err}", shell.display());
                    EXIT_INTERNAL
                }
            }
        }
    }
```

Rationale:

- Native Windows binary execution should not pretend `$SHELL`/PowerShell/cmd support exists.
- WSL2 remains unaffected because WSL2 builds/runs as Linux and uses the existing Unix branch.

- [ ] **Step 2: Run focused tests**

Run:

```bash
rtk cargo test --test platform_scope
rtk cargo test -p aegis-sandbox
```

Expected:

- `shell_compat_should_not_execute_native_windows_shells` passes.
- CI-related `platform_scope` test may still fail until Iteration 4.

- [ ] **Step 3: Commit if using per-iteration commits**

```bash
rtk git add src/shell_compat.rs tests/platform_scope.rs
rtk git commit -m "fix: make native windows shell execution unsupported"
```

---

## Iteration 4: Remove native Windows CI job

**Files:**

- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Delete the `windows` job**

Remove this whole job from `.github/workflows/ci.yml`:

```yaml
  windows:
    name: Windows (compile + unit tests)
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@93cb6efe18208431cddfb8368fd83d5badbf9bfd # v5.0.1

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9 # master (Add 1.94.1 patch release)
        with:
          toolchain: ${{ env.RUST_TOOLCHAIN }}

      - name: Cache cargo registry
        uses: actions/cache@a7833574556fa59680c1b7cb190c1735db73ebf0 # v5.0.0
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: windows-cargo-${{ hashFiles('**/Cargo.lock') }}
          restore-keys: windows-cargo-

      # Integration tests that spawn the aegis binary use Unix process
      # semantics and are gated by #[cfg(unix)].  Running --lib exercises
      # the #[cfg(windows)] code paths and proves the crate compiles cleanly
      # on Windows without pulling in Unix-only syscalls.
      - name: cargo test --lib
        run: cargo test --lib --locked
```

- [ ] **Step 2: Verify YAML-level regression test**

Run:

```bash
rtk cargo test --test platform_scope
```

Expected:

- `ci_should_not_run_native_windows_job_for_1_0` passes.
- All `platform_scope` tests pass unless docs still need tightening.

- [ ] **Step 3: Commit if using per-iteration commits**

```bash
rtk git add .github/workflows/ci.yml tests/platform_scope.rs
rtk git commit -m "ci: remove native windows job"
```

---

## Iteration 5: Tighten docs and platform-support contracts

**Files:**

- Modify: `README.md`
- Modify: `docs/platform-support.md`
- Modify: `docs/ci.md`
- Modify: `docs/release-readiness.md`
- Modify: `docs/releases/v1.0.0.md`
- Optional with explicit human approval: `crates/aegis-sandbox/Cargo.toml`

- [ ] **Step 1: Check current docs for stale native Windows claims**

Run:

```bash
rtk git grep -n "Windows\\|windows\\|Job Object\\|PowerShell\\|cmd.exe" README.md docs crates/aegis-sandbox/Cargo.toml
```

Expected after earlier iterations:

- `README.md` and `docs/platform-support.md` may mention Windows only to say WSL2 is best-effort and native Windows shells are unsupported.
- No docs should say native Windows CI, native Windows shell execution, or Job Objects are supported.
- `crates/aegis-sandbox/Cargo.toml` may still contain the stale package description; do not edit it without human approval.

- [ ] **Step 2: Keep README support wording explicit**

Ensure `README.md` contains wording equivalent to:

```markdown
- **Windows with WSL2**

  Run Aegis inside a WSL2 Linux terminal. Native Windows shells like
  **PowerShell** and **cmd.exe** are **not** supported.
```

Also ensure it links:

```markdown
[Platform support](docs/platform-support.md)
```

- [ ] **Step 3: Keep `docs/platform-support.md` explicit**

Ensure the support matrix contains rows equivalent to:

```markdown
| Windows host via WSL2 terminal | Best-effort / not separately validated | Linux userspace and POSIX-style shell execution inside WSL2 | Treated as a Linux environment for terminal usage, but not yet backed by dedicated WSL CI/smoke validation. |
| Windows | Not supported | `PowerShell` and `cmd.exe` are out of scope | Deferred until Aegis has a dedicated Windows interception design. |
```

Ensure the unsupported section contains:

```markdown
The installer rejects Windows explicitly instead of pretending support exists.
```

- [ ] **Step 4: Remove native Windows CI claims from CI/release docs**

If `docs/ci.md` mentions native Windows CI, replace it with:

```markdown
Native Windows CI is intentionally not part of the Aegis 1.0 matrix. Windows
host usage is documented through WSL2, where Aegis runs as Linux.
```

If `docs/release-readiness.md` mentions native Windows release artifacts or native Windows smoke tests, replace it with:

```markdown
Aegis 1.0 does not publish native Windows artifacts. Windows users should run
the Linux build inside WSL2.
```

If `docs/releases/v1.0.0.md` already states native Windows shells are unsupported, keep it. If it claims native Windows binaries are supported, replace that claim with:

```markdown
Native Windows shells are unsupported; Windows host usage is documented through
WSL2 terminal guidance.
```

- [ ] **Step 5: Optional Cargo metadata update after explicit approval**

If the human explicitly approves editing `crates/aegis-sandbox/Cargo.toml`, change:

```toml
description = "Sandboxing layer for Aegis — bwrap+Landlock (Linux), Seatbelt (macOS), Job Objects (Windows)."
```

to:

```toml
description = "Sandboxing layer for Aegis — bwrap+Landlock (Linux), Seatbelt (macOS); native Windows unsupported."
```

If approval is not granted, do not edit the file and mention the stale metadata in the handoff.

- [ ] **Step 6: Run doc/platform tests**

Run:

```bash
rtk cargo test --test platform_support_docs
rtk cargo test --test platform_scope
```

Expected:

- Both tests pass.

- [ ] **Step 7: Commit if using per-iteration commits**

Without Cargo metadata approval:

```bash
rtk git add README.md docs/platform-support.md docs/ci.md docs/release-readiness.md docs/releases/v1.0.0.md tests/platform_scope.rs
rtk git commit -m "docs: align windows scope with wsl2 policy"
```

With Cargo metadata approval:

```bash
rtk git add README.md docs/platform-support.md docs/ci.md docs/release-readiness.md docs/releases/v1.0.0.md crates/aegis-sandbox/Cargo.toml tests/platform_scope.rs
rtk git commit -m "docs: align windows scope with wsl2 policy"
```

---

## Iteration 6: Update task tracking

**Files:**

- Modify: `TASKS.md`

- [ ] **Step 1: Mark M4.1 done**

Replace:

```markdown
- [ ] **M4.1 — Remove native-Windows scope**
```

with:

```markdown
- [x] **M4.1 — Remove native-Windows scope**
```

Update the M4.1 details to describe the actual result:

```markdown
  - Removed the native `windows-latest` CI job from the 1.0 matrix.
  - Removed native Windows Job Object sandbox dispatch/code from
    `aegis-sandbox`; native Windows now routes to unsupported sandbox behavior,
    while WSL2 continues to use the Linux implementation.
  - Native Windows shell execution fails explicitly with WSL2 guidance instead
    of falling through to `PowerShell`/`cmd.exe` semantics.
  - Docs and regression tests keep WSL2-as-Linux separate from native Windows.
  - _Done when (met):_ CI matrix matches PRD §8 (Linux x86_64/aarch64, macOS
    arm64/x86_64; Windows covered transitively via WSL2/Linux); no doc claims
    native PowerShell/cmd support.
```

- [ ] **Step 2: Update Suggested ordering**

In `TASKS.md`, change:

```markdown
4. **M4** (platform reconciliation) — prevents shipping contradictory Windows claims.
```

to:

```markdown
4. ~~**M4** (platform reconciliation)~~ — ✅ done; native Windows scope removed, WSL2 documented as Linux.
```

- [ ] **Step 3: Run TASKS/doc tests**

Run:

```bash
rtk cargo test --test platform_scope
rtk cargo test --test platform_support_docs
```

Expected:

- Tests pass.

- [ ] **Step 4: Commit if using per-iteration commits**

```bash
rtk git add TASKS.md
rtk git commit -m "docs: close m4 platform reconciliation"
```

---

## Iteration 7: Final verification

**Files:**

- Verify all changed files.

- [ ] **Step 1: Inspect changed files**

Run:

```bash
rtk git status --short --branch
rtk git diff --stat
rtk git diff -- .github/workflows/ci.yml crates/aegis-sandbox/src/lib.rs crates/aegis-sandbox/src/unsupported.rs src/shell_compat.rs tests/platform_scope.rs README.md docs/platform-support.md TASKS.md
```

Expected:

- `crates/aegis-sandbox/src/windows.rs` is deleted.
- No native `windows-latest` CI job remains.
- Native Windows shell execution path is explicit unsupported, not a command runner.
- Docs still mention WSL2 and native Windows unsupported.

- [ ] **Step 2: Run formatting**

Run:

```bash
rtk cargo fmt --check
```

Expected:

- Passes.

- [ ] **Step 3: Run clippy**

Run:

```bash
rtk cargo clippy -- -D warnings
```

Expected:

- Passes.

- [ ] **Step 4: Run focused and full tests**

Run:

```bash
rtk cargo test -p aegis-sandbox
rtk cargo test --test platform_scope
rtk cargo test --test platform_support_docs
rtk cargo test
```

Expected:

- All pass.

- [ ] **Step 5: Run security/dependency gates**

Run:

```bash
rtk cargo audit
rtk cargo deny check
```

Expected:

- `cargo audit` should pass or report only the repo’s known allowed Starlark-chain warnings.
- `cargo deny check` may still fail on pre-existing Starlark-chain advisories/duplicate lock entries. Do not attribute that baseline debt to M4.1 unless this task changed dependency files.

- [ ] **Step 6: Optional native target sanity checks**

If Windows target/toolchain is installed locally:

```bash
rtk cargo check -p aegis-sandbox --target x86_64-pc-windows-gnu
rtk cargo clippy -p aegis-sandbox --target x86_64-pc-windows-gnu -- -D warnings
```

Expected after removing `windows.rs`:

- The sandbox crate compiles without Job Object FFI.
- Clippy no longer reports Job Object-specific warnings.

If the linker/toolchain is missing, record the environment limitation rather than adding dependencies or CI jobs.

---

## Acceptance criteria

- M4.1 in `TASKS.md` is marked `[x]`.
- `.github/workflows/ci.yml` has no `windows-latest` job.
- `crates/aegis-sandbox/src/windows.rs` is deleted.
- `crates/aegis-sandbox/src/lib.rs` no longer dispatches to `windows.rs`.
- Native Windows sandbox availability is `false` through `unsupported.rs`.
- Native Windows command execution does not run `cmd.exe`, PowerShell, or Job Objects; it emits an explicit unsupported message with WSL2 guidance.
- `README.md` and `docs/platform-support.md` state:
  - Linux and macOS are supported,
  - Windows host usage is WSL2/Linux only,
  - native PowerShell/cmd are unsupported.
- Regression tests prevent native Windows CI/docs/sandbox scope from returning unnoticed.
- No new dependencies.
- No new `unsafe`.
- No production `.unwrap()` / `.expect()`.
- Verification results are recorded in the handoff.

## Self-review checklist

- Spec coverage:
  - PRD §5.5 Windows unsupported: covered by removing `windows.rs` dispatch and shell execution.
  - PRD §8 WSL2 as Linux: covered by docs and leaving Linux path untouched.
  - TASKS M4.1 CI matrix: covered by removing `windows-latest` job.
  - TASKS M4.1 no native PowerShell/cmd claims: covered by docs/tests and shell unsupported branch.
- Placeholder scan:
  - No task uses “TBD”, “TODO”, “similar to”, or undefined future functions.
- Type/signature consistency:
  - `SandboxConfig`, `SandboxError`, and `SandboxResult` public types remain unchanged.
  - `sandbox_available_for` remains public and returns `false` on unsupported targets.
  - `prepare_for_exec` remains POSIX-only.

