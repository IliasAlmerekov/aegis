# Aegis Launch Readiness Plan

> Status: proposed roadmap based on repository review performed on 2026-04-16

## Goal

Bring Aegis to:

1. a clean, honest, public MVP release;
2. then a more credible production/security-ready state.

## Executive Summary

Aegis already has strong fundamentals:

- solid Rust code quality;
- strong testing culture;
- explicit threat-model thinking;
- good CI and release hygiene;
- performance and fuzzing checks already wired in.

The main gaps are not basic correctness. They are:

- documentation drift and release-positioning inconsistency;
- oversized modules that are getting harder to maintain safely;
- a security/release trust story that is not yet strong enough for serious security-product positioning;
- a snapshot feature surface that is broader than its current maturity story.

---

## Phase 0 — Lock Product Positioning

### Why

The repository currently reads partly like an MVP and partly like a mature security product.
That ambiguity should be removed before launch.

### Decision

Recommended near-term positioning:

**Aegis v1 = public MVP / practical shell guardrail for AI agents**

Not:

- a sandbox;
- a hard security boundary;
- a fully mature enterprise security product.

### Actions

- choose a single release posture for the next public release;
- align all user-facing language to that posture in:
  - `README.md`
  - `SECURITY.md`
  - `CHANGELOG.md`
  - release notes
  - repository description / marketing copy

### Exit Criteria

- the same maturity claim appears consistently across docs;
- limitations are stated clearly and without overpromising.

### Blocker Level

**Release blocker**

---

## Phase 1 — Eliminate Documentation Drift

### Why

The review found internal inconsistencies that reduce trust:

- `Cargo.toml` says `0.2.0`;
- `SECURITY.md` still says “pre-1.0”;
- `CONVENTION.md` now uses roadmap language instead of a missing backlog
  concept;
- repository instructions reference `RTK.md`, now present as the root helper
  doc.

### Actions

- update `SECURITY.md` to match the current versioning and release posture;
- remove, replace, or restore broken references to:
  - roadmap/backlog language
  - `RTK.md`
- review and align these documents:
  - `README.md`
  - `docs/config-schema.md`
  - `docs/threat-model.md`
  - `docs/platform-support.md`
  - `docs/ci.md`
  - `docs/releases/current-line.md`
  - `docs/releases/v1.0.0.md`
  - `CHANGELOG.md`

### Exit Criteria

- no missing referenced contract files remain;
- version/maturity language is consistent across all top-level docs;
- documented behavior matches the current implementation and CI setup.

### Blocker Level

**Release blocker**

---

## Phase 2 — Separate “Launchable” from “Security-Grade”

### Why

The project needs two different gates:

1. what is enough for a public v1 launch;
2. what is enough for serious security-tool positioning.

### Actions

Create two explicit checklists.

#### A. Minimum Launch Checklist

- tests, lint, audit, deny, bench policy pass;
- docs are internally consistent;
- release workflow works on a real tag;
- checksums are published and verifiable;
- install/uninstall flow is validated;
- threat model is honest and visible;
- supported platforms are clearly documented.

#### B. Security-Grade Checklist

- SBOM generation;
- provenance / attestations;
- artifact signing;
- verification-first install path;
- stronger platform validation;
- mature snapshot-provider maturity policy;
- stronger default audit-integrity guidance.

### Exit Criteria

- MVP blockers and long-term hardening items are tracked separately;
- roadmap decisions stop mixing “nice to have” with real release blockers.

### Blocker Level

**Management blocker before release execution**

---

## Phase 3 — Architectural Cleanup of Oversized Modules

### Why

The main maintainability risk is file growth, not lack of features.

High-risk large files include:

- `src/main.rs`
- `src/audit/logger.rs`
- `src/config/model.rs`
- `src/ui/confirm.rs`
- parts of `src/snapshot/`

### Priority 1 — Shrink `src/main.rs`

#### Problem

`main.rs` is no longer thin. It contains too much command handling and orchestration detail.

#### Actions

Split into focused handlers/modules, for example:

- shell-wrapper execution flow;
- JSON output rendering;
- audit subcommand handling;
- config subcommand handling;
- rollback command handling;
- exit-code and reporting helpers.

#### Target State

`main.rs` should primarily:

- parse CLI input;
- initialize runtime;
- dispatch to handlers.

### Priority 2 — Decompose `src/audit/logger.rs`

#### Suggested split

- `entry.rs`
- `timestamp.rs`
- `integrity.rs`
- `rotation.rs`
- `query.rs`
- `writer.rs`

### Priority 3 — Decompose `src/config/model.rs`

#### Suggested split

- schema types / enums;
- config loading;
- layer merge logic;
- validation;
- serialization / init template helpers.

### Priority 4 — Decompose `src/ui/confirm.rs`

#### Suggested split

- rendering;
- prompt flow;
- tty / non-tty handling;
- highlighting;
- block / deny presentation.

### Exit Criteria

- critical modules become easier to reason about in isolation;
- architecture matches the project’s own “focused modules, thin main” contract more closely;
- future security-sensitive changes become safer to review.

### Blocker Level

**Not a hard MVP blocker, but a strong blocker for long-term maintainability**

---

## Phase 4 — Rationalize Snapshot Scope and Maturity

### Why

Snapshot support is broad:

- Git
- Docker
- PostgreSQL
- MySQL
- SQLite
- Supabase

That is powerful, but it expands maintenance burden and security-review surface.

### Actions

- classify providers by maturity:
  - **core**: Git, Docker
  - **extended**: PostgreSQL, SQLite
  - **advanced / experimental**: MySQL, Supabase
- document for each provider:
  - applicability rules;
  - guarantees;
  - rollback caveats;
  - known failure modes.

### Exit Criteria

- users can tell which snapshot providers are battle-tested and which are still advancing;
- the docs reflect real maturity rather than feature count alone.

### Blocker Level

**Important before stronger security/product positioning**

---

## Phase 5 — Strengthen Release and Supply-Chain Trust

### Why

For a security-adjacent CLI, trust in shipped artifacts matters almost as much as code quality.

### Actions

#### Required before stronger release confidence

- run the release workflow on a real tag;
- verify artifact publication end-to-end;
- verify checksum generation and validation end-to-end.

#### Next-level hardening

- add SBOM generation;
- add provenance metadata / attestations;
- add artifact signing;
- document the verification path clearly.

### Exit Criteria

- release pipeline is exercised, not only defined;
- users have a trustworthy validation path for downloaded binaries.

### Blocker Level

- **real tag + checksum validation:** release blocker
- **SBOM / signing / attestations:** blocker for serious security-grade positioning

---

## Phase 6 — Improve the Install Story

### Why

The current install path is convenient, but a security-focused tool should not rely only on `curl | sh`.

### Actions

- keep the current fast installer path;
- add a first-class manual verification path:
  - download release artifact;
  - download `.sha256`;
  - verify checksum;
  - install manually;
- surface both paths clearly in `README.md` and release docs.

### Exit Criteria

- the project supports both convenience install and verification-first install;
- the secure/manual path is easy to find and officially documented.

### Blocker Level

**Important for launch quality; stronger blocker for security-oriented positioning**

---

## Phase 7 — Promote Audit Integrity to a Recommended Secure Mode

### Why

Audit logging is one of the project’s core trust surfaces, but integrity mode is still optional and not strongly foregrounded.

### Actions

- recommend `ChainSha256` in docs and config examples;
- add a “secure default profile” example;
- update troubleshooting and audit docs to explain when integrity verification should be used.

### Exit Criteria

- operators can easily discover and adopt tamper-evident audit mode;
- audit integrity becomes part of the normal deployment story.

### Blocker Level

**Not an MVP blocker, but strongly recommended**

---

## Phase 8 — Turn Platform Claims into Verified Claims

### Why

Linux and macOS support look credible. WSL is documented as best-effort rather than explicitly validated.

### Actions

- add smoke validation for supported Linux and macOS release targets;
- decide whether WSL should:
  - remain best-effort only, or
  - receive explicit validation coverage.

### Exit Criteria

- platform-support claims are based on repeatable checks, not just assumptions;
- WSL language matches actual validation reality and avoids overclaiming
  validation that does not exist.

### Blocker Level

**Important if platform claims remain prominent in the README**

---

## Phase 9 — Final Launch Gates

### Public MVP Launch Gate

Before public launch, all of the following should be true:

- [ ] release posture is explicit and consistent;
- [ ] documentation drift is resolved;
- [ ] release workflow has been validated on a real tag;
- [ ] checksums are published and verified;
- [ ] install/uninstall flow is validated end-to-end;
- [ ] CI is green;
- [ ] benchmark policy is green;
- [ ] fuzzing job is green;
- [ ] platform claims are accurate and documented.

### Security-Grade Positioning Gate

Before presenting Aegis as a serious mature security tool, additionally require:

- [ ] oversized core modules are decomposed;
- [ ] snapshot provider maturity is explicitly classified;
- [ ] SBOM exists;
- [ ] provenance / attestations exist;
- [ ] artifacts are signed;
- [ ] secure install path is first-class;
- [ ] audit integrity is part of the recommended deployment path.

---

## Priority Order

### P0 — Do Immediately

1. lock release positioning;
2. fix documentation drift;
3. validate release on a real tag;
4. validate checksum and install verification flow;
5. ensure public docs tell the same story.

### P1 — Do Next

6. split `src/main.rs`;
7. split `src/audit/logger.rs`;
8. split `src/config/model.rs`;
9. split `src/ui/confirm.rs`.

### P2 — Trust and Hardening

10. classify snapshot providers by maturity;
11. add platform smoke validation;
12. add SBOM;
13. add provenance / attestations;
14. add artifact signing;
15. promote a verification-first install story and secure audit defaults.

---

## Recommended Launch Strategy

### Recommended Near-Term Strategy

Launch Aegis as:

**“A practical shell guardrail for AI agents.”**

That means:

- release it publicly;
- keep the docs honest;
- avoid claiming it is a sandbox or a strong security boundary;
- treat supply-chain and architectural hardening as the next milestone, not as invisible assumptions.

### Short Version

If the goal is a fast, credible launch:

- finish **P0** first;
- ship as a well-documented public v1 / MVP;
- then execute **P1** and **P2** before stronger security-product positioning.

---

## Suggested Next Step

Convert this roadmap into an execution plan with:

- owners;
- target files;
- estimated effort;
- blocker severity;
- ordering over 1–2 weeks.
