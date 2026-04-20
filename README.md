# Aegis

> A shell guard for AI agents. Sits between your agent and the terminal — and asks before anything destructive runs.

[![CI](https://github.com/IliasAlmerekov/aegis/actions/workflows/ci.yml/badge.svg)](https://github.com/IliasAlmerekov/aegis/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-linux%20%7C%20macos%20%7C%20wsl-lightgrey)](docs/platform-support.md)

<p align="center">
  <img src="src/assets/howitwork.png" alt="Aegis intercepts commands before they reach the real shell and classifies them as Safe, Warn, Danger, or Block." width="900" />
</p>

---

## Why Aegis exists

AI coding agents (Claude Code, Codex, Cursor, etc.) are powerful — but they can also delete your database, wipe your files, or break your project in a single command.

This happens more often than you'd think:

- You give the agent **full permission** and just keep pressing Enter without reading what it's doing
- You're **vibe coding** — you don't fully understand what the agent is trying to do, and it drops a table or removes a directory before you notice
- The agent makes a mistake, and there's **nothing between it and your terminal**

**Aegis is that something.** It's a free, open-source shell proxy that sits between the AI agent and your real terminal. Every command the agent tries to run goes through Aegis first. Safe commands pass through instantly. Dangerous commands get stopped, and you see a clear prompt: "This command wants to delete X. Allow it?"

Think of it as a seatbelt for AI-assisted coding. You probably won't crash — but if you do, you'll be glad it was there.

---

## What does Aegis do?

When an AI agent runs a shell command, Aegis intercepts it and decides what to do:

| Level | What happens | Example |
|-------|-------------|---------|
| **Safe** | Runs instantly, no questions asked | `ls`, `echo hello`, `git status` |
| **Warn** | Aegis asks: "Allow this?" | `git push --force`, `npm publish` |
| **Danger** | Aegis asks, and can snapshot for rollback | `rm -rf ./src`, `DROP TABLE users` |
| **Block** | Refused, always | `rm -rf /`, `mkfs.ext4 /dev/sda` |

---

## Install

One command. Works on **Linux**, **macOS**, and **Windows (WSL2)**.

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
```

The installer will:

1. Download the right binary for your system
2. Verify its checksum (so you know it hasn't been tampered with)
3. Set up Aegis globally and, when available from a local checkout, install
   Claude Code / Codex hooks automatically

## Install behavior

- Global: installs the `aegis` binary by default.
- Local: enables shell integration, installs Claude Code / Codex hooks when available from a local checkout, and lets you use `aegis off` / `aegis on` for a temporary toggle.
- Binary: if a pre-built release is unavailable for your platform, use the source install path below instead.
- The installer rejects the removed `AEGIS_SETUP_MODE` and `AEGIS_SKIP_SHELL_SETUP` controls instead of silently ignoring them.

Automatic shell setup currently recognizes `bash` and `zsh`. If you use another
shell or a custom rc file, set `AEGIS_SHELL_RC=/path/to/your/rcfile` before
rerunning the installer. If you are already inside an Aegis shell, also set
`AEGIS_REAL_SHELL` to the path of your real shell.

When disabled, Aegis behaves as though it is not installed for ordinary local shell and supported agent usage. CI ignores the local disabled flag and continues enforcing policy.

### Alternative: install from source

If the pre-built binary is not available for your platform:

```bash
cargo install --git https://github.com/IliasAlmerekov/aegis aegis
```

### Windows

Aegis works on Windows through **WSL2** (Windows Subsystem for Linux). Open a WSL2 terminal and run the install command above. native Windows shells like PowerShell and cmd.exe are not supported.

---

## Verify it works

Open a new terminal to let the global shell integration take effect.

```bash
# Check the binary is installed
aegis --version

# This should show a confirmation prompt — press n to deny
aegis -c 'rm -rf /tmp/aegis-test'

# This should pass through instantly, no prompt
aegis -c 'echo hello'
```

If the first command shows a dialog and the second passes through — Aegis is working.

### Verify routing is active

```bash
echo "$SHELL"             # should print the path to aegis
echo "$AEGIS_REAL_SHELL"  # should print your real shell (e.g. /bin/zsh)
```

---

## Connect to your AI agent

### Claude Code

Claude Code will automatically use Aegis because `$SHELL` already points to it.

If you need to set the shell path manually, paste the output of `command -v aegis` into the `shell` field.

### Codex, Cursor, and other agents

If the agent respects `$SHELL` — it works automatically.

If the agent has its own shell setting, find the `shell` field and set it to:

```bash
command -v aegis
# e.g. /usr/local/bin/aegis
```

Aegis accepts the common shell-launcher forms agents use internally, including
`-lc`, `-ic`, and separate `-l -c` / `-i -c` flag pairs. That means you can
point the agent directly at the `aegis` binary instead of relying on a wrapper
script just to translate shell flags.

---

## Useful commands

### View the decision log

```bash
aegis audit --last 20          # last 20 entries
aegis audit --risk Danger      # only dangerous commands
aegis audit --format json      # export as JSON
```

### Config

```bash
aegis config init    # create .aegis.toml in the current directory
aegis config show    # print the active config
```

### Roll back after a dangerous command

```bash
aegis rollback '<snapshot-id>'
```

### Add your own rules

Add to `~/.config/aegis/config.toml` or `.aegis.toml`:

```toml
[[custom_patterns]]
id          = "USR-001"
risk        = "Danger"
pattern     = "my-nuke-script\\.sh"
description = "Internal teardown script"
safe_alt    = "my-nuke-script.sh --dry-run"
```

---

## Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/uninstall.sh | sh
```

---

## Limitations

Aegis is a heuristic text filter, **not a sandbox**. It will not catch:

- Obfuscated or encoded commands
- `eval "$(something)"` — commands assembled at runtime
- Indirect execution: write a script first, run it later

Full details: [Threat model](docs/threat-model.md)

---

## Docs

- [Changelog](CHANGELOG.md)
- [Current release line](docs/releases/current-line.md)
- [Config schema](docs/config-schema.md)
- [Release readiness](docs/release-readiness.md)
- [Threat model](docs/threat-model.md)
- [Platform support](docs/platform-support.md)
- [CI and releases](docs/ci.md)
- [Troubleshooting and recovery](docs/troubleshooting.md)

---

## Contributing

Aegis is open source under the MIT license. Contributions, issues, and feature requests are welcome.

## License

MIT — see [LICENSE](LICENSE).
