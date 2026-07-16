<div align="center">

# Aegis

**Make AI agents ask first. Undo them when they don't.**  
Your AI agent is one `rm -rf` away from ruining your week. Aegis proxies its shell:
safe commands run instantly, destructive ones need your approval — with a Snapshot
taken first, so even a "yes" is recoverable.

[![version](https://img.shields.io/badge/version-0.6.2-60A5FA?style=flat-square)](CHANGELOG.md)
[![platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20WSL2-22C55E?style=flat-square)](#how-to-install)
[![license](https://img.shields.io/badge/license-MIT-A855F7?style=flat-square)](LICENSE)
[![built with](https://img.shields.io/badge/built%20with-Rust-F59E0B?style=flat-square)](Cargo.toml)

![How Aegis works: an AI agent's command is screened by Aegis — safe commands run instantly, dangerous ones wait for human approval](src/assets/aegis.gif)

</div>

---

## What is Aegis?

Aegis is a Rust CLI that sits between an AI agent and your real shell. Claude Code,
Codex, Cursor — any agent that runs shell commands goes through it. Every command
is risk-scored before it executes:

| Level | What happens |
|-------|-------------|
| **Safe** | Runs immediately unless bounded effect-opaque Required recovery degrades |
| **Warn** | Pauses and asks for your approval |
| **Danger** | Asks first, then attempts configured Snapshots before execution |
| **Block** | Refused outright — no prompt |

> [!NOTE]
> Aegis is a heuristic guardrail, not a sandbox or privilege boundary.
> See [`docs/threat-model.md`](docs/threat-model.md) for the full security model.

The optional **Sandbox** is a best-effort write/network guardrail add-on and
not a confidentiality boundary: it does not promise to hide readable files
or secrets.
If optional confinement is unavailable, Aegis records
`sandbox_status = "unavailable"` and warns on the active Shell or Watch channel
before running unconfined. Set `sandbox.required = true` to block instead.

Bounded **Effect-opaque execution** such as `sh ./cleanup.sh` stays on its normal
`RiskLevel`, but Protect/Strict requires at least one configured Snapshot before
it runs. If none is created, non-interactive execution denies and an interactive
user sees only **Run once without recovery** or **Deny**. Aegis does not inspect the referenced script, and a successful Snapshot is still not a complete backup
or universal undo.

---

## Before / After

<table>
<tr>
<th width="50%">Without Aegis</th>
<th width="50%">With Aegis</th>
</tr>
<tr>
<td>

```
$ rm -rf ~/.config

[command runs silently]

$ ls ~/.config
ls: cannot access '~/.config':
No such file or directory

# Config gone. No backup.
```

</td>
<td>

```
$ rm -rf ~/.config

⚠ DANGER — FS-001 · Recursive delete
Command  rm -rf ~/.config
Risk     Danger
Pattern  FS-001 — rm with -rf flag

[A] approve  [D] deny  [i] info

● Denied.
```

</td>
</tr>
</table>

---

## Why Aegis?

AI agents move fast — and sometimes fast in the wrong direction:

- delete files and directories
- reset or rewrite git history
- drop databases
- publish or push something unintended

Blocking alone isn't enough: block too much and the agent is useless, block too
little and you're restoring from backups. Aegis takes a different deal — **you stay
in the loop, and mistakes become rollbacks, not incidents**:

- **Ask first.** Destructive commands pause for your decision in a TUI — approve, deny, or inspect.
- **Snapshot before damage.** Dangerous commands trigger best-effort Snapshots (git, Docker) before they run, and opaque scripts *require* recovery unless the trusted recovery policy opts out.
- **Remember everything.** An append-only audit log records every command and every decision.

---

## How to install

> [!IMPORTANT]
> **Windows:** install inside WSL2. Native PowerShell and `cmd.exe` are not supported — there is no native Windows build.

### Quick install (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis-shellguard/main/scripts/install.sh | sh
```

Installs the binary, writes a managed block to `~/.zshrc` or `~/.bashrc`, and hooks into Claude Code / Codex when those config directories already exist. Reload your shell afterwards.

### npm

```bash
npm i -g @iliasalmerekov/aegis
```

Runs `aegis install-hooks --all` automatically when Claude Code or Codex config directories are present. Set `AEGIS_NPM_SKIP_HOOKS=1` to opt out.

npm and Cargo install the binary only; neither runs the global shell installer or edits your shell startup files. Opt in with `aegis setup-shell` (see below).

### Homebrew

```bash
brew tap IliasAlmerekov/aegis
brew install aegis
```

Homebrew installs the binary only — like npm and Cargo, it does not run the global shell installer. Opt in with `aegis setup-shell`.

### Developer source install

```bash
cargo install --git https://github.com/IliasAlmerekov/aegis-shellguard --tag v0.6.2 aegis
```

---

### Shell-proxy mode (package manager installs)

Package manager installs are binary-only. To opt in to shell-proxy mode:

```bash
aegis setup-shell
```

This adds a managed block to `~/.zshrc` or `~/.bashrc` that sets `SHELL` to the
aegis binary and `AEGIS_REAL_SHELL` to your real shell. Remove it with:

```bash
aegis setup-shell --remove
```

The convenience installer is **Global**-first: it installs the binary, writes the managed shell block, and sets up Claude Code / Codex hooks when those config directories already exist. The old **Local** project-only and **Binary**-only installer modes have been removed; package-manager installs are binary-only.

---

### Verify it works

```bash
aegis --version                         # prints version number
aegis -c 'echo hello'                   # safe — runs immediately, no prompt
aegis -c 'rm -rf /tmp/aegis-test'       # danger — interceptor appears, press D to deny
```

> [!TIP]
> If `echo hello` runs right away and the risky command prompts — Aegis is working.

---

## Connect to your AI agent

**Claude Code** and **Codex** are protected through `PreToolUse` hooks — not shell-proxy tricks. These hooks intercept Bash commands regardless of `$SHELL`.

```bash
# Claude Code
aegis install-hooks --claude-code

# All supported agents at once
aegis install-hooks --all
```

Re-run after upgrading to migrate any older `aegis hook` / `aegis-rewrite.sh` registration to the current shim.

> [!TIP]
> **Other agents:** for tools that respect `$SHELL`, run `aegis setup-shell`. For an agent with a `shell` config field, find the `shell` field and set it to the output of `command -v aegis`.

---

## How it works

```
AI agent command
      │
      ▼
 Aegis parses and classifies it
      │
      ├──▶ Safe   ──▶ run immediately
      ├──▶ Warn   ──▶ ask first
      ├──▶ Danger ──▶ ask first, then snapshot if approved/configured
      └──▶ Block  ──▶ refuse
                          │
                          ▼
               real shell executes only
               what you approved
```

Effect opacity is a separate axis: a bounded effect-opaque command that would
otherwise execute must create a required Snapshot, receive a one-time Recovery
override, or deny.

![Aegis command flow](src/assets/howitwork.png)

---

## Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis-shellguard/main/scripts/uninstall.sh | sh
```

---

## Docs

| Document | Description |
|----------|-------------|
| [Architecture decisions](docs/adr/README.md) | ADR-001 through ADR-015 |
| [Threat model](docs/threat-model.md) | Security scope and assumptions |
| [Config schema](docs/config-schema.md) | `aegis.toml` reference |
| [Platform support](docs/platform-support.md) | Linux, macOS, WSL2 details |
| [Release readiness](docs/release-readiness.md) | 1.0 gate status |
