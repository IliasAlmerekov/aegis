# Aegis Roadmap to v1.0.0

Aegis already has the shape of a strong early public release. The roadmap to
`v1.0.0` is not a feature wishlist; it is a maturity plan focused on
trustworthiness, security honesty, operational reliability, and release
discipline.

Until `v1.0.0`, Aegis should be treated as an evolving pre-1.0 tool:

- breaking changes may still happen between minor releases when they improve
  safety, correctness, or contract clarity
- such changes must be called out explicitly in release notes and migration docs
- new feature work should not weaken interception, approval, snapshot, audit, or
  rollback guarantees

Platform position before `v1.0.0`:

- Linux and macOS remain the supported platforms for the path to `v1.0.0`
- native Windows shell support is explicitly **out of scope** for `v1.0.0`
- Windows-specific interception can be revisited after the Unix-first contract is
  stable

---

## v0.2.0 — Hardening Foundation

**Goal:** Strengthen the core safety model so parser, scanner, snapshot, rollback,
and audit behavior are harder to break accidentally and easier to trust under
adversarial or unusual inputs.

**Key deliverables:**

- add and maintain fuzzing coverage for parser and scanner behavior
- expand regression coverage for heredocs, quoting, pipes, multiline commands,
  subshells, and indirect execution edge cases
- harden snapshot and rollback invariants across providers, especially path
  safety, integrity checks, and fail-closed behavior
- document current limitations, non-goals, and known bypass classes with tighter
  alignment to the actual implementation
- ensure performance-sensitive parser/scanner behavior has a maintained baseline

**Exit criteria:**

- parser/scanner fuzz targets exist and run in normal engineering workflow
- critical shell parsing edge cases have explicit regression tests
- snapshot and rollback failure paths have stronger regression coverage
- threat model, limitations, and public docs no longer overstate behavior
- benchmark-sensitive paths have an explicit baseline and documented budget

**Out of scope:**

- new major snapshot provider families
- dashboard, notifications, remote control surfaces
- native Windows shell interception

---

## v0.3.0 — Release Engineering and Compatibility Contracts

**Goal:** Make releases easier to trust by tightening artifact integrity,
installation quality, and explicit compatibility promises.

**Key deliverables:**

- produce reproducible release artifacts where practical
- publish and verify checksums for release artifacts
- add signing, attestations, or equivalent artifact trust signals when the
  release pipeline supports them reliably
- validate install, upgrade, and uninstall flows end-to-end
- formalize compatibility contracts for config schema, audit log format, and exit
  codes
- document supported platforms, targets, and shell assumptions clearly
- validate `cargo publish --dry-run` and packaging/release pipeline behavior

**Exit criteria:**

- release artifacts are consistently built, checksumed, and verifiable
- install and upgrade flows are tested and documented
- compatibility promises for config, audit log, and exit codes are written down
- platform-support documentation matches actual tested behavior
- release checklist exists and is usable by someone other than the project author

**Out of scope:**

- large new product surfaces
- policy DSL redesign
- team workflow integrations

---

## v0.4.0 — Operational Maturity and Stabilization

**Goal:** Turn Aegis from a well-built pre-1.0 tool into a stable release
candidate with clearer operator experience, stronger failure handling, and a
deliberate stabilization window.

**Key deliverables:**

- improve diagnostics and user-facing remediation for config, snapshot, rollback,
  audit, and CI failures
- expand end-to-end coverage for low-disk, missing-binary, permission-denied,
  interrupted-process, and corrupted-artifact scenarios
- enforce performance regression checks for hot paths
- strengthen audit and snapshot behavior under operational stress
- review docs for operator workflows, troubleshooting, and recovery
- start a **feature freeze** for `v1.0.0`: no new roadmap-sized features, only
  stabilization, hardening, compatibility cleanup, and documentation

**Exit criteria:**

- major failure modes have clear diagnostics and tested recovery behavior
- end-to-end tests cover core operational failure paths
- performance regression checks are part of normal release validation
- troubleshooting and recovery documentation are publishable
- feature freeze is in effect and post-freeze work is limited to stabilization

**Out of scope:**

- roadmap-sized new features
- new platforms
- product-surface expansion unrelated to v1 stabilization

---

## v1.0.0 — Production-Ready Release

**Goal:** Ship a production-ready Unix-first Aegis release with explicit support
contracts, hardened core behavior, trusted release artifacts, and honest security
documentation.

**Key deliverables:**

- close all `v1.0.0` release gates listed below
- publish stable support, compatibility, and limitations documentation
- ship a release that is installable, auditable, and maintainable by users who do
  not know the codebase internals
- complete a final stabilization pass with no known high-severity correctness or
  fail-open regressions

**Exit criteria:**

- every release gate below is satisfied
- documentation reflects actual product behavior with no major mismatches
- normal install, upgrade, rollback, and audit workflows are validated
- release quality is not dependent on undocumented local knowledge

**Out of scope:**

- post-`v1.0.0` expansion ideas listed in `POST_V1.md`

---

## v1.0.0 Release Gates

The project should not call itself production-ready until all of these are true:

- parser and scanner fuzzing exist, are maintained, cover critical input
  classes, and are exercised in CI or equivalent release validation
- parser/scanner regression coverage includes tricky shell edge cases and known
  historical failures
- safe-path performance expectations are documented and checked against a baseline
- supported platform matrix is documented and tested
- config schema, audit log format, and exit-code compatibility promises are
  documented
- threat model, limitations, and non-goals are current and honest
- snapshot and rollback flows are fail-closed and regression-tested across
  supported providers
- release artifacts are checksumed and verifiable
- install, upgrade, and uninstall flows are documented and validated
- CI, security, and dependency-policy gates pass consistently
- troubleshooting and recovery guidance exists for common operational failures

---

## Beyond v1.0.0

Ideas that may be valuable later, but do **not** block `v1.0.0`, are tracked in
[`POST_V1.md`](POST_V1.md).
