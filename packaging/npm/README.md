# @iliasalmerekov/aegis

NPM distribution wrapper for Aegis.

```bash
npm i -g @iliasalmerekov/aegis
```

The package downloads the correct Aegis GitHub Release binary for the current
Linux/macOS x64/arm64 host during `postinstall`, verifies SHA256, and exposes
the `aegis` command.

NPM installs the binary only. It does not edit `.bashrc`, `.zshrc`, Codex
configuration, or Claude configuration. After installation, run
`aegis install-hooks --all` if you want supported agent hooks, or run
`aegis setup-shell` to opt in to shell-proxy mode for tools that launch commands
through `$SHELL -c`.

To undo shell-proxy setup later:

```bash
aegis setup-shell --remove
```

Native Windows shells are not supported; use Aegis from WSL2 on Windows.