# Distribution Closeout: npm Wrapper + Homebrew Smoke Evidence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the remaining distribution evidence gap for `TASKS.md` M3.4 (`npm wrapper package`) and collect the real Homebrew smoke evidence still required for M3.3.

**Architecture:** Keep distribution verification outside Aegis runtime code. npm verification is split into network-free Rust contract tests, npm package dry-run, no-network install smoke, and live binary-download smoke; Homebrew verification is split into formula contract tests, tap audit, and real install/test on macOS and Linux. Do not mark either milestone complete until the corresponding live install evidence exists.

**Tech Stack:** Rust integration tests, Node.js/npm package tooling, Homebrew formula/tap tooling, GitHub Release binary assets with `.sha256` sidecars, Aegis `rtk` command wrapper.

---

## Scope and non-goals

- **In scope:** local verification plan for `packaging/npm`, `packaging/homebrew`, release-readiness evidence, and final `TASKS.md` status updates.
- **In scope:** exact local commands for npm and brew verification.
- **Out of scope:** changing scanner/parser/sandbox/runtime behavior.
- **Out of scope:** modifying `Cargo.toml`, `Cargo.lock`, `deny.toml`, or CI workflow files without explicit human approval.
- **Out of scope:** publishing npm or pushing Homebrew tap updates without explicit operator approval.
- **Do not claim M3.3 done** unless `brew install` and `brew test` have passed on both macOS and Linux.
- **Do not claim M3.4 done** unless npm dry-run and live `npm i -g` evidence prove the correct host binary is installed and runnable.

## Current files and responsibilities

- `TASKS.md`
  - Tracks M3.3, M3.4, M3.5, and final M6 release checklist status.
- `docs/release-readiness.md`
  - Holds release-operator validation checklists for Homebrew, npm, and Cargo install paths.
- `packaging/npm/package.json`
  - Defines the npm package metadata, global `aegis` bin shim, supported OS/CPU, files, and `postinstall`.
- `packaging/npm/bin/aegis.js`
  - npm executable shim that forwards to the downloaded native binary.
- `packaging/npm/scripts/install.js`
  - Downloads the platform release asset, follows redirects, verifies SHA256, and installs `vendor/aegis`.
- `packaging/npm/scripts/smoke.js`
  - npm package-local smoke script.
- `packaging/npm/checksums.json`
  - Pinned release tag and SHA256 values for all supported release assets.
- `scripts/update-npm-package.sh`
  - Regenerates npm checksums from GitHub Release `.sha256` sidecars.
- `tests/npm_package.rs`
  - Network-free npm packaging contract tests.
- `tests/npm_live.rs`
  - Gated live npm install smoke test.
- `packaging/homebrew/Formula/aegis.rb`
  - Source formula for the public Homebrew tap.
- `scripts/update-homebrew-formula.sh`
  - Regenerates the formula from a GitHub Release tag and sidecar checksums.
- `tests/homebrew_formula.rs`
  - Network-free Homebrew formula contract tests.
- `tests/homebrew_live.rs`
  - Gated live Homebrew tap/install/test smoke test.

---

## Task 1: Establish a clean distribution workspace

**Files:**
- Inspect only: working tree and current branch.

- [ ] **Step 1: Check branch and dirty files**

Run:

```bash
rtk git branch --show-current
rtk git status --short
```

Expected:

- Branch should be the distribution branch being closed, currently expected as `feat/npm-wrapper`.
- Unrelated files must not be included in the distribution commit.
- If `test_q` is still shown as deleted, decide whether it is intentional before staging anything.
- If `docs/superpowers/plans/*.md` files are untracked, stage only this plan file when committing the plan.

- [ ] **Step 2: Check the exact release tag encoded by npm and Homebrew**

Run:

```bash
rtk sed -n '1,80p' packaging/npm/checksums.json
rtk sed -n '1,80p' packaging/npm/package.json
rtk sed -n '1,80p' packaging/homebrew/Formula/aegis.rb
```

Expected:

- `packaging/npm/package.json` version matches the release encoded in `packaging/npm/checksums.json`.
- `packaging/homebrew/Formula/aegis.rb` version points at the same release tag unless a newer release has been intentionally selected.
- All release asset names are one of:
  - `aegis-linux-x86_64`
  - `aegis-linux-aarch64`
  - `aegis-macos-x86_64`
  - `aegis-macos-aarch64`

---

## Task 2: Verify npm packaging contracts without network

**Files:**
- Test: `tests/npm_package.rs`
- Inspect: `packaging/npm/package.json`
- Inspect: `packaging/npm/scripts/install.js`
- Inspect: `packaging/npm/checksums.json`

- [ ] **Step 1: Run npm Rust contract tests**

Run:

```bash
rtk cargo test --test npm_package
```

Expected:

- PASS.
- Confirms package name, `bin.aegis`, `postinstall`, supported OS/CPU, all release assets, SHA256 checksums, fail-closed installer behavior, redirect handling, and docs coverage.

- [ ] **Step 2: Fix only npm packaging surfaces if the contract fails**

Allowed files for fixes:

```text
packaging/npm/package.json
packaging/npm/bin/aegis.js
packaging/npm/scripts/install.js
packaging/npm/scripts/smoke.js
packaging/npm/checksums.json
packaging/npm/README.md
scripts/update-npm-package.sh
tests/npm_package.rs
tests/npm_live.rs
README.md
docs/release-readiness.md
```

Do not modify Rust runtime crates for npm packaging failures.

- [ ] **Step 3: Re-run the npm contract test after any fix**

Run:

```bash
rtk cargo test --test npm_package
```

Expected:

- PASS before moving to npm dry-run.

---

## Task 3: Verify npm package contents and dry-run publish shape

**Files:**
- Inspect: `packaging/npm/package.json`
- Inspect: npm dry-run output.

- [ ] **Step 1: Run package dry-run**

Run:

```bash
rtk npm pack --dry-run ./packaging/npm
```

To capture machine-readable evidence of the packed file list, use the JSON
variant:

```bash
rtk npm pack --dry-run --json ./packaging/npm
```

> Note: `npm --prefix <dir> pack` does not resolve the package directory in
> npm 11 — it looks for `package.json` at the repo root and fails with `ENOENT`.
> Pass the package directory as a positional argument (`./packaging/npm`)
> instead.

Expected:

- PASS.
- Package includes only intended publish files:
  - `package.json`
  - `README.md`
  - `checksums.json`
  - `bin/aegis.js`
  - `scripts/install.js`
  - `scripts/smoke.js`
- Package does not include `vendor/aegis`.
- Package does not include temporary files, release binaries, `.tmp` files, or repo-local build artifacts.

- [ ] **Step 2: Run npm publish dry-run from the package directory**

Run:

```bash
rtk npm publish --dry-run ./packaging/npm
```

Expected:

- PASS.
- npm reports the package tarball and publish metadata without actually publishing.
- If npm requires login for this dry-run in the local environment, record that as an environment limitation and still keep `npm pack --dry-run` evidence.

---

## Task 4: Verify npm install path without network

**Files:**
- Inspect: `packaging/npm/scripts/install.js`
- Inspect: `packaging/npm/bin/aegis.js`

- [ ] **Step 1: Install with skip-download into an isolated prefix**

Run:

```bash
rtk env AEGIS_NPM_SKIP_DOWNLOAD=1 npm install -g --prefix /tmp/aegis-npm-prefix ./packaging/npm
```

Expected:

- PASS.
- Installer creates a test `vendor/aegis` binary under the installed package.
- No shell startup files or agent config files are modified.

- [ ] **Step 2: Run the installed npm shim**

Run:

```bash
rtk /tmp/aegis-npm-prefix/bin/aegis --version
```

Expected:

- Exit 0.
- Output contains `aegis test binary` in skip-download mode.

- [ ] **Step 3: Clean isolated npm prefix**

Run:

```bash
rtk rm -rf /tmp/aegis-npm-prefix
```

Expected:

- `/tmp/aegis-npm-prefix` is removed.
- Do not remove any non-temporary project path.

---

## Task 5: Verify npm live binary download smoke

**Files:**
- Test: `tests/npm_live.rs`
- Inspect: `packaging/npm/scripts/install.js`
- Inspect: `packaging/npm/checksums.json`

- [ ] **Step 1: Run gated live npm test**

Run:

```bash
rtk env AEGIS_TEST_LIVE_NPM=1 cargo test --test npm_live -- --nocapture
```

Expected:

- PASS on a machine with network access and npm available.
- Test packs/installs the npm package and verifies `aegis --version`.
- The installer downloads the release asset for the host platform, follows GitHub redirects, verifies SHA256, and installs the binary.

- [ ] **Step 2: If the gated test cannot run locally, run a manual isolated live install**

Run:

```bash
rtk npm install -g --prefix /tmp/aegis-npm-live ./packaging/npm
rtk /tmp/aegis-npm-live/bin/aegis --version
```

Expected:

- Exit 0.
- `aegis --version` prints the version from `packaging/npm/package.json`.
- If network access is blocked, do not mark M3.4 complete in `TASKS.md`.

- [ ] **Step 3: Clean isolated live npm prefix**

Run:

```bash
rtk rm -rf /tmp/aegis-npm-live
```

Expected:

- `/tmp/aegis-npm-live` is removed.

---

## Task 6: Verify Homebrew formula contracts without live install

**Files:**
- Test: `tests/homebrew_formula.rs`
- Inspect: `packaging/homebrew/Formula/aegis.rb`
- Inspect: `scripts/update-homebrew-formula.sh`
- Inspect: `docs/release-readiness.md`

- [ ] **Step 1: Run Homebrew formula contract tests**

Run:

```bash
rtk cargo test --test homebrew_formula
```

Expected:

- PASS.
- Confirms all four platform assets, four 64-hex SHA256 pins, raw-binary `using: :nounzip`, no shell rc mutation, `test do`, caveats, and release-readiness runbook coverage.

- [ ] **Step 2: Fix only Homebrew packaging surfaces if the contract fails**

Allowed files for fixes:

```text
packaging/homebrew/Formula/aegis.rb
scripts/update-homebrew-formula.sh
tests/homebrew_formula.rs
tests/homebrew_live.rs
README.md
docs/release-readiness.md
```

Do not modify Rust runtime crates for Homebrew formula failures.

- [ ] **Step 3: Re-run the Homebrew formula contract test after any fix**

Run:

```bash
rtk cargo test --test homebrew_formula
```

Expected:

- PASS before moving to live Homebrew smoke.

---

## Task 7: Verify Homebrew tap and live install smoke

**Files:**
- Test: `tests/homebrew_live.rs`
- Inspect: public tap `IliasAlmerekov/homebrew-aegis`
- Inspect: `packaging/homebrew/Formula/aegis.rb`

- [ ] **Step 1: Run gated live Homebrew test on macOS**

Run on macOS with Homebrew:

```bash
rtk env AEGIS_TEST_LIVE_HOMEBREW=1 cargo test --test homebrew_live -- --nocapture
```

Expected:

- PASS.
- Test taps `IliasAlmerekov/aegis`, installs `aegis`, runs `brew test aegis`, and verifies `aegis --version`.

- [ ] **Step 2: Run gated live Homebrew test on Linux**

Run on Linux with Homebrew:

```bash
rtk env AEGIS_TEST_LIVE_HOMEBREW=1 cargo test --test homebrew_live -- --nocapture
```

Expected:

- PASS.
- Same validation as macOS, but using the Linux release asset.

- [ ] **Step 3: If the gated test cannot be used, run manual Homebrew smoke**

Run on each platform:

```bash
rtk brew tap IliasAlmerekov/aegis
rtk brew install aegis
rtk brew test aegis
rtk aegis --version
```

Expected:

- `brew tap` succeeds or reports the tap is already present.
- `brew install aegis` installs the release binary.
- `brew test aegis` passes.
- `aegis --version` prints the formula version.
- Record separate macOS and Linux evidence.

- [ ] **Step 4: Audit the tap formula when Homebrew is available**

Run in the tap checkout:

```bash
rtk brew audit --strict --online --formula aegis
```

Expected:

- PASS with `0 problems`.
- If the local environment lacks Homebrew, leave M3.3 open and record the missing tool as an environment limitation.

---

## Task 8: Run Rust repository gates for changed packaging/docs/tests

**Files:**
- All changed project files.

- [ ] **Step 1: Run formatting gate**

Run:

```bash
rtk cargo fmt --check
```

Expected:

- PASS.

- [ ] **Step 2: Run clippy gate**

Run:

```bash
rtk cargo clippy -- -D warnings
```

Expected:

- PASS.

- [ ] **Step 3: Run full test suite**

Run:

```bash
rtk cargo test
```

Expected:

- PASS.
- Gated live npm/Homebrew tests skip unless their environment variables are set.

- [ ] **Step 4: Run supply-chain gates if the distribution closeout is being prepared for merge**

Run:

```bash
rtk cargo audit
rtk cargo deny check
```

Expected:

- `rtk cargo audit` exits 0.
- `rtk cargo deny check` result must be reported exactly. If it fails due to pre-existing baseline advisories, do not describe it as green.

---

## Task 9: Record release evidence and update task status

**Files:**
- Modify: `docs/release-readiness.md`
- Modify: `TASKS.md`

- [ ] **Step 1: Record npm evidence**

In `docs/release-readiness.md`, under `## npm wrapper validation`, update checkboxes only for commands that actually passed:

```markdown
## npm wrapper validation

- [x] `scripts/update-npm-package.sh v0.5.6` regenerated
      `packaging/npm/checksums.json` from the selected release tag.
- [x] `npm publish --dry-run` succeeds from `packaging/npm`.
- [x] `npm i -g @iliasalmerekov/aegis` succeeds on Linux x64.
- [ ] `npm i -g @iliasalmerekov/aegis` succeeds on macOS arm64 or x64.
- [x] `aegis --version` prints the selected release version after npm install.
- [x] npm install does not mutate shell startup files or agent config.
```

If macOS npm evidence was also collected, mark the macOS npm line `[x]`. If npm has not been published yet, keep M3.4 open in `TASKS.md` even if local package install passed.

- [ ] **Step 2: Record Homebrew evidence**

In `docs/release-readiness.md`, under `## Homebrew tap validation`, update checkboxes only for commands that actually passed:

```markdown
## Homebrew tap validation

- [x] `packaging/homebrew/Formula/aegis.rb` was generated from the selected
      GitHub Release tag.
- [x] The published tap contains the same `Formula/aegis.rb`.
- [x] `brew audit --strict --online --formula aegis` passes in the tap.
- [x] `brew install IliasAlmerekov/aegis/aegis` succeeds on macOS.
- [x] `brew install IliasAlmerekov/aegis/aegis` succeeds on Linux.
- [x] `brew test IliasAlmerekov/aegis/aegis` passes on both platforms.
```

If any platform was not tested, keep the corresponding line unchecked and keep M3.3 open in `TASKS.md`.

- [ ] **Step 3: Update `TASKS.md` only when done-when contracts are met**

For M3.4, change this only after npm publish and live install evidence:

```markdown
- [x] **M3.4 — npm wrapper package**
  Wrapper that downloads/installs the correct platform binary for the `npm i -g`
  audience.
  - _Done when (met):_ `package.json` published; `npm i -g` installs the right
    binary for the host platform. Evidence recorded in `docs/release-readiness.md`.
```

For M3.3, change this only after macOS and Linux Homebrew install/test evidence:

```markdown
- [x] **M3.3 — Homebrew formula/tap**
  - _Done when (met):_ formula published to the tap and installs on macOS and Linux;
    `brew install` smoke-tested. Evidence recorded in `docs/release-readiness.md`.
```

If either evidence set is incomplete, leave the task as `[ ]`.

---

## Task 10: Commit only the verified distribution scope

**Files:**
- Stage only files intentionally changed by this plan.

- [ ] **Step 1: Review final diff**

Run:

```bash
rtk git status --short
rtk git diff -- docs/superpowers/plans/2026-06-22-distribution-closeout-npm-brew.md docs/release-readiness.md TASKS.md packaging/npm packaging/homebrew tests/npm_package.rs tests/npm_live.rs tests/homebrew_formula.rs tests/homebrew_live.rs scripts/update-npm-package.sh scripts/update-homebrew-formula.sh README.md
```

Expected:

- Diff contains only distribution plan/evidence/status changes.
- No unrelated `test_q` deletion unless it has been explicitly approved.
- No generated `vendor/aegis` binary.
- No `Cargo.toml`, `Cargo.lock`, `deny.toml`, or CI workflow changes unless explicitly approved.

- [ ] **Step 2: Stage intended files**

Run:

```bash
rtk git add docs/superpowers/plans/2026-06-22-distribution-closeout-npm-brew.md
```

If evidence/status files were changed after verification, stage them explicitly:

```bash
rtk git add docs/release-readiness.md TASKS.md
```

- [ ] **Step 3: Commit the plan or evidence**

For plan-only commit:

```bash
rtk git commit -m "docs: plan distribution closeout checks"
```

For evidence/status commit after successful verification:

```bash
rtk git commit -m "docs: record distribution smoke evidence"
```

Expected:

- Commit succeeds.
- No `Co-Authored-By` trailer.

---

## Self-review checklist

- [ ] Every command in this plan uses `rtk`.
- [ ] npm local checks include contract test, package dry-run, skip-download install, and live install.
- [ ] Homebrew local checks include formula contract test, tap audit, macOS install/test, and Linux install/test.
- [ ] M3.3 stays open unless macOS and Linux Homebrew evidence exists.
- [ ] M3.4 stays open unless npm publish/live install evidence exists.
- [ ] No runtime/security-sensitive Rust code changes are required for this plan.
- [ ] No protected dependency or CI files are modified by default.
