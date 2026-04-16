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
