# Installer Checksum Verification Design

**Date:** 2026-04-14
**Status:** Approved in chat, pending written-spec review

## Objective

Close the release-blocking trust-chain gap in the Aegis installer by making
checksum verification mandatory before installation and by documenting a
verification-first install path in `README.md`.

This is a narrow pre-release hardening change. It should close the release gate
already documented in `CONVENTION.md` without turning into a broader release UX
or supply-chain redesign.

## Scope

### In scope

1. **Mandatory installer checksum verification**
   - `scripts/install.sh` downloads the target binary and its matching
     `.sha256` sidecar
   - the installer verifies the binary before writing the final binary into
     `BINDIR`
   - the checksum path is fail-closed

2. **Verifier-chain contract**
   - preferred verifier: `sha256sum`
   - fallback verifier: `shasum -a 256`
   - if neither exists, installation fails

3. **Installer regression coverage**
   - success case
   - checksum mismatch
   - missing checksum
   - missing verifier tools
   - successful fallback from missing `sha256sum` to available `shasum`

4. **Verification-first README update**
   - make manual checksum verification the recommended install path
   - keep `curl | sh` as a quick install path
   - explicitly document that quick install performs mandatory checksum
     verification and fails closed on verification errors

### Out of scope

This P0 does **not** include:

- signatures, provenance, or attestations
- release workflow redesign
- changes to release asset naming or release trigger semantics
- Windows installer support
- additional verifier backends such as `openssl`
- insecure bypass modes such as `--insecure`, `continue anyway`, or
  `skip verify`
- follow-up roadmap work beyond the minimum doc changes needed for this gate

## Problem Statement

The current installer trust chain is incomplete in a release-critical way:

- `scripts/install.sh` downloads the binary used by the main install path
- `.github/workflows/release.yml` already publishes `.sha256` files for each
  release asset
- `README.md` currently promotes `curl | sh` as the primary install path
- `CONVENTION.md` already lists checksum verification and a
  verification-first install path as release-readiness gates

Without checksum verification in the installer, the practical trust chain stops
at the most common installation step. That makes this a release blocker for the
first trustworthy public release.

## Non-Goals

This work does **not** attempt to:

- solve the full supply-chain story for Aegis
- guarantee artifact authenticity beyond checksum matching
- claim reproducible builds or signed releases
- redesign shell-wrapper setup or uninstall behavior
- add future-facing abstraction beyond what improves readability in
  `scripts/install.sh`

## Design Decisions

### 1. Verification is mandatory and happens before final install

The installer must:

1. resolve the target asset name as it does today
2. derive the matching `.sha256` sidecar URL for that exact asset
3. download both into a temporary directory
4. verify the downloaded binary against the downloaded checksum
5. install into `BINDIR` only after verification succeeds

Writing into the temporary directory is allowed before verification. Writing the
final binary into the target path under `BINDIR` is not.

### 2. The checksum must match the exact requested asset

The `.sha256` file used by the installer must correspond to the exact binary
asset selected for the resolved OS, architecture, and version.

The contract is not “some checksum file exists nearby.” The contract is “the
checksum sidecar for this exact release asset is present and passes
verification.”

### 3. The checksum stage is strictly fail-closed

The installer must stop with a non-zero exit before final installation when any
of the following occur:

- binary download fails
- checksum download fails
- no supported checksum verifier tool exists
- checksum verification fails

There is no warning-only path and no continue-anyway mode. This is a real trust
gate, not advisory output.

### 4. Verifier selection stays intentionally narrow

For this P0, the verifier chain is:

1. `sha256sum`
2. `shasum -a 256`
3. hard fail if neither is available

`openssl` is intentionally excluded to keep the shell implementation narrow and
reliable across Linux and macOS.

### 5. Verification implementation should optimize for reliability, not cleverness

The source of truth remains the existing published `.sha256` file from the
release workflow. The installer may verify it in either of two acceptable ways:

- use a standard checksum verification command if its behavior is stable for the
  published checksum-file format
- read the expected digest from the checksum file, compute the actual digest,
  and compare them directly

The implementation should choose the simplest reliable path without changing the
release artifact contract just to make the installer easier to write.

### 6. Error messages should be explicit and stable enough to test by substring

Checksum-stage failures should be written clearly to `stderr` and distinguish at
least these classes of failure:

- binary download failed
- checksum download failed
- no supported checksum tool found
- checksum verification failed

Tests should assert stable substrings rather than requiring the entire error
message to remain byte-for-byte identical.

### 7. Existing shell-setup behavior is preserved

This change must not redesign or weaken the rest of the installer flow:

- OS / architecture resolution stays the same
- release asset naming stays the same
- shell wrapper setup logic stays the same
- uninstall flow stays the same

The only intended semantic change is that installer trust verification becomes
mandatory before final installation.

### 8. README should become verification-first without removing quick install

`README.md` should present installation in this order:

1. **Recommended / verification-first install**
   - download the binary
   - download the matching `.sha256`
   - verify the binary manually with `sha256sum` or `shasum -a 256`
   - install the verified binary

2. **Quick install**
   - keep `curl | sh`
   - explicitly state that the installer downloads the binary and `.sha256`,
     verifies them before install, and fails closed on missing checksum,
     mismatch, or missing verifier tools

The manual path should be truly manual, not merely a different way to invoke the
installer.

## Implementation Shape

### Files expected to change

- `scripts/install.sh`
- `tests/installer_flow.rs`
- `README.md`

### Installer structure

Small helper functions are encouraged when they improve readability, such as:

- selecting the checksum verifier
- downloading the checksum sidecar
- verifying the downloaded binary

These functions should remain local and practical. This is not the place to
build a generic shell framework for future supply-chain features.

## Test Design

`tests/installer_flow.rs` should cover at least these cases:

1. **Success**
   - binary and checksum download successfully
   - verifier exists
   - verification succeeds
   - binary appears in `BINDIR`
   - existing shell-wrapper setup still succeeds

2. **Checksum mismatch**
   - checksum does not match binary
   - installer fails
   - final binary path under `BINDIR` is untouched
   - `stderr` contains a stable checksum-failure substring

3. **Missing checksum**
   - binary download succeeds
   - checksum download fails or is absent
   - installer fails
   - final binary path under `BINDIR` is untouched
   - `stderr` contains a stable checksum-download-failure substring

4. **No supported verifier**
   - neither `sha256sum` nor `shasum` is available
   - installer fails
   - final binary path under `BINDIR` is untouched
   - `stderr` contains a stable no-supported-tool substring

5. **Fallback order**
   - `sha256sum` is unavailable
   - `shasum` is available
   - installer succeeds via fallback

Verifier-related tests should isolate `PATH` so they do not accidentally pass by
using real host tools.

## Portability Decisions

This design intentionally supports only the currently documented Unix-like
release targets and keeps checksum verification narrow and explicit:

- Linux commonly uses `sha256sum`
- macOS commonly uses `shasum -a 256`
- no extra verifier compatibility layer is introduced in this P0

That narrowness is intentional and preferable to a larger but more fragile
implementation.

## Risks and Mitigations

### Risk: fragile `.sha256` parsing

Mitigation:

- keep the release checksum file as the source of truth
- choose the verification strategy that is most reliable for the existing file
  format

### Risk: host-environment leakage in tests

Mitigation:

- isolate `PATH` in verifier tests
- explicitly stub the desired verifier availability per scenario

### Risk: partial install before verification

Mitigation:

- assert in tests that the final binary path in `BINDIR` remains untouched until
  verification passes

### Risk: scope creep into broader supply-chain work

Mitigation:

- explicitly treat signatures, provenance, attestations, and broader release UX
  hardening as follow-up work

## Readiness Criteria

This release gate is closed only when all of the following are true:

- `install.sh` downloads the selected binary and the matching checksum sidecar
- checksum verification is mandatory
- final `BINDIR` install does not happen before verification succeeds
- binary download failure exits non-zero
- checksum download failure exits non-zero
- missing supported verifier tool exits non-zero
- checksum mismatch exits non-zero
- fallback from missing `sha256sum` to available `shasum` succeeds
- shell-wrapper setup still behaves as before after a successful verified install
- `README.md` makes the manual verification-first path the recommended install
  path
- `README.md` still documents `curl | sh` as a quick install path with explicit
  fail-closed verification behavior

## Final Summary

This is a deliberately narrow P0 hardening change:

- it closes the installer trust-chain gap on the main release path
- it matches the release-readiness contract already documented in
  `CONVENTION.md`
- it avoids pretending to solve the broader artifact-authenticity problem
- it is small enough to ship quickly without turning into an open-ended
  hardening project
