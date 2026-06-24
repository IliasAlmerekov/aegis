# @iliasalmerekov/aegis

NPM distribution wrapper for Aegis.

```bash
npm i -g @iliasalmerekov/aegis
```

The package downloads the correct Aegis GitHub Release binary for the current
Linux/macOS x64/arm64 host during `postinstall`, verifies SHA256, and exposes
the `aegis` command.

NPM installs the binary and never edits `.bashrc` or `.zshrc`. As a
convenience, `postinstall` performs **best-effort** agent-hook setup: if a
`~/.claude` or `~/.codex` directory already exists, it runs
`aegis install-hooks --all`. It never creates those directories, and a hook-setup
failure never fails the npm install. Set `AEGIS_NPM_SKIP_HOOKS=1` to opt out.

If no agent directory exists yet, install one of the agents and then run:

```bash
aegis install-hooks --all
```

Claude Code and Codex command execution is protected through these `PreToolUse`
hooks, not through `setup-shell` — those agents ignore a non-bash/zsh `$SHELL`
in their Bash tool, so only the hook intercepts their commands.

To opt in to shell-proxy mode for tools that launch commands through `$SHELL -c`:

```bash
aegis setup-shell
```

To undo shell-proxy setup later:

```bash
aegis setup-shell --remove
```

Native Windows shells are not supported; use Aegis from WSL2 on Windows.