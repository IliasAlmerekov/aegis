# Aegis

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

Native Windows shells such as PowerShell and `cmd.exe` are not supported.

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

Homebrew and npm install the binary only. To opt in to shell-proxy mode after
installing with a package manager, run:

```bash
aegis setup-shell
```

Developer source install:

```bash
cargo install --git https://github.com/IliasAlmerekov/aegis --tag v0.5.8 aegis
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

For a visual overview, see:

![Aegis command flow](src/assets/howitwork.png)