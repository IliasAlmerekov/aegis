# Aegis

> A shell guard for AI agents. Sits between your agent and the terminal — and asks before anything destructive runs.

[![CI](https://github.com/IliasAlmerekov/aegis/actions/workflows/ci.yml/badge.svg)](https://github.com/IliasAlmerekov/aegis/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-linux%20%7C%20macos-lightgrey)](docs/platform-support.md)

<p align="center">
  <img src="src/assets/howitwork.png" alt="Aegis intercepts commands before they reach the real shell and classifies them as Safe, Warn, Danger, or Block." width="900" />
</p>

---

## What does Aegis do?

When an AI agent (like Claude Code) runs a shell command, Aegis intercepts it first and decides what to do:

| Level | What happens |
|-------|-------------|
| **Safe** | Command runs instantly, no questions asked |
| **Warn** | Aegis asks: "Allow this?" |
| **Danger** | Aegis asks, and can take a snapshot for rollback |
| **Block** | Command is refused, always |

---

## Step 1 — Install

Open a terminal and run one command:

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
```

The installer will:
- detect your platform (Linux / macOS)
- download the binary and verify its checksum
- install `aegis` on your PATH
- add the shell wrapper to `~/.bashrc` or `~/.zshrc`

> **Getting a 404?** The release hasn't been published yet. Install from source instead:
> ```bash
> cargo install --git https://github.com/IliasAlmerekov/aegis aegis
> ```

---

## Step 2 — Restart your terminal

Close the current terminal window and open a new one. This loads the updated shell config.

Then confirm the install worked:

```bash
which aegis      # should print the path, e.g. /usr/local/bin/aegis
echo "$SHELL"    # should point to aegis
```

---

## Step 3 — Connect to your AI agent

For Aegis to watch **every agent command**, the agent needs to use `aegis` as its shell.

### Claude Code

1. Open Claude Code settings
2. Find the `shell` field
3. Set it to: `$(which aegis)`

### Other agents (Codex CLI, etc.)

If the agent respects the `$SHELL` environment variable — it already works after the `install.sh` setup.

If the agent has an explicit shell path setting — set it to the output of:

```bash
which aegis
```

---

## Step 4 — Test it

Run these two commands in the same terminal where your agent runs:

```bash
# Should show a confirmation prompt — press n to deny
aegis -c 'rm -rf /tmp/aegis-test'

# Should pass through instantly with no prompt
aegis -c 'echo hello'
```

If the first command shows a dialog — Aegis is active and working.

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

### Add your own block rules

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

- obfuscated or encoded commands
- `eval "$(something)"` — commands assembled at runtime
- indirect execution: write a script first, run it later

Full details: [Threat model](docs/threat-model.md)

---

## Docs

- [Config schema](docs/config-schema.md)
- [Threat model](docs/threat-model.md)
- [Platform support](docs/platform-support.md)
- [CI and releases](docs/ci.md)

---

## License

MIT — see [LICENSE](LICENSE).
