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
| **Danger** | Aegis asks, and can take a snapshot for rollback when configured (Git, Docker, and database providers) |
| **Block** | Command is refused, always |

---

## Step 1 — Install

There are two install paths:

### Option A — convenience installer

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
```

The installer will:
- detect your platform (Linux / macOS)
- download the binary and verify its checksum
- install `aegis` on your PATH
- add the shell wrapper to `~/.bashrc` or `~/.zshrc`

### Option B — verification-first manual install

Prefer to verify the release asset yourself first? Follow the manual
checksum-first path in [Release readiness](docs/release-readiness.md). It shows
how to download the release asset, fetch the matching `.sha256` sidecar, verify
it with `sha256sum` or `shasum -a 256`, place the verified binary on your
`PATH`, and point your shell or agent at `aegis` afterward.

If you're on Windows, the best-effort path is to run Aegis inside a WSL2
terminal; native Windows shells like `PowerShell` and `cmd.exe` are not
supported yet.

If install fails, see [Troubleshooting](docs/troubleshooting.md), especially
for checksum and shell-wrapper issues.

> **Getting a 404?** The release asset for the version you need isn't available yet. Install from source instead:
> ```bash
> cargo install --git https://github.com/IliasAlmerekov/aegis aegis
> ```

---

## Step 2 — Restart your terminal

Close the current terminal window and open a new one. This loads the updated shell config.

Then confirm the binary install worked:

```bash
command -v aegis  # should print the path, e.g. /usr/local/bin/aegis
aegis --help      # should print the CLI help text
```

These checks prove that `aegis` is installed and runnable, but they do not
prove that command routing is active.

To verify the active routing setup:

- **Convenience installer or `$SHELL`-based setup**: confirm your shell is
  actually running the wrapper by checking that `SHELL` points to the absolute
  `aegis` path and `AEGIS_REAL_SHELL` points to your real shell:

  ```bash
  echo "$SHELL"            # should print the absolute path to aegis
  echo "$AEGIS_REAL_SHELL"  # should print your real shell path
  ```

- **Explicit agent shell-path setup**: confirm the agent setting itself points
  to the absolute `aegis` path printed by `command -v aegis`.

---

## Step 3 — Connect to your AI agent

For Aegis to watch **every agent command**, the agent needs to use `aegis` as its shell.

### Claude Code

1. Open Claude Code settings
2. Find the `shell` field
3. Run `command -v aegis`, then paste the absolute path it prints into that
   field

### Other agents (Codex CLI, etc.)

If the agent respects the `$SHELL` environment variable — set it to
the absolute path printed by `command -v aegis` in your shell profile, and
preserve the real shell with `AEGIS_REAL_SHELL` as described in
[Release readiness](docs/release-readiness.md), or launch the agent from a
shell where both variables are already set correctly.

If the agent has an explicit shell path setting — set it to the output of:

```bash
command -v aegis
```

---

## Step 4 — Test it

Run these two commands in the same terminal where your agent runs. They prove
the wrapper binary is working, but they still do not by themselves prove the
agent is routed through it:

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

- [Changelog](CHANGELOG.md)
- [Current release line](docs/releases/current-line.md)
- [Config schema](docs/config-schema.md)
- [Release readiness](docs/release-readiness.md)
- [Threat model](docs/threat-model.md)
- [Platform support](docs/platform-support.md)
- [CI and releases](docs/ci.md)
- [Troubleshooting and recovery](docs/troubleshooting.md)

---

## License

MIT — see [LICENSE](LICENSE).
