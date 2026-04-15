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

### Cannot write binary / rc file during install

**Why:** Insufficient permissions for `BINDIR` or shell RC path.

**Fix:**

1. Use a writable prefix (`AEGIS_BINDIR=$HOME/.local/bin`).
2. Ensure `~/.bashrc` / `~/.zshrc` writable.
3. Re-run with `AEGIS_REAL_SHELL` and `AEGIS_SHELL_RC` explicitly set when shell detection is wrong.

### Wrapper recursion errors

**Error:** `refusing to wrap ... recursively`

**Why:** `SHELL` already points to Aegis-managed wrapper and real shell is not explicitly provided.

**Fix:**

1. Export the real shell explicitly: `export AEGIS_REAL_SHELL=$(command -v bash)` (or `zsh`).
2. Re-run installer with `AEGIS_REAL_SHELL` set.

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
