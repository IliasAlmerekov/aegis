# Aegis troubleshooting and recovery

This page covers common operational failures and practical recovery steps.

## Installer: quick failure lookup

### `unsupported operating system: Windows`

**Why:** Aegis shell proxy is implemented for Unix-style shells only.

**Fix:**

1. Run inside WSL2 Linux terminal (not native PowerShell/cmd).
2. Confirm `README.md` platform policy and support matrix.

### `checksum verification failed`

**Why:** The downloaded `.sha256` artifact is missing, malformed, or does not match the binary.

**Fix:**

1. Rerun install with stable network access and re-check URL.
2. Confirm the release asset names match your OS/ARCH (`aegis-linux-x86_64`, `aegis-macos-aarch64`, etc.).
3. Verify downloaded files are not changed by intermediaries (proxy/caching/CDN layer).
4. Ensure binary and checksum assets are in sync before retrying.

### `checksum download failed`

**Why:** `curl`/`wget` or checksum download URL is unavailable.

**Fix:**

1. Retry when network is stable.
2. Verify GitHub release assets exist for the selected tag.
3. If this is a temporary registry/CDN issue, try again after a short interval.

### `no supported checksum tool found`

**Why:** Host has neither `sha256sum` nor `shasum`.

**Fix:**

1. Install one of them (`sha256sum` preferred).
2. Re-run install once tool is available.

### Manual checksum verification fails

**Why:** The downloaded release asset and `.sha256` sidecar do not match, or
one of the files was changed in transit.

**Fix:**

1. Re-download both files from the same release tag.
2. Make sure you are checking the checksum file for the exact asset name.
3. Verify with one of the supported commands:
   - `sha256sum -c <asset-name>.sha256`
   - `shasum -a 256 -c <asset-name>.sha256`
4. Do not install the binary until the checksum check passes.

### Cannot write binary / rc file during install

**Why:** Insufficient permissions for `BINDIR` or shell RC path.

**Fix:**

1. Use a writable prefix (`AEGIS_BINDIR=$HOME/.local/bin`).
2. Ensure `~/.bashrc` / `~/.zshrc` writable.
3. Re-run with `AEGIS_REAL_SHELL` and `AEGIS_SHELL_RC` explicitly set when shell detection is wrong.

### `AEGIS_SETUP_MODE and AEGIS_SKIP_SHELL_SETUP are deprecated`

**Why:** The convenience installer no longer supports the old mode-selection
switches. It now performs the global shell-setup path only.

**Fix:**

1. Remove `AEGIS_SETUP_MODE` and `AEGIS_SKIP_SHELL_SETUP` from your install command or environment.
2. If you only want the binary, use the verification-first manual install path from `docs/release-readiness.md`.
3. If you need a custom shell RC file, rerun with `AEGIS_SHELL_RC=/path/to/your/rcfile`.

### `automatic shell setup supports bash and zsh`

**Why:** The convenience installer only knows how to pick an RC file
automatically for `bash` and `zsh`.

**Fix:**

1. Re-run with `AEGIS_SHELL_RC=/path/to/your/rcfile`.
2. If shell detection is wrong because you are already inside an Aegis-managed shell, also set `AEGIS_REAL_SHELL=/path/to/your-real-shell`.
3. If you only want the binary on `PATH`, use the manual install path in `docs/release-readiness.md`.

### `Agent hook setup skipped; no supported agent directories were detected.`

**Why:** The installer checked your `HOME` for supported agent directories
before attempting automatic hook setup, and it did not find a detectable
`~/.claude` or `~/.codex` directory. Aegis skips hook installation when an
agent directory does not exist yet.

**Fix:**

1. Start Claude Code or Codex once so its config directory exists.
2. Re-run `aegis install-hooks --all`.
3. If your current shell does not see `aegis` on `PATH` yet, use the absolute
   path printed by the installer, such as `$HOME/.local/bin/aegis install-hooks --all`.
4. If you only want to verify the installed binary path, run `command -v aegis`.

### Wrapper recursion errors

**Error:** `refusing to wrap ... recursively`

**Why:** `SHELL` already points to Aegis-managed wrapper and real shell is not explicitly provided.

**Fix:**

1. Export the real shell explicitly: `export AEGIS_REAL_SHELL=$(command -v bash)` (or `zsh`).
2. Re-run installer with `AEGIS_REAL_SHELL` set.

## Audit integrity verification

### `aegis audit --verify-integrity` fails

**Why:** The integrity chain was never enabled, a rotated segment is missing,
or the log files were altered.

**Fix:**

1. Confirm `[audit] integrity_mode = "ChainSha256"` was enabled before the log
   entries you want to verify were written.
2. Make sure the active audit file and any rotated archives are present.
3. Re-run `aegis audit --verify-integrity` against the full log set.
4. Treat the failure as a sign that the log should not be trusted until you
   inspect the files.

## Uninstall recovery

If wrapper state becomes inconsistent:

- `curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/uninstall.sh | sh`
- Check `~/.bashrc` / `~/.zshrc` cleanup: only Aegis-managed block should be removed.
- Confirm binary removal: `command -v aegis` no longer points to managed path.

Then reinstall cleanly.

## Rollback / snapshot failures

`aegis rollback` is strict and may fail by design when invariants are not met.

### Common messages and next steps

- `snapshot not found`
- malformed snapshot ID
- `rollback conflict` or stash-related conflict path
- manifest/config mismatch

For these cases:

1. Check audit trail for a clear snapshot id:
   `aegis audit --risk Danger --format json`.
2. Do not rerun destructive commands blindly after rollback denial.
3. In conflict cases, inspect repository state (`git status`, open files, git logs) before manual recovery.

## References

- `docs/platform-support.md`
- `README.md`
- `docs/ci.md`
- `docs/threat-model.md`
