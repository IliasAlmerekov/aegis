# M6 Docs, README, Landing, and Production Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the M6 documentation/readiness gap by making README and landing truthful, minimal, current, and production-focused while removing non-production clutter and shrinking the landing bundle.

**Architecture:** Treat `README.md` as the short public entry point and keep detailed operational material in `docs/`. Treat `landing/` as the marketing surface: it must mirror the current release/install facts, avoid unsupported claims, and ship only assets/dependencies that are needed at runtime. Do not weaken security documentation; move or link necessary detail instead of deleting it blindly.

**Tech Stack:** Rust workspace docs and checks via `rtk cargo ...`; landing uses Vite + React + Tailwind CSS v4; all shell commands go through `rtk`.

---

## Scope and non-goals

### In scope

- Rewrite `README.md` to a very small structure:
  1. `What is Aegis?`
  2. `Why Aegis?`
  3. `How to install`
  4. `How it works`
- Keep README honest: Aegis is a heuristic shell guardrail, not a sandbox or hard security boundary.
- Keep the threat model visible from README without creating a long README section.
- Update the landing page so install information, version wording, supported platforms, and safety claims match the current project.
- Remove production-unnecessary tracked files after verifying they are unused.
- Remove ignored local build/dependency artifacts from the checkout, if present, without treating them as source changes.
- Tree-shake the landing by removing unused sections/assets/dependencies or lazy-loading only what remains.
- Update `TASKS.md`, `PROJECT_STATE.md`, and `docs/release-readiness.md` only where evidence supports status changes.

### Out of scope

- Do not change Aegis command interception behavior.
- Do not change scanner/parser/policy/sandbox logic.
- Do not modify `Cargo.toml`, `Cargo.lock`, `deny.toml`, or CI workflow files unless the user explicitly approves it later.
- Do not claim macOS Homebrew/npm smoke has passed unless new evidence is collected.
- Do not delete ADRs, threat model, release-readiness notes, platform docs, or troubleshooting docs just because README becomes shorter.

---

## Current signals to preserve

- `TASKS.md` M6 still has open docs/readiness items.
- `PROJECT_STATE.md` says the 1.0 docs gate is open and README/docs need updating.
- README currently contains too much operational detail for the requested public entry point.
- README currently includes an outdated Cargo example: `cargo install --git ... --tag v0.5.7 aegis`, while project state says current version is `0.5.6`.
- Landing currently shows stale/incorrect copy:
  - `Open Source · Rust · v0.1`
  - `cargo install aegis-guard`
  - direct `$SHELL` setup examples that do not match the explicit `aegis setup-shell` workflow
  - "signed JSONL" / "immutable" audit language that overstates the current audit guarantees
- Landing has heavy/non-production candidates:
  - ignored local artifacts: `landing/node_modules`, `landing/dist`
  - tracked design/source artifacts to verify before removal: `landing/pencil.pen`, `landing/DESIGN.md`, `landing/tokens.json`
  - tracked heavy assets: `landing/images/Hitem3d-1781772057946.glb`, `landing/images/generated-1781681175337.png`, `landing/public/models/shield.glb`
  - possible duplicate diagram asset: `src/assets/howitwork.png`
- Root contains tracked `test_q` (~3.8 MiB); verify purpose before removal.

---

## File map

### Primary docs

- Modify: `README.md`
  - Make it short and public-facing.
  - Keep only the requested sections.
  - Include a short threat-model/limitations link inside the minimal structure.
- Modify: `docs/release-readiness.md`
  - Align checklist boxes with recorded evidence only.
  - Keep caveats for macOS smoke and release-operator follow-ups.
- Modify: `TASKS.md`
  - Mark M6 docs sub-items only after README/landing/docs are updated and verified.
  - Keep unchecked items for perf, security corpus, and unsupported/unverified platform gates.
- Modify: `PROJECT_STATE.md`
  - Update "Last updated" and "What was done last session" after implementation.
  - Update M6 docs gate status only if the docs gate is actually closed.
- Modify: `CHANGELOG.md`
  - Add a concise `Changed` entry for the README/landing documentation simplification.

### Landing

- Modify: `landing/src/App.jsx`
  - Remove sections that are no longer part of the simplified production landing.
- Modify: `landing/src/components/sections/Hero.jsx`
  - Replace stale version/install text.
  - Copy installer command correctly.
- Modify: `landing/src/components/sections/FeatureSection.jsx`
  - Keep "Why Aegis" only if it states true current behavior.
- Modify: `landing/src/components/sections/HowItWorks.jsx`
  - Replace old `$SHELL` setup flow with current install/setup-shell flow.
- Modify: `landing/src/components/sections/CTABanner.jsx`
  - Replace stale `cargo install` emphasis with current supported install options.
- Modify: `landing/src/components/sections/Footer.jsx`
  - Keep links minimal and accurate.
- Modify: `landing/src/components/ui/Nav.jsx`
  - Navigation should match remaining sections only.
- Modify: `landing/index.html`
  - Update title/description to match honest positioning.
- Modify: `landing/package.json`
  - Remove 3D dependencies if 3D components/assets are no longer used.
- Modify: `landing/package-lock.json`
  - Regenerate after package changes using npm through `rtk`.

### Cleanup candidates

- Delete only after reference checks pass:
  - `test_q`
  - `landing/pencil.pen`
  - `landing/DESIGN.md`
  - `landing/tokens.json`
  - `landing/images/Hitem3d-1781772057946.glb`
  - `landing/images/generated-1781681175337.png`
  - `landing/public/models/shield.glb`
  - unused landing section/component files after simplifying `App.jsx`
- Local ignored cleanup, not source changes:
  - `landing/node_modules`
  - `landing/dist`

---

## Proposed README shape

Use exactly these high-level headings:

```markdown
# Aegis

## What is Aegis?

## Why Aegis?

## How to install

## How it works
```

Content rules:

- Keep the whole README short.
- Do not include full config schema, long troubleshooting, release runbooks, or long install behavior explanations.
- Link to details instead of embedding them.
- Mention limitations in one concise sentence under `What is Aegis?` or `How it works`:
  - "Aegis is a heuristic guardrail, not a sandbox or privilege boundary. See `docs/threat-model.md`."
- Install section should show current supported paths:
  - convenience installer
  - Homebrew
  - npm
  - Cargo source install only as developer path, using current release tag or a clearly generic `vX.Y.Z`
- Say package-manager installs are binary-only and that shell-proxy mode is explicit:
  - `aegis setup-shell`
- Do not claim macOS package smoke passed unless verified.

---

## Implementation iterations

### Task 1: Baseline inventory and stale-claim list

**Files:**
- Read: `README.md`
- Read: `TASKS.md`
- Read: `PROJECT_STATE.md`
- Read: `docs/release-readiness.md`
- Read: `docs/threat-model.md`
- Read: `landing/src/**/*.jsx`
- Read: `landing/package.json`

- [ ] **Step 1: Confirm clean working tree**

Run:

```bash
rtk git status --short
```

Expected:

```text
ok
```

- [ ] **Step 2: Search for stale or risky public claims**

Run:

```bash
rtk grep -R -n "v0.1\|v0.5.7\|aegis-guard\|signed JSONL\|immutable\|not a sandbox\|under 2 ms\|cargo install" README.md landing docs TASKS.md PROJECT_STATE.md
```

Expected:

```text
Matches are reviewed and copied into the task notes before editing.
```

- [ ] **Step 3: Inventory landing bundle and tracked cleanup candidates**

Run:

```bash
rtk du -sh landing/* test_q
rtk git ls-files landing test_q
```

Expected:

```text
The implementer identifies tracked files vs ignored local artifacts before deleting anything.
```

- [ ] **Step 4: Commit nothing**

This is an inventory task only. Do not edit files yet.

---

### Task 2: Rewrite README to the minimal public contract

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Replace README with a short public version**

Use this structure and adapt wording only if live project facts require it:

```markdown
# Aegis

> A small safety layer for AI agents that run shell commands.

## What is Aegis?

Aegis is a Rust CLI that sits between an AI agent and your real shell.
It checks each command before it runs:

- safe commands run immediately
- risky commands ask for approval
- catastrophic commands are blocked

Aegis is a heuristic guardrail, not a sandbox or privilege boundary. See
[`docs/threat-model.md`](docs/threat-model.md) for the full security model.

## Why Aegis?

AI agents can move fast and run destructive commands by mistake:

- delete files
- reset repositories
- drop databases
- publish or push something dangerous

Aegis adds a human checkpoint before that damage happens. It also records
decisions in an append-only audit log and can take best-effort snapshots for
some dangerous commands.

## How to install

Supported platforms:

- Linux
- macOS
- Windows through WSL2

Native Windows shells such as PowerShell and `cmd.exe` are not supported.

### Convenience installer

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
```

Reload your shell, then check:

```bash
aegis --version
```

### Homebrew

```bash
brew tap IliasAlmerekov/aegis
brew install aegis
```

### npm

```bash
npm i -g @iliasalmerekov/aegis
```

Homebrew and npm install the binary only. To opt in to shell-proxy mode after
installing with a package manager, run:

```bash
aegis setup-shell
```

Developer source install:

```bash
cargo install --git https://github.com/IliasAlmerekov/aegis --tag v0.5.6 aegis
```

## How it works

```text
AI agent command
      |
      v
 Aegis parses and classifies it
      |
      +--> Safe   -> run
      +--> Warn   -> ask first
      +--> Danger -> snapshot if configured, then ask first
      +--> Block  -> refuse
      |
      v
 real shell executes only approved commands
```

For a visual overview, see:

![Aegis command flow](src/assets/howitwork.png)
```

- [ ] **Step 2: Verify README stays short**

Run:

```bash
rtk wc -l README.md
```

Expected:

```text
README.md is small enough to scan quickly. Target: under 130 lines.
```

- [ ] **Step 3: Verify README has no stale install strings**

Run:

```bash
rtk grep -n "aegis-guard\|v0.1\|v0.5.7\|signed JSONL\|immutable" README.md
```

Expected:

```text
No matches.
```

- [ ] **Step 4: Commit README only**

Run:

```bash
rtk git add README.md
rtk git commit -m "docs: simplify readme for m6"
```

Expected:

```text
Commit succeeds.
```

---

### Task 3: Make landing content truthful and current

**Files:**
- Modify: `landing/src/components/sections/Hero.jsx`
- Modify: `landing/src/components/sections/FeatureSection.jsx`
- Modify: `landing/src/components/sections/HowItWorks.jsx`
- Modify: `landing/src/components/sections/CTABanner.jsx`
- Modify: `landing/src/components/ui/Nav.jsx`
- Modify: `landing/index.html`

- [ ] **Step 1: Update hero install command**

In `landing/src/components/sections/Hero.jsx`, replace:

```jsx
Open Source · Rust · v0.1
```

with:

```jsx
Open Source · Rust · v0.5.6
```

Replace visible and copied install command:

```jsx
cargo install aegis-guard
```

with:

```jsx
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
```

Replace clipboard text with the same installer command.

- [ ] **Step 2: Remove unsupported install/setup claims from How It Works**

In `landing/src/components/sections/HowItWorks.jsx`, replace the first step with:

```jsx
{
  num: '01',
  heading: 'Install Aegis',
  body: 'Use the installer, Homebrew, npm, or Cargo source install. Package-manager installs are binary-only.',
  lines: [
    { prompt: '$', text: 'npm i -g @iliasalmerekov/aegis', color: '#ddffdc' },
    { prompt: '$', text: 'aegis --version', color: '#ddffdc' },
    { text: 'aegis 0.5.6', color: '#7fee64' },
  ],
}
```

Replace old `$SHELL` setup wording with a second step that states explicit opt-in:

```jsx
{
  num: '02',
  heading: 'Opt in to shell-proxy mode',
  body: 'Run setup-shell when you want tools that use $SHELL -c to route through Aegis.',
  lines: [
    { prompt: '$', text: 'aegis setup-shell', color: '#ddffdc' },
    { text: 'managed shell block installed', color: '#7fee64' },
  ],
}
```

Keep the approve/deny step as the third step, but avoid claiming perfect coverage.

- [ ] **Step 3: Fix audit wording**

In `landing/src/components/sections/FeatureSection.jsx` and `landing/src/components/sections/AuditSection.jsx`, replace overclaims:

```text
signed JSONL
immutable
```

with:

```text
append-only JSONL
tamper-evident when hash-chain integrity is enabled
```

If `AuditSection` is removed in Task 4, this edit can be skipped for deleted content.

- [ ] **Step 4: Update metadata**

In `landing/index.html`, set:

```html
<meta name="description" content="Aegis is a Rust shell guardrail for AI agents. It asks before risky commands run and blocks catastrophic commands." />
<title>Aegis — AI shell command guardrail</title>
```

- [ ] **Step 5: Verify stale landing strings are gone**

Run:

```bash
rtk grep -R -n "v0.1\|aegis-guard\|signed JSONL\|immutable\|Point your AI tool" landing/src landing/index.html
```

Expected:

```text
No matches.
```

- [ ] **Step 6: Build landing**

Run:

```bash
rtk npm --prefix landing run build
```

Expected:

```text
vite build completes successfully.
```

- [ ] **Step 7: Commit landing copy**

Run:

```bash
rtk git add landing/src landing/index.html
rtk git commit -m "docs: align landing copy with current install flow"
```

Expected:

```text
Commit succeeds.
```

---

### Task 4: Simplify landing sections and tree-shake runtime bundle

**Files:**
- Modify: `landing/src/App.jsx`
- Modify: `landing/src/components/sections/*.jsx`
- Modify: `landing/src/components/ui/Nav.jsx`
- Modify: `landing/package.json`
- Modify: `landing/package-lock.json`
- Delete if unused: `landing/src/components/3d/Shield.jsx`
- Delete if unused: `landing/src/components/3d/ShieldScene.jsx`
- Delete if unused: `landing/public/models/shield.glb`
- Delete if unused: `landing/images/Hitem3d-1781772057946.glb`

- [ ] **Step 1: Decide final landing sections**

Use only sections that map to the README:

```text
Nav
Hero
Why Aegis
How It Works
Install / CTA
Footer
```

Remove these from `landing/src/App.jsx` unless the user explicitly wants them:

```jsx
TrustStrip
AuditSection
```

- [ ] **Step 2: Remove 3D runtime if not needed**

If the page can use a static diagram or CSS-only command flow instead of 3D, remove imports in `Hero.jsx`:

```jsx
import { lazy, Suspense } from 'react'
```

and remove:

```jsx
const ShieldScene = lazy(...)
<Suspense>...</Suspense>
```

Replace it with a lightweight static command-flow card or the existing `src/assets/howitwork.png` only if Vite can serve it cleanly from the landing app.

- [ ] **Step 3: Remove unused 3D dependencies**

If `ShieldScene` and `Shield` are deleted, remove from `landing/package.json`:

```json
"@react-three/drei": "^9.122.0",
"@react-three/fiber": "^8.18.0",
"three": "^0.173.0"
```

Then run:

```bash
rtk npm --prefix landing install
```

Expected:

```text
package-lock.json is regenerated without three/react-three packages.
```

- [ ] **Step 4: Verify no 3D imports remain**

Run:

```bash
rtk grep -R -n "three\|@react-three\|ShieldScene\|Shield.jsx\|shield.glb" landing/src landing/package.json
```

Expected:

```text
No matches if 3D was removed.
```

- [ ] **Step 5: Build and compare output**

Run:

```bash
rtk npm --prefix landing run build
rtk du -sh landing/dist
```

Expected:

```text
Build succeeds and landing/dist is smaller than before the 3D dependency removal.
```

- [ ] **Step 6: Commit tree-shaking change**

Run:

```bash
rtk git add landing
rtk git commit -m "perf: trim landing runtime bundle"
```

Expected:

```text
Commit succeeds.
```

---

### Task 5: Remove production-unnecessary files safely

**Files:**
- Potential delete: `test_q`
- Potential delete: `landing/pencil.pen`
- Potential delete: `landing/DESIGN.md`
- Potential delete: `landing/tokens.json`
- Potential delete: `landing/images/Hitem3d-1781772057946.glb`
- Potential delete: `landing/images/generated-1781681175337.png`
- Potential delete: `landing/public/models/shield.glb`
- Potential delete: unused landing component files

- [ ] **Step 1: Verify each candidate is unreferenced**

Run:

```bash
rtk grep -R -n "test_q\|pencil.pen\|DESIGN.md\|tokens.json\|Hitem3d-1781772057946\|generated-1781681175337\|shield.glb" . --exclude-dir=.git --exclude-dir=node_modules --exclude-dir=dist
```

Expected:

```text
Only self-references or plan references remain. Any runtime/docs reference must be handled before deletion.
```

- [ ] **Step 2: Remove ignored local artifacts**

Run:

```bash
rtk rm -rf landing/node_modules landing/dist
```

Expected:

```text
Local checkout size is reduced. This should not appear in git diff if ignored.
```

- [ ] **Step 3: Delete tracked production-unnecessary files**

Only after Step 1 proves they are unused, run:

```bash
rtk git rm test_q
rtk git rm landing/pencil.pen landing/DESIGN.md landing/tokens.json
```

If 3D assets were removed in Task 4, also run:

```bash
rtk git rm landing/images/Hitem3d-1781772057946.glb landing/images/generated-1781681175337.png landing/public/models/shield.glb
```

- [ ] **Step 4: Verify app still builds**

Run:

```bash
rtk npm --prefix landing install
rtk npm --prefix landing run build
```

Expected:

```text
Install and build succeed after cleanup.
```

- [ ] **Step 5: Commit cleanup**

Run:

```bash
rtk git add landing/package-lock.json landing/package.json
rtk git commit -m "chore: remove non-production landing artifacts"
```

Expected:

```text
Commit succeeds.
```

---

### Task 6: Reconcile M6 docs and status files

**Files:**
- Modify: `TASKS.md`
- Modify: `docs/release-readiness.md`
- Modify: `PROJECT_STATE.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Update release-readiness checklist truthfully**

In `docs/release-readiness.md`, mark only evidence-backed items as checked.

Keep unchecked or caveated:

```text
macOS Homebrew smoke is still an operator follow-up.
macOS npm smoke is still an operator follow-up.
Hot path p99 must be confirmed separately.
Security bypass corpus must be run separately.
```

- [ ] **Step 2: Update TASKS M6 checklist**

In `TASKS.md`, mark these only if completed by Tasks 2-5:

```markdown
- [x] README and docs accurately describe all features through Phase 6.
- [x] Threat model and known limitations visible **on the README** (link to
      `docs/threat-model.md`).
```

Do not mark these unless separate evidence exists:

```markdown
- [ ] CI includes ARM cross-compilation (`aarch64-unknown-linux-musl`) (← M3.2).
- [ ] Sandbox tested on `ubuntu-latest` and `macos-latest`; a command writing
      outside allowed paths is killed; audit records profile/status per execution.
- [ ] Hot path < 2 ms (p99) confirmed by `cargo criterion`; no regression.
- [ ] Zero false negatives on `tests/fixtures/security_bypass_corpus.toml`.
- [ ] CHANGELOG.md updated for the 1.0 release.
```

- [ ] **Step 3: Update PROJECT_STATE**

Set:

```markdown
## Last updated

2026-06-23
```

Replace "What was done last session" with concise bullets for this docs/landing cleanup.

If README/landing/docs are complete, update:

```markdown
| 1.0 docs gate | README, threat model, docs accuracy | ✅ Done |
```

Keep perf/test gates open.

- [ ] **Step 4: Update CHANGELOG**

Add under `## [Unreleased]`:

```markdown
### Changed
- Simplified README and landing copy for M6 release-readiness accuracy.

### Removed
- Removed non-production landing artifacts and unused landing runtime assets.
```

If no tracked files were removed, omit the `Removed` entry.

- [ ] **Step 5: Commit status/docs**

Run:

```bash
rtk git add TASKS.md docs/release-readiness.md PROJECT_STATE.md CHANGELOG.md
rtk git commit -m "docs: reconcile m6 readiness status"
```

Expected:

```text
Commit succeeds.
```

---

### Task 7: Verification gates

**Files:**
- No planned source edits unless a gate exposes a docs/test issue.

- [ ] **Step 1: Format check**

Run:

```bash
rtk cargo fmt --check
```

Expected:

```text
Pass.
```

- [ ] **Step 2: Clippy**

Run:

```bash
rtk cargo clippy -- -D warnings
```

Expected:

```text
Pass.
```

- [ ] **Step 3: Tests**

Run:

```bash
rtk cargo test
```

Expected:

```text
Pass.
```

- [ ] **Step 4: Supply-chain checks**

Run:

```bash
rtk cargo audit
rtk cargo deny check
```

Expected:

```text
Both exit 0. If optional starlark warnings appear in audit output, report them exactly and compare with current docs before changing status.
```

- [ ] **Step 5: Landing build**

Run:

```bash
rtk npm --prefix landing run build
```

Expected:

```text
Vite build succeeds.
```

- [ ] **Step 6: Stale claim scan**

Run:

```bash
rtk grep -R -n "aegis-guard\|v0.1\|v0.5.7\|signed JSONL\|immutable\|hard security boundary\|PowerShell" README.md landing/src landing/index.html docs TASKS.md PROJECT_STATE.md
```

Expected:

```text
No stale public claims. Native Windows/PowerShell may appear only in explicit unsupported-platform wording.
```

- [ ] **Step 7: Final diff review**

Run:

```bash
rtk git diff --stat
rtk git diff -- README.md landing TASKS.md docs/release-readiness.md PROJECT_STATE.md CHANGELOG.md
```

Expected:

```text
Diff is limited to docs/landing cleanup and status reconciliation. No Rust runtime behavior changed.
```

---

## Self-review checklist for implementer

- [ ] README has only the requested core sections.
- [ ] README still links to `docs/threat-model.md`.
- [ ] README does not hide that Aegis is heuristic and not a sandbox/privilege boundary.
- [ ] Landing install commands match current supported install paths.
- [ ] Landing does not claim `aegis-guard`, `v0.1`, `v0.5.7`, or "signed/immutable" audit logs.
- [ ] No production-needed docs were deleted.
- [ ] Non-production artifacts were removed only after reference checks.
- [ ] Landing bundle was rebuilt after dependency/asset cleanup.
- [ ] `TASKS.md` and `PROJECT_STATE.md` changed only where supported by evidence.
- [ ] All commands in the implementation used `rtk`.

---

## Execution options

Plan complete and saved to `docs/superpowers/plans/2026-06-23-m6-docs-readme-landing-cleanup.md`.

Two execution options:

1. **Subagent-Driven (recommended)** — dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — execute tasks in this session using executing-plans, batch execution with checkpoints.

