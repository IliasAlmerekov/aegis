# Changelog

This changelog records the release documentation state for Aegis. It is intended
to stay aligned with the repository's current docs, release workflow, and
installer behavior.

## v1.0.0

Release documentation for the `v1.0.0` tag / release line.

### Highlights documented for this release line

- The release workflow is configured to produce GitHub Release artifacts for four targets:
  - `x86_64-unknown-linux-gnu`
  - `aarch64-unknown-linux-gnu`
  - `x86_64-apple-darwin`
  - `aarch64-apple-darwin`
- Each binary is produced with a matching `.sha256` sidecar.
- The install path is configured to verify the downloaded checksum before writing to `BINDIR`.
- The current docs state the supported platform matrix and the known
  limitations of the heuristic guardrail model.
- Troubleshooting and recovery guidance exists for install, checksum, and
  rollback failures.

### What is not claimed

- No SBOM is published by the current release workflow.
- No provenance metadata or attestations are generated or attached by the
  current release workflow.
- This release documentation does not claim byte-for-byte reproducible builds
  across all environments.

### Reference docs

- [v1.0.0 release summary](docs/releases/v1.0.0.md)
- [Release and CI guarantees](docs/ci.md)
- [Platform support](docs/platform-support.md)
- [Threat model](docs/threat-model.md)
- [Troubleshooting and recovery](docs/troubleshooting.md)
