# Deterministic CI and Honest Docs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pin the CI/release pipeline to immutable workflow and tooling inputs, then align README and supporting docs with the current Aegis behavior without changing runtime semantics.

**Architecture:** Keep runtime code behavior untouched and treat the workflows plus docs as the only primary implementation surfaces. Use one pass to establish immutable workflow constants (`RUST_TOOLCHAIN`, tool versions, action SHAs), a second pass to update workflow files, and a final documentation pass that mirrors the current code paths in `src/ui/confirm.rs`, `src/decision.rs`, `src/config/model.rs`, and `src/policy_output.rs`.

**Tech Stack:** GitHub Actions YAML, Rust toolchain `1.94.0`, `cargo-audit 0.22.1`, `cargo-deny 0.19.1`, `cross 0.2.5`, Markdown docs, existing repo verification commands through `rtk`.

---

## File Structure

### Existing files to modify

- `.github/workflows/ci.yml` — pin action refs, pin Rust/tool versions, keep current CI job contract intact.
- `.github/workflows/release.yml` — pin action refs, pin Rust/`cross`, preserve current release target matrix and checksum/upload flow.
- `README.md` — shorten to an honest user-facing summary and remove behavior drift.
- `docs/config-schema.md` — expand into the precise config/policy reference for current runtime behavior.
- `docs/platform-support.md` — keep only platform matrix and support-boundary content.

### New files to create

- `docs/ci.md` — source of truth for deterministic CI/release guarantees and workflow-vs-runtime CI distinctions.

### Runtime files to read but not semantically modify

- `src/ui/confirm.rs` — confirmation acceptance is only `y` / `yes`; everything else denies.
- `src/decision.rs` — current `Protect` / `Audit` / `Strict` policy matrix and snapshot-request behavior.
- `src/config/model.rs` — current config fields, `snapshot_policy`, layered merge, `ci_policy`, and allowlist comments.
- `src/policy_output.rs` — current `--output json` schema v1.
- `src/main.rs` / `src/watch.rs` — runtime CI behavior wording and policy routing references.

### Boundaries to preserve

- Do not change product runtime semantics.
- Do not change allowlist, mode, snapshot, confirm, or JSON contract behavior.
- Do not “fix” doc drift by changing code.
- If an honest doc update reveals a real runtime mismatch that needs a behavioral fix, stop and open a follow-up instead of folding it into this plan.

---

### Task 1: Establish immutable workflow constants and action pins

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release.yml`
- Read: `docs/superpowers/specs/2026-04-12-deterministic-ci-honest-docs-design.md`

- [ ] **Step 1: Record the exact constants that will be pinned**

Add a working note at the top of your implementation scratchpad using these constants:

```text
RUST_TOOLCHAIN=1.94.0
CARGO_AUDIT_VERSION=0.22.1
CARGO_DENY_VERSION=0.19.1
CROSS_VERSION=0.2.5
```

Action releases to resolve into commit SHAs before editing workflows:

```text
actions/checkout
actions/cache
actions/upload-artifact
actions/download-artifact
dtolnay/rust-toolchain
softprops/action-gh-release
```

- [ ] **Step 2: Verify the current workflows still contain floating refs that must be removed**

Run:

```bash
rtk rg -n "@stable|@v[0-9]" .github/workflows/ci.yml .github/workflows/release.yml
```

Expected: matches for floating refs such as `dtolnay/rust-toolchain@stable`, `actions/checkout@v4`, `actions/cache@v4`, and `softprops/action-gh-release@v2`.

- [ ] **Step 3: Resolve the exact SHAs for the selected action releases and annotate them**

For each action release chosen during implementation, replace floating refs with the immutable form:

```yaml
- uses: actions/checkout@<resolved-sha> # v4.2.2
- uses: actions/cache@<resolved-sha> # v4.3.0
- uses: dtolnay/rust-toolchain@<resolved-sha> # stable toolchain action release
```

Do not proceed with workflow edits until every action used by these two workflows has a resolved SHA and a readable release comment.

- [ ] **Step 4: Commit nothing yet**

Do **not** commit after this task. The constants and SHAs are inputs for Tasks 2 and 3.

---

### Task 2: Make `.github/workflows/ci.yml` deterministic and self-explanatory

**Files:**
- Modify: `.github/workflows/ci.yml`
- Test: `.github/workflows/ci.yml`

- [ ] **Step 1: Write the failing verification checks for CI determinism**

Before editing, define the checks you expect the final file to satisfy:

```bash
rtk rg -n "@stable|@v[0-9]" .github/workflows/ci.yml
rtk rg -n "cargo install cargo-audit --locked$|cargo install cargo-deny --locked$" .github/workflows/ci.yml
rtk rg -n "RUST_TOOLCHAIN: 1\\.94\\.0|CARGO_AUDIT_VERSION: 0\\.22\\.1|CARGO_DENY_VERSION: 0\\.19\\.1" .github/workflows/ci.yml
```

Expected before the edit:

- first command finds floating action refs
- second command finds unversioned installs
- third command fails because shared constants do not exist yet

- [ ] **Step 2: Update the workflow to use pinned constants and SHAs**

Reshape the workflow header to centralize versions:

```yaml
env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1
  RUST_TOOLCHAIN: 1.94.0
  CARGO_AUDIT_VERSION: 0.22.1
  CARGO_DENY_VERSION: 0.19.1
```

Then update each relevant step to follow this pattern:

```yaml
- uses: actions/checkout@<resolved-sha> # v4.2.2

- name: Install Rust toolchain
  uses: dtolnay/rust-toolchain@<resolved-sha> # <resolved release>
  with:
    toolchain: ${{ env.RUST_TOOLCHAIN }}
    components: rustfmt, clippy

- name: Install cargo-audit
  run: cargo install cargo-audit --version ${{ env.CARGO_AUDIT_VERSION }} --locked

- name: Install cargo-deny
  run: cargo install cargo-deny --version ${{ env.CARGO_DENY_VERSION }} --locked
```

Do not change the job inventory or remove the existing bench-policy flow.

- [ ] **Step 3: Re-run the determinism checks and confirm the file is pinned**

Run:

```bash
rtk rg -n "@stable|@v[0-9]" .github/workflows/ci.yml
rtk rg -n "cargo install cargo-audit --version \\$\\{\\{ env\\.CARGO_AUDIT_VERSION \\}\\} --locked" .github/workflows/ci.yml
rtk rg -n "cargo install cargo-deny --version \\$\\{\\{ env\\.CARGO_DENY_VERSION \\}\\} --locked" .github/workflows/ci.yml
rtk rg -n "RUST_TOOLCHAIN: 1\\.94\\.0|CARGO_AUDIT_VERSION: 0\\.22\\.1|CARGO_DENY_VERSION: 0\\.19\\.1" .github/workflows/ci.yml
```

Expected:

- first command returns no matches
- the remaining commands each find the expected pinned lines

- [ ] **Step 4: Commit the CI workflow**

Run:

```bash
rtk git add .github/workflows/ci.yml
rtk git commit -m "ci: pin workflow and audit tool versions"
```

---

### Task 3: Make `.github/workflows/release.yml` deterministic without changing release semantics

**Files:**
- Modify: `.github/workflows/release.yml`
- Test: `.github/workflows/release.yml`

- [ ] **Step 1: Write the failing verification checks for release determinism**

Run:

```bash
rtk rg -n "@stable|@v[0-9]" .github/workflows/release.yml
rtk rg -n "cargo install cross --locked$" .github/workflows/release.yml
rtk rg -n "RUST_TOOLCHAIN: 1\\.94\\.0|CROSS_VERSION: 0\\.2\\.5" .github/workflows/release.yml
```

Expected before the edit:

- floating refs are still present
- `cross` install is unversioned
- shared constants are missing

- [ ] **Step 2: Update the release workflow to use the same immutable inputs**

Add the shared constants:

```yaml
env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1
  RUST_TOOLCHAIN: 1.94.0
  CROSS_VERSION: 0.2.5
```

Update action refs to SHA pins and change toolchain/tool installs to:

```yaml
- uses: actions/checkout@<resolved-sha> # v4.2.2

- name: Install Rust toolchain
  uses: dtolnay/rust-toolchain@<resolved-sha> # <resolved release>
  with:
    toolchain: ${{ env.RUST_TOOLCHAIN }}
    targets: ${{ matrix.target }}

- name: Install cross (Linux aarch64 only)
  if: matrix.use_cross
  run: cargo install cross --version ${{ env.CROSS_VERSION }} --locked
```

Keep the target matrix, checksum generation, artifact upload, and GitHub Release steps semantically unchanged.

- [ ] **Step 3: Re-run the release determinism checks**

Run:

```bash
rtk rg -n "@stable|@v[0-9]" .github/workflows/release.yml
rtk rg -n "cargo install cross --version \\$\\{\\{ env\\.CROSS_VERSION \\}\\} --locked" .github/workflows/release.yml
rtk rg -n "RUST_TOOLCHAIN: 1\\.94\\.0|CROSS_VERSION: 0\\.2\\.5" .github/workflows/release.yml
```

Expected:

- first command returns no matches
- the remaining commands find the pinned constants and versioned `cross` install

- [ ] **Step 4: Commit the release workflow**

Run:

```bash
rtk git add .github/workflows/release.yml
rtk git commit -m "ci: pin release workflow inputs"
```

---

### Task 4: Add `docs/ci.md` and document the workflow contract honestly

**Files:**
- Create: `docs/ci.md`
- Modify: `README.md`
- Test: `docs/ci.md`

- [ ] **Step 1: Draft the CI doc structure from the approved spec**

Create `docs/ci.md` with these top-level sections:

```md
# CI and Release Guarantees

## Pinned Inputs
## Current CI Jobs
## What CI Guarantees
## What CI Does Not Guarantee
## Runtime `ci_policy` vs GitHub Actions CI
## Release Workflow Contract
```

- [ ] **Step 2: Fill `docs/ci.md` with the exact current workflow contract**

Document the pinned constants and current job set explicitly, including the distinction between workflow determinism and runtime behavior:

```md
## Pinned Inputs

- Rust toolchain: `1.94.0`
- `cargo-audit`: `0.22.1`
- `cargo-deny`: `0.19.1`
- `cross`: `0.2.5`
- GitHub Actions in `.github/workflows/*.yml` are pinned by commit SHA

## What CI Guarantees

- the workflow definition does not depend on floating toolchain/tool/action refs
- CI runs formatting, linting, tests, dependency audit, deny policy, release builds, and benchmark policy checks as defined in the pinned workflows

## What CI Does Not Guarantee

- byte-for-byte reproducible binaries across all environments
- stronger runtime security semantics than the Aegis code actually implements
```

Also include a section explaining that `ci_policy` is a runtime Aegis config surface, not the GitHub Actions workflow definition itself.

- [ ] **Step 3: Verify the new CI doc contains the required contract language**

Run:

```bash
rtk rg -n "Pinned Inputs|What CI Guarantees|What CI Does Not Guarantee|Runtime `ci_policy` vs GitHub Actions CI|1\\.94\\.0|0\\.22\\.1|0\\.19\\.1|0\\.2\\.5" docs/ci.md
```

Expected: all required sections and pinned versions are present.

- [ ] **Step 4: Commit the CI documentation**

Run:

```bash
rtk git add docs/ci.md
rtk git commit -m "docs: add ci guarantees reference"
```

---

### Task 5: Expand `docs/config-schema.md` into the precise runtime config and policy reference

**Files:**
- Modify: `docs/config-schema.md`
- Read: `src/config/model.rs`
- Read: `src/decision.rs`
- Read: `src/ui/confirm.rs`
- Read: `src/policy_output.rs`

- [ ] **Step 1: Write the target outline before editing**

Reshape `docs/config-schema.md` so it contains these sections:

```md
# Config schema

## Schema evolution
## Layered merge order
## Current schema version
## Mode semantics
## Allowlist semantics
## Snapshot policy
## CI policy
## JSON output contract
## Compatibility policy
```

- [ ] **Step 2: Replace drifted or missing semantics with text that matches the current code**

Use these exact runtime facts as the source of truth:

```md
### Prompt semantics

- interactive approval accepts only `y` / `yes`
- empty input, any other input, read failure, and non-interactive prompt-required flows deny

### Snapshot policy

- `None` never requests snapshots
- `Selective` honors `auto_snapshot_git` / `auto_snapshot_docker`
- `Full` requests all applicable snapshot plugins regardless of per-plugin flags

### CI policy

- `ci_policy` is a runtime policy input
- in `Protect`, `ci_policy = Block` blocks non-safe commands instead of prompting
- `Strict` is not weakened by CI
- `Audit` remains non-blocking
```

For the JSON contract, document schema version `1` and the current top-level fields from `src/policy_output.rs`:

```json
{
  "schema_version": 1,
  "command": "rm -rf /tmp",
  "risk": "danger",
  "decision": "prompt",
  "exit_code": 2,
  "mode": "protect",
  "ci_state": { "detected": false, "policy": "block" },
  "matched_patterns": [],
  "allowlist_match": { "matched": false, "effective": false },
  "snapshots_created": [],
  "snapshot_plan": { "requested": true, "applicable_plugins": [] },
  "execution": { "mode": "evaluation_only", "will_execute": false },
  "decision_source": "builtin_pattern"
}
```

- [ ] **Step 3: Verify the expanded config doc includes every required semantics section**

Run:

```bash
rtk rg -n "Layered merge order|Mode semantics|Allowlist semantics|Snapshot policy|CI policy|JSON output contract|y` / `yes|evaluation_only|decision_source" docs/config-schema.md
```

Expected: all required headings and current-runtime terminology are present.

- [ ] **Step 4: Commit the config-schema doc**

Run:

```bash
rtk git add docs/config-schema.md
rtk git commit -m "docs: align config schema reference"
```

---

### Task 6: Rewrite `README.md` and keep `docs/platform-support.md` narrow

**Files:**
- Modify: `README.md`
- Modify: `docs/platform-support.md`
- Test: `README.md`
- Test: `docs/platform-support.md`

- [ ] **Step 1: Remove high-level README drift without turning it into a spec**

Update the README so the user-facing behavior summary matches the current code:

```md
- confirmation approves only on `y` / `yes`
- default is deny
- `Protect`, `Audit`, and `Strict` are summarized at a high level
- snapshot policy is summarized at a high level
- runtime `ci_policy` is mentioned briefly
- `--output json` is described briefly and linked to `docs/config-schema.md`
- detailed CI/release guarantees link to `docs/ci.md`
```

Delete or soften any wording that implies:

- `Warn` defaults to yes
- README is the canonical policy edge-case reference
- CI determinism means more than pinned workflow/tooling inputs

- [ ] **Step 2: Keep `docs/platform-support.md` focused on platform boundaries only**

Make sure `docs/platform-support.md` contains only support-matrix content like:

```md
## Support matrix
## Current strategy
## Unsupported Windows strategy
## Why Windows is deferred
```

Do not add mode, allowlist, snapshot, or CI semantics here.

- [ ] **Step 3: Verify the README/platform docs now point readers to the right sources of truth**

Run:

```bash
rtk rg -n "docs/config-schema\\.md|docs/ci\\.md|y/N|y` / `yes|default is deny|ci_policy" README.md
rtk rg -n "Support matrix|Windows|Unix-like" docs/platform-support.md
rtk rg -n "Mode semantics|Snapshot policy|JSON output contract|ci_policy" docs/platform-support.md
```

Expected:

- README contains the short, honest references and links
- platform-support still contains platform-only content
- the final command returns no policy/config-topic matches in `docs/platform-support.md`

- [ ] **Step 4: Commit the README/platform docs**

Run:

```bash
rtk git add README.md docs/platform-support.md
rtk git commit -m "docs: sync readme and platform support"
```

---

### Task 7: Run final verification and review for semantic drift

**Files:**
- Modify: none
- Test: `.github/workflows/ci.yml`
- Test: `.github/workflows/release.yml`
- Test: `README.md`
- Test: `docs/config-schema.md`
- Test: `docs/ci.md`
- Test: `docs/platform-support.md`

- [ ] **Step 1: Review the final diff for forbidden semantic changes**

Run:

```bash
rtk git diff -- .github/workflows/ci.yml .github/workflows/release.yml README.md docs/config-schema.md docs/ci.md docs/platform-support.md
```

Check manually that the diff:

- only changes workflow pinning and explanatory wording
- does not change Rust runtime code
- does not smuggle in policy behavior changes

- [ ] **Step 2: Run the final textual verification sweep**

Run:

```bash
rtk rg -n "@stable|@v[0-9]" .github/workflows/ci.yml .github/workflows/release.yml
rtk rg -n "1\\.94\\.0|0\\.22\\.1|0\\.19\\.1|0\\.2\\.5" .github/workflows/ci.yml .github/workflows/release.yml docs/ci.md
rtk rg -n "y` / `yes|default is deny|Strict|Protect|Audit|snapshot_policy|decision_source|evaluation_only" README.md docs/config-schema.md
```

Expected:

- no floating workflow refs remain
- all pinned version strings appear in workflows/docs where expected
- prompt/mode/snapshot/JSON terminology appears consistently in docs

- [ ] **Step 3: Run repository verification appropriate to the changed scope**

If no Rust/runtime code changed, run the documentation/workflow checks plus the repo baseline smoke checks:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
```

If any runtime-help text or tests were touched during implementation, keep the same commands and inspect their output carefully for unintended semantic fallout.

- [ ] **Step 4: Commit the final verification pass**

Run:

```bash
rtk git add .github/workflows/ci.yml .github/workflows/release.yml README.md docs/config-schema.md docs/ci.md docs/platform-support.md
rtk git commit -m "docs: finalize deterministic ci docs"
```

---

## Self-Review

### Spec coverage

This plan covers every approved spec surface:

- deterministic CI pins in `.github/workflows/ci.yml`
- deterministic release pins in `.github/workflows/release.yml`
- new `docs/ci.md`
- honest high-level README wording
- expanded `docs/config-schema.md`
- narrow `docs/platform-support.md`
- explicit protection against runtime semantic drift

### Placeholder scan

The plan uses exact file paths, exact verification commands, and explicit version constants:

- Rust `1.94.0`
- `cargo-audit 0.22.1`
- `cargo-deny 0.19.1`
- `cross 0.2.5`

The only values intentionally resolved during execution are action commit SHAs, because the implementation must bind each selected action release to the exact immutable ref being pinned. That discovery is itself an explicit planned step, not an omitted detail.

### Type and naming consistency

The same naming is used throughout:

- `RUST_TOOLCHAIN`
- `CARGO_AUDIT_VERSION`
- `CARGO_DENY_VERSION`
- `CROSS_VERSION`
- `docs/ci.md`
- `docs/config-schema.md`

No alternate names are introduced later in the plan.
