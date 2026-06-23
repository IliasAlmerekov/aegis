# Aegis

![How Aegis works: an AI agent's command is screened by Aegis — safe commands run instantly, dangerous ones wait for human approval](src/assets/aegis.gif)

> A small safety layer for AI agents that run shell commands.

## What is Aegis?

Aegis is a Rust CLI that sits between an AI agent and your real shell.
It checks each command before it runs:

- safe commands run immediately
- risky commands ask for approval
- catastrophic commands are blocked

Aegis is a heuristic guardrail, not a sandbox or privilege boundary. See
[`docs/threat-model.md`](docs/threat-model.md) for the full security model.

## Why Aegis?

AI agents can move fast and run destructive commands by mistake:

- delete files
- reset repositories
- drop databases
- publish or push something dangerous

Aegis adds a human checkpoint before that damage happens. It also records
decisions in an append-only audit log and can take best-effort snapshots for
some dangerous commands.

## How to install

Supported platforms:

- Linux
- macOS
- Windows through WSL2

On Windows, install inside WSL2; native Windows shells such as PowerShell and
`cmd.exe` are not supported.

### Convenience installer

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
```

Reload your shell, then check:

```bash
aegis --version
```

### Homebrew

```bash
brew tap IliasAlmerekov/aegis
brew install aegis
```

### npm

```bash
npm i -g @iliasalmerekov/aegis
```

Homebrew installs the binary only. npm and Cargo install the binary only too;
none of them run the global shell installer or edit your shell startup files.

To opt in to shell-proxy mode after installing with a package manager, run:

```bash
aegis setup-shell
```

This adds a managed block to `~/.zshrc` or `~/.bashrc` that sets `SHELL` to the
aegis binary and `AEGIS_REAL_SHELL` to your real shell. Remove it with:

```bash
aegis setup-shell --remove
```

### Install behavior

The convenience installer is **Global**-first: it installs the binary, writes
the managed shell block, and sets up Claude Code / Codex hooks when those
config directories already exist. The old **Local** project-only and
**Binary**-only installer modes have been removed; package-manager installs are
binary-only.

### Check that it works

```bash
aegis -c 'rm -rf /tmp/aegis-test'   # should prompt — press n
aegis -c 'echo hello'               # should run immediately
```

If `echo hello` runs right away and the risky command prompts, Aegis is working.

### Connect to your AI agent

For **Claude Code**, run `command -v aegis` and paste that path into the
`shell` field. For other agents that respect `$SHELL`, Aegis works
automatically; otherwise find the `shell` field and set it to the aegis path.

### Developer source install

```bash
cargo install --git https://github.com/IliasAlmerekov/aegis --tag v0.5.8 aegis
```

### Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/uninstall.sh | sh
```

## How it works

```text
AI agent command
      |
      v
 Aegis parses and classifies it
      |
      +--> Safe   -> run
      +--> Warn   -> ask first
      +--> Danger -> snapshot if configured, then ask first
      +--> Block  -> refuse
      |
      v
 real shell executes only approved commands
```

![Aegis command flow](src/assets/howitwork.png)

## Docs

- [Architecture decisions](docs/adr/README.md)
- [Threat model](docs/threat-model.md)
- [Config schema](docs/config-schema.md)
- [Release readiness](docs/release-readiness.md)
- [Platform support](docs/platform-support.md)
