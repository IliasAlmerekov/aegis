# Aegis

> A simple safety layer for AI agents. It sits between the agent and your terminal, and asks before dangerous commands run.

[![CI](https://github.com/IliasAlmerekov/aegis/actions/workflows/ci.yml/badge.svg)](https://github.com/IliasAlmerekov/aegis/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-linux%20%7C%20macos%20%7C%20wsl-lightgrey)](docs/platform-support.md)

<p align="center">
  <img src="src/assets/howitwork.png" alt="Aegis intercepts commands before they reach the real shell and classifies them as Safe, Warn, Danger, or Block." width="900" />
</p>

---

## What Aegis does

AI agents are fast. Sometimes too fast.

One bad command can:

- delete files,
- reset a repo,
- drop a database,
- or push something dangerous.

**Aegis adds one small safety step.**

It checks the command first:

- safe commands run right away,
- risky commands ask for confirmation,
- the worst commands are blocked.

---

## Install Aegis

Works on:

- **Linux**
- **macOS**
- **Windows with WSL2**

native Windows shells like **PowerShell** and **cmd.exe** are **not** supported.

### Step 1: copy this command

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
```

### Step 2: paste it into your terminal

Wait until the installer finishes.

### Step 3: open a new terminal

This is important. Aegis updates your shell setup, so open a fresh terminal window or tab.

### Step 4: check that it installed

```bash
aegis --version
```

If you see a version number, the install worked.

---

## Check that Aegis is really working

Run these two commands:

```bash
# This should show a confirmation prompt. Press n.
aegis -c 'rm -rf /tmp/aegis-test'

# This should run immediately.
aegis -c 'echo hello'
```

If the **second command shows a prompt** and the **third prints `hello` right away**, **Aegis is working**.

### Optional: check shell routing

```bash
echo "$SHELL"
echo "$AEGIS_REAL_SHELL"
```

- `SHELL` should point to Aegis
- `AEGIS_REAL_SHELL` should point to your real shell, like `/bin/zsh` or `/bin/bash`

---

## What happens when a command is checked?

When an AI agent runs a shell command, Aegis intercepts it and decides what to do:

| Level | What happens | Example |
|-------|-------------|---------|
| **Safe** | Runs instantly, no questions asked | `ls`, `echo hello`, `git status` |
| **Warn** | Aegis asks: "Allow this?" | `git push --force`, `npm publish` |
| **Danger** | Aegis asks, and can snapshot for rollback | `rm -rf ./src`, `DROP TABLE users` |
| **Block** | Refused, always | `rm -rf /`, `mkfs.ext4 /dev/sda` |

---

## Install behavior

The convenience installer is **global-first**.

- **Global** — installs the `aegis` binary, writes the managed shell-integration
  block, and then attempts local Claude Code / Codex hook setup when a real
  local checkout and supported agent directories are present.
- **Local** — the old project-only shell mode has been removed from the
  convenience installer. Use `aegis off` / `aegis on` for a temporary local
  workflow change without uninstalling the shell wrapper, or configure a manual
  shell path only for the agent you care about.
- **Binary** — the old binary-only installer mode has been removed. If you only
  want the binary, use the verification-first manual path or the source install
  path below.
- The installer rejects the removed `AEGIS_SETUP_MODE` and
  `AEGIS_SKIP_SHELL_SETUP` controls instead of silently ignoring them.

Automatic shell setup currently recognizes `bash` and `zsh`.

If you use another shell or a custom rc file, set:

```bash
AEGIS_SHELL_RC=/path/to/your/rcfile
```

and run the installer again.

If you are already inside an Aegis shell, also set:

```bash
AEGIS_REAL_SHELL=/path/to/your/real/shell
```

When disabled, Aegis behaves as though it is not installed for ordinary local
shell and supported agent usage. By default, detected CI environments ignore
the local disabled flag and continue enforcing policy. `AEGIS_CI` can
explicitly override CI detection.

### If automatic agent setup did not run

If you installed from a local checkout, you can run this manually:

```bash
sh scripts/agent-setup.sh
```

This installs supported hooks for **Claude Code** and **Codex** when their config folders exist.

### Alternative: install from source

If the pre-built binary is not available for your platform:

```bash
cargo install --git https://github.com/IliasAlmerekov/aegis aegis
```

### Windows

Aegis works on Windows through **WSL2** (Windows Subsystem for Linux).

Open a WSL2 terminal and run the install command there.

native Windows shells like **PowerShell** and **cmd.exe** are not supported.

---

## Turn it off and on again

If you want to pause Aegis for a moment:

```bash
aegis off
```

To turn it back on:

```bash
aegis on
```

To see the current state:

```bash
aegis status
```

If Aegis is disabled locally, detected CI environments still enforce policy by default.

---

## Connect to your AI agent

### Claude Code

In many setups, Claude Code will automatically use Aegis because `$SHELL` already points to it.

If you need to set it manually:

```bash
command -v aegis
```

Then paste that path into the `shell` field.

### Codex, Cursor, and other agents

If the agent respects `$SHELL` — it works automatically.

If the agent has its own shell setting, find the `shell` field and set it to:

```bash
command -v aegis
# e.g. /usr/local/bin/aegis
```

For **Codex**, optional hooks can also add startup guidance and block Bash commands that are not wrapped through `aegis --command`.

---

## Useful commands

```bash
aegis status                  # show current state
aegis off                     # disable temporarily
aegis on                      # enable again

aegis audit --last 20          # last 20 entries
aegis audit --risk Danger      # only dangerous commands
aegis audit --format json      # export as JSON
aegis audit --verify-integrity # verify audit chain
```

```bash
aegis config init    # create .aegis.toml in the current directory
aegis config show    # print the active config
aegis config validate # validate merged config
```

```bash
aegis rollback '<snapshot-id>'
```

### Snapshots

For dangerous commands, Aegis can take best-effort snapshots with configured providers such as:

- Git
- Docker
- PostgreSQL
- MySQL / MariaDB
- SQLite
- Supabase

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

## Limitations

Aegis is a heuristic text filter, **not a sandbox**. It will not catch:

- Obfuscated or encoded commands
- `eval "$(something)"` — commands assembled at runtime
- Indirect execution: write a script first, run it later

Full details: [Threat model](docs/threat-model.md)

---

## Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/uninstall.sh | sh
```

---

## Docs

- [Architecture decisions](docs/architecture-decisions.md)
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
