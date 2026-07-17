# Aegis release readiness

This document separates launch blockers from longer-term security hardening.
It describes requirements to complete, not claims about work already validated.
Do not read any checklist item as done unless a release note or verification
record says so.

## Why two checklists?

Aegis has two different adoption thresholds:

1. **Minimum Launch Checklist** — what must be true before treating the current
   line as a shippable public MVP.
2. **Security-Grade Checklist** — what should be added later for a stronger
   trust posture and release supply-chain story.

Keeping those lists separate avoids mixing launch blockers with worthwhile but
non-blocking hardening work.

## Minimum Launch Checklist

These items are launch blockers for the current public line:

- [x] `README.md`, `docs/*`, and release notes agree on Aegis being a
      heuristic shell guardrail, not a sandbox or hard security boundary.
- [ ] CI exercises the `curl | sh` installer against a real GitHub Release artifact on every supported platform.
- [ ] The convenience installer and troubleshooting paths are documented
      clearly enough for first-time users to complete installation.
- [ ] The release workflow is exercised on a real tag before the release is
      treated as trustworthy.
- [ ] Release artifacts ship with checksum sidecars and users can verify them
      before installation.
- [ ] Install and uninstall guidance is current and matches the shipped
      release assets.
- [x] Supported platforms are stated clearly and match the shipped binaries.
- [x] Threat-model and limitation language is visible, honest, and easy to
      find.

### Language-aware analysis 1.0 gate (ADR-022)

These items remain unchecked until runtime implementation and qualification are
verified; the ADR and implementation plan alone do not satisfy the gate.

- [ ] The additive analysis foundation preserves existing Scanner results,
      starts no worker on the no-source safe path, and keeps that path under
      2 ms.
- [ ] The ephemeral parsing worker, source reader, recursive queue, typed
      degradation, Policy/TUI/Watch/Hook/CI behavior, and Audit v1/v2
      compatibility pass their production qualification matrix.
- [ ] Python is production-qualified and included in every official release
      target.
- [ ] JavaScript is production-qualified and included in every official release
      target.
- [ ] TypeScript is production-qualified and included in every official release
      target.
- [ ] Shell/Bash is production-qualified and included in every official release
      target.
- [ ] The pinned Tree-sitter runtime and grammar manifest pass license,
      supply-chain, ABI, corpus, fuzz, binary-size, memory, latency, and all-four-
      target release gates.
- [ ] Documentation states the residual dynamic-code, dependency, encoding,
      TOCTOU, privacy, and unsupported-language limits without implying program
      verification or sandbox guarantees.

## GitHub Release asset validation

- [ ] `.github/workflows/release.yml` includes all four supported release assets:
      `aegis-linux-x86_64`, `aegis-linux-aarch64`, `aegis-macos-x86_64`,
      and `aegis-macos-aarch64`.
- [ ] The release workflow publishes a matching `.sha256` sidecar for each asset.
- [ ] `rtk cargo test --test release_workflow` passes.
- [ ] `rtk env AEGIS_TEST_LIVE_RELEASE=1 AEGIS_TEST_RELEASE_TAG=vX.Y.Z cargo test --test release_assets_live -- --nocapture`
      passes against the selected real tag.
- [ ] Every downloaded sidecar verifies its matching binary with `sha256sum -c`
      or `shasum -a 256 -c`.

The live release test is gated by `AEGIS_TEST_LIVE_RELEASE=1` so default
`rtk cargo test` stays network-free. M3.5 is not complete until this live check
passes against the tag being used for installer, Homebrew, and npm checksum
updates.

### Evidence recorded 2026-06-22 (release v0.5.6)

`rtk cargo test --test release_workflow`: PASS (9 tests, including the binary +
`.sha256` publication contract). `rtk env AEGIS_TEST_LIVE_RELEASE=1
AEGIS_TEST_RELEASE_TAG=v0.5.6 cargo test --test release_assets_live --
--nocapture`: PASS. The GitHub Release contains all four supported binaries and
all four `.sha256` sidecars; each sidecar verifies its matching binary.

## Security-Grade Checklist

These items are not launch blockers, but they matter for a more security-grade
posture later:

- [ ] SBOM generation or equivalent supply-chain metadata.
- [ ] Provenance or attestation support for release artifacts.
- [ ] Artifact signing, if and when the release pipeline supports it.
- [ ] Stronger platform and installer validation beyond the basic checksum
      flow.
- [ ] Clear maturity policy for snapshot providers and their rollback limits.
- [ ] Stronger default guidance for audit integrity and log verification.

## Live installer validation

The convenience installer is exercised end-to-end in CI on `ubuntu-latest` and `macos-latest` by the `live-installer` job. The test downloads the latest GitHub Release asset for the host platform, verifies the SHA-256 sidecar, installs the binary into a temporary `BINDIR`, and asserts that `aegis --version` succeeds. This job is gated in the test suite by the `AEGIS_TEST_LIVE_INSTALL=1` environment variable so default `cargo test` remains network-free.


## Snapshot/rollback live backend validation

The `Live snapshot/rollback (Docker + SQLite)` CI job closes M5.3 by exercising snapshot and rollback against real backends on `ubuntu-latest`.

- Docker coverage runs `tests/docker_integration.rs::snapshot_rollback_reverts_filesystem_change` with `AEGIS_DOCKER_TESTS=1` after pulling the `alpine` fixture image.
- SQLite coverage runs `tests/snapshot_rollback_live.rs::sqlite_snapshot_rollback_restores_database_file_through_aegis_cli` with `AEGIS_SQLITE_SNAPSHOT_TESTS=1` after installing the real `sqlite3` CLI.
- The SQLite test uses the Aegis CLI end-to-end: a Danger-shaped allowlisted command creates a pre-execution SQLite snapshot, mutates the database through `sqlite3`, rolls back by the audit-recorded snapshot id, and verifies the post-rollback database contents.

## Supply-chain gate evidence

### Evidence recorded 2026-06-23

- `rtk cargo audit`: exits 0. 4 unmaintained warnings visible in the full
  `Cargo.lock` output (RUSTSEC-2023-0089, RUSTSEC-2024-0388, RUSTSEC-2025-0057,
  RUSTSEC-2024-0436), all routing through `starlark 0.14.2`, which is reachable
  only via the optional `starlark-policy` feature. These warnings are not present
  in the default-feature build.
- `rtk cargo deny check`: exits 0, **zero warnings**. The default-feature graph
  excludes optional crates, so no advisory, ban, license, or source findings arise.
- CI security job updated from `cargo deny check bans licenses sources` to
  `cargo deny check` (full check, including advisories), matching the local gate.

### Release binary behavior (policy.star)

Published release binaries are built without `--features starlark-policy`.
This means:

- The `starlark-policy` feature is **not included** in official releases.
- If a user has `~/.aegis/policy.star`, the release binary will return a clear
  `AegisError::Config` at startup rather than silently ignoring the policy (fail-closed).
- Users who need Starlark policy support must build from source with
  `cargo install --features starlark-policy`.

This is an intentional product decision: the default supply-chain gate is clean,
and users who opt into the advisory-tainted dependency chain do so explicitly.

## Homebrew tap validation

- [ ] `packaging/homebrew/Formula/aegis.rb` was generated from the selected
      GitHub Release tag.
- [ ] The published tap contains the same `Formula/aegis.rb`.
- [ ] `brew audit --strict --online --formula aegis` passes in the tap.
- [ ] `brew install IliasAlmerekov/aegis-shellguard/aegis` succeeds on macOS.
- [ ] `brew install IliasAlmerekov/aegis-shellguard/aegis` succeeds on Linux.
- [ ] `brew test IliasAlmerekov/aegis-shellguard/aegis` passes on both platforms.

Homebrew validation is currently a release-operator smoke test rather than a
default CI job. The required commands are listed above; a gated live test
(`AEGIS_TEST_LIVE_HOMEBREW=1`) lives in `tests/homebrew_live.rs` and can be
run manually where Homebrew is available.

### Evidence recorded 2026-06-22 (Linux x64 / WSL2, release v0.5.6)

Network-free formula contract suite `tests/homebrew_formula.rs`: PASS — confirms
all four platform assets, four 64-hex SHA256 pins pinned to the v0.5.6 release,
raw-binary `using: :nounzip`, no shell rc mutation, `test do`, caveats, and
release-readiness runbook coverage. `packaging/homebrew/Formula/aegis.rb` version
and SHA256 values match `packaging/npm/checksums.json` for the same release tag.

Live Linux x64 Homebrew evidence was collected on 2026-06-22 against the public
tap after fixing the tap repository's line-ending policy
(`c209468 fix: force LF line endings for formulae`). A clean retap produced
`Formula/aegis.rb: Ruby script, ASCII text`; `brew audit --strict --online
--formula IliasAlmerekov/aegis-shellguard/aegis` exited 0; `brew install
IliasAlmerekov/aegis-shellguard/aegis` installed v0.5.6; `brew test
IliasAlmerekov/aegis-shellguard/aegis` passed; and
`/home/linuxbrew/.linuxbrew/opt/aegis/bin/aegis --version` printed
`aegis 0.5.6`.

macOS Homebrew smoke is still an operator follow-up. M3.3 is accepted as closed
for this release pass based on the published formula, Linux clean-retap smoke,
and the release asset/checksum contract that covers both macOS assets.

## Homebrew tap publish runbook

Operator runbook for closing M3.3 Task 6. Run every step on release; the
formula is generated deterministically by `scripts/update-homebrew-formula.sh`
so the source-of-truth file is `packaging/homebrew/Formula/aegis.rb`.

1. Regenerate the formula from the release tag (idempotent):

   ```bash
   scripts/update-homebrew-formula.sh vX.Y.Z
   ```

2. Create the tap repository once (skip if it already exists):

   ```bash
   gh repo create IliasAlmerekov/homebrew-aegis --public --description "Homebrew tap for Aegis"
   ```

3. Clone the tap and lay out the formula under `Formula/`:

   ```bash
   git clone git@github.com:IliasAlmerekov/homebrew-aegis.git /tmp/homebrew-aegis
   mkdir -p /tmp/homebrew-aegis/Formula
   cp packaging/homebrew/Formula/aegis.rb /tmp/homebrew-aegis/Formula/aegis.rb
   ```

4. Audit the formula inside the tap:

   ```bash
   cd /tmp/homebrew-aegis
   brew audit --strict --online --formula aegis
   ```

   Expected: `0 problems`. Fix any style issue in
   `packaging/homebrew/Formula/aegis.rb` first, regenerate, and re-copy so the
   source repo and the tap stay in sync.

5. Commit and push the tap:

   ```bash
   git add Formula/aegis.rb
   git commit -m "aegis X.Y.Z"
   git push origin main
   ```

6. Smoke-test the public install on macOS and on Linux (clean Homebrew prefix):

   ```bash
   brew untap IliasAlmerekov/aegis 2>/dev/null || true
   brew tap IliasAlmerekov/aegis
   brew install aegis
   brew test aegis
   aegis --version
   ```

7. Record evidence for both platforms (macOS and Linux) in the release notes,
   then proceed to the M3.3 completion checklist.

## npm wrapper validation

- [ ] `scripts/update-npm-package.sh vX.Y.Z` regenerated
      `packaging/npm/checksums.json` from the selected release tag.
- [ ] `npm publish --dry-run` succeeds from `packaging/npm`.
- [ ] `npm i -g @iliasalmerekov/aegis` succeeds on Linux x64.
- [ ] `npm i -g @iliasalmerekov/aegis` succeeds on macOS arm64 or x64.
- [x] `aegis --version` prints the selected release version after npm install.
- [x] npm install does not mutate shell startup files or agent config.

### Evidence recorded 2026-06-22 (Linux x64 / WSL2, release v0.5.6)

Network-free contract suite `tests/npm_package.rs`: PASS (8 tests). `npm pack
--dry-run --json ./packaging/npm`: PASS — tarball contains exactly `package.json`,
`README.md`, `checksums.json`, `bin/aegis.js`, `scripts/install.js`,
`scripts/smoke.js`; no `vendor/aegis`, no build artifacts. `npm publish --dry-run`
reports the tarball and `@iliasalmerekov/aegis@0.5.6` metadata but requires
registry login in this environment (recorded as an environment limitation; `npm
pack --dry-run` evidence retained). Skip-download install
(`AEGIS_NPM_SKIP_DOWNLOAD=1 npm install -g --prefix /tmp/... ./packaging/npm`):
PASS — `aegis --version` prints `aegis test binary`, exit 0. Live local-package
install (`npm install -g --prefix /tmp/aegis-npm-live ./packaging/npm`): PASS on
Linux x64 — the installer downloaded `aegis-linux-x86_64` from the v0.5.6 GitHub
Release, followed the GitHub redirect, verified SHA256 against
`checksums.json`, and `aegis --version` printed `aegis 0.5.6` (exit 0). Install
writes only inside the chosen npm prefix; no shell startup files or agent config
files were modified.

Live npm registry evidence was collected on 2026-06-22. The package page shows
`@iliasalmerekov/aegis@0.5.6` as public. Registry metadata was briefly lagging
(`npm view @iliasalmerekov/aegis version` returned E404), but registry install
worked: `npm install -g --prefix /tmp/aegis-npm-registry
@iliasalmerekov/aegis` added one package, and
`/tmp/aegis-npm-registry/bin/aegis --version` printed `aegis 0.5.6` on Linux
x64.

macOS npm smoke is still an operator follow-up. M3.4 is accepted as closed for
this release pass based on the public npm package, Linux registry install smoke,
and the package checksum contract that covers both macOS assets.

A packaging robustness fix was applied during this closeout: `packaging/npm/package.json`
`bin.aegis` was changed from `./bin/aegis.js` to `bin/aegis.js` (the normalized
form npm emits in the published tarball). Previously `npm install -g
./packaging/npm` triggered `npm pkg fix` auto-correction that mutated the source
`package.json` in place and broke `tests/npm_package.rs` on the next `cargo test`.
The normalized form is stable under `npm install` and matches the published
tarball; `tests/npm_package.rs` was updated to assert the same form.

## Cargo install validation

- [ ] `cargo install --git https://github.com/IliasAlmerekov/aegis-shellguard --tag vX.Y.Z aegis`
      succeeds on a clean machine with a current stable Rust toolchain.
- [ ] `aegis --version` prints the selected release version after Cargo install.
- [ ] crates.io publication, if enabled, is handled as a separate release
      checkpoint because the root crate depends on internal workspace crates.

npm wrapper validation is currently a release-operator smoke test rather than a
default CI job. Network-free contract tests live in `tests/npm_package.rs` and
the gated live test (`AEGIS_TEST_LIVE_NPM=1`) lives in `tests/npm_live.rs`; both
keep default `cargo test` network-free. Cargo support for M3.4 is the documented
`cargo install --git` source-build path; crates.io publication remains a
separate human-controlled release checkpoint.

## Verification-first manual install path

Use this path when you want to validate a release asset before installing it.
It is intentionally generic so it still makes sense for the current pre-1.0
line.

1. Download the release asset for your platform from the release page.
2. Download the matching `.sha256` sidecar from the same release.
3. Verify the checksum with the tool available on your system:

   ```bash
   sha256sum -c <asset-name>.sha256
   # or
   shasum -a 256 -c <asset-name>.sha256
   ```

   This verifies the downloaded binary against the checksum sidecar published
   with the same release asset. It proves integrity of the file you downloaded,
   but it does **not** authenticate the publisher or provide signature /
   attestation verification. Those artifacts are not published yet.

4. If verification passes, make the binary available on your `PATH`.
   For example, on Linux x86_64:

   ```bash
   asset=aegis-linux-x86_64
   mkdir -p "$HOME/.local/bin"
   chmod +x "./$asset"
   mv "./$asset" "$HOME/.local/bin/aegis"
   export PATH="$HOME/.local/bin:$PATH"
   ```

   Replace `aegis-linux-x86_64` with your platform asset name, such as
   `aegis-macos-aarch64`. Add the `PATH` line to your shell profile if you
   want it to persist.
5. Make your shell or agent use the installed binary:

   - Claude Code: run `command -v aegis`, then paste the absolute path it
     prints into the `shell` setting
   - shell-based launchers that honor `$SHELL`: export
     `SHELL=/absolute/path/to/aegis` and
     `AEGIS_REAL_SHELL=/absolute/path/to/your-real-shell` in your shell
     profile, or start them from a shell where both values already point to
     the installed binary and the preserved real shell respectively

6. If you want the convenience wrapper behavior too, follow the wrapper
   instructions in `README.md` or use the convenience installer path instead.
   The convenience installer now performs the global shell-setup path only and
   rejects the removed `AEGIS_SETUP_MODE` / `AEGIS_SKIP_SHELL_SETUP` controls.
   Automatic shell setup recognizes `bash` and `zsh`; for another shell or a
   custom rc file, rerun with `AEGIS_SHELL_RC=/path/to/your/rcfile` (and set
   `AEGIS_REAL_SHELL` too if you are already inside an Aegis-managed shell).

If the checksum does not match, stop and re-download both files from the same
release. Do not install a binary whose checksum you could not verify.

## Audit integrity guidance

For security-conscious deployments, prefer chained audit integrity:

```toml
[audit]
rotation_enabled = true
integrity_mode = "ChainSha256"
```

The runtime default is `ChainSha256`. It links audit segments with SHA-256
hashes to detect corruption and inconsistent edits; it has no keyed or remote
anchor. Set `integrity_mode = "Off"` only when intentionally opting out of
integrity checks.

After enabling integrity mode, verify the active and rotated logs with:

```bash
aegis audit --verify-integrity
```

## Fuzz CI validation

- M5.2 is covered by `.github/workflows/ci.yml` job `fuzz`, which runs
  `parser`, `scanner`, and `heredoc` fuzz targets with `-runs=100000`.
- Corpora are committed under `fuzz/corpus/parser`, `fuzz/corpus/scanner`, and
  `fuzz/corpus/heredoc`.
- `tests/fuzz_ci.rs` asserts the CI wiring and committed corpus contract.

## References

- `README.md`
- `docs/config-schema.md`
- `docs/ci.md`
- `docs/troubleshooting.md`
- `docs/threat-model.md`
- `docs/releases/current-line.md`
