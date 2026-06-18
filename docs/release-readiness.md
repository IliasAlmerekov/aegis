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

- [ ] `README.md`, `docs/*`, and release notes agree on Aegis being a
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
- [ ] Supported platforms are stated clearly and match the shipped binaries.
- [ ] Threat-model and limitation language is visible, honest, and easy to
      find.

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

## Homebrew tap validation

- [ ] `packaging/homebrew/Formula/aegis.rb` was generated from the selected
      GitHub Release tag.
- [ ] The published tap contains the same `Formula/aegis.rb`.
- [ ] `brew audit --strict --online --formula aegis` passes in the tap.
- [ ] `brew install IliasAlmerekov/aegis/aegis` succeeds on macOS.
- [ ] `brew install IliasAlmerekov/aegis/aegis` succeeds on Linux.
- [ ] `brew test IliasAlmerekov/aegis/aegis` passes on both platforms.

Homebrew validation is currently a release-operator smoke test rather than a
default CI job. The required commands are listed above; a gated live test
(`AEGIS_TEST_LIVE_HOMEBREW=1`) lives in `tests/homebrew_live.rs` and can be
run manually where Homebrew is available. The manual
`.github/workflows/homebrew-live.yml` workflow can run the same gated test on
GitHub-hosted Linux and macOS runners after the tap is published.

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

The current runtime default remains `integrity_mode = "Off"`, which is still
acceptable for lower-trust or low-overhead setups. `ChainSha256` makes audit
segments tamper-evident by chaining SHA-256 hashes across entries and rotated
files.

After enabling integrity mode, verify the active and rotated logs with:

```bash
aegis audit --verify-integrity
```

## References

- `README.md`
- `docs/config-schema.md`
- `docs/ci.md`
- `docs/troubleshooting.md`
- `docs/threat-model.md`
- `docs/releases/current-line.md`
