# Aegis

> A shell proxy that prompts for risky operations and hard-blocks catastrophic ones.

[![CI](https://github.com/IliasAlmerekov/aegis/actions/workflows/ci.yml/badge.svg)](https://github.com/IliasAlmerekov/aegis/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-linux%20%7C%20macos-lightgrey)](docs/platform-support.md)

Aegis sits in front of your shell, inspects commands before they run, and can:

- let safe commands pass through instantly
- ask for confirmation on risky commands
- hard-block catastrophic commands
- record decisions in an audit log

---

## How it Works

If Aegis is configured as the shell wrapper for your terminal or agent session,
commands are checked **before** they reach the real shell.

<p align="center">
  <img src="src/assets/howitwork.png" alt="Aegis intercepts commands before they reach the real shell and classifies them as Safe, Warn, Danger, or Block." width="900" />
</p>

In the default interactive Protect flow:

- `Safe` → runs immediately
- `Warn` → asks for confirmation
- `Danger` → asks for confirmation and may create snapshots when configured
- `Block` → refuses to run

Mode-specific behavior is configured in `.aegis.toml`; see [Config schema](docs/config-schema.md).

---

## Install in One Command

### Recommended: one-command install

If you just want to get started, run:

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
```

What this installer does:

1. detects your platform
2. downloads the matching release binary
3. downloads the matching `.sha256`
4. verifies the checksum before installation
5. installs `aegis`
6. writes the managed shell-wrapper setup for `bash` or `zsh`

The installer fails closed on:

- missing checksum
- checksum mismatch
- missing supported checksum tool
- unsupported platform

### If the installer returns 404

That usually means a GitHub release has not been published yet for the version
you are trying to install.

In that case, use the source install path below temporarily, or install from
the published GitHub Releases page once release assets exist.

---

## Why You Might Want It

Aegis is useful if you:

- use Claude Code, Codex CLI, or other AI agents that run terminal commands
- want a second chance before destructive shell commands execute
- want command decisions logged instead of silently trusting the session

### Supported install environments

- Linux: supported
- macOS: supported
- Windows host via WSL2 terminal: expected to work as a Linux environment
- native Windows shells (`PowerShell`, `cmd.exe`): unsupported

See [Platform support](docs/platform-support.md) for the exact support policy.

---

## Make Aegis Intercept Agent Commands Automatically

Installing the binary is only the first step.

If you want Aegis to **listen to agent commands automatically**, your terminal
or agent session must use `aegis` as its shell, or respect `$SHELL` after Aegis
has been configured as the shell wrapper.

### If you installed with `install.sh`

For `bash` and `zsh`, the installer already does the important setup for you.

It:

- installs `aegis`
- updates your shell rc file with a managed block
- exports `SHELL` to the Aegis binary

That means new shell sessions started from that rc file should route commands
through Aegis automatically.

### Step 1: open a new terminal

After installation, close the current terminal and open a new one so the
updated rc file is loaded.

### Step 2: verify that your shell now points to Aegis

Run:

```bash
echo "$SHELL"
which aegis
```

You want both checks to make sense:

- `which aegis` should print the installed Aegis path
- `echo "$SHELL"` should point to that Aegis path

### Step 3: make sure your agent tool uses that shell

There are two common cases.

#### Case A: the tool respects `$SHELL`

Many agent tools launched from your terminal will use the current shell
environment automatically.

In that case:

1. install with `install.sh`
2. open a new terminal
3. start the agent from that terminal

#### Case B: the tool has an explicit shell path setting

If the tool lets you choose the shell executable directly, set it to:

```bash
$(which aegis)
```

Examples:

- **Claude Code**: set the shell path to `$(which aegis)`
- **Codex CLI / other agent tools**: if they expose a shell executable setting, point it to the Aegis binary

### If you installed manually or from source

Manual binary install and `cargo install` give you the binary, but they do
**not** fully set up automatic shell wrapping for you.

The easiest fix is to run the installer-managed setup path:

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
```

If you prefer to configure things yourself, then your shell or agent tool must
launch `aegis` instead of the real shell.

### Quick check: is Aegis actually intercepting commands?

From the same terminal session where your agent runs:

```bash
aegis -c 'rm -rf /tmp/aegis-test'
```

If you see a confirmation prompt, Aegis is active in that session.

### Troubleshooting

If Aegis is installed but agent commands are not being intercepted:

- open a fresh terminal after installation
- run `echo "$SHELL"`
- run `which aegis`
- verify that your agent tool either respects `$SHELL` or is explicitly pointed at `aegis`
- rerun `install.sh` if needed

### Manual install from a release

If you prefer not to pipe the installer script to `sh`, open the
[GitHub Releases page](https://github.com/IliasAlmerekov/aegis/releases),
download the binary and matching `.sha256` for your platform, verify the
checksum, then place the binary on your `PATH` as `aegis`.

Current release asset names are:

- `aegis-linux-x86_64`
- `aegis-linux-aarch64`
- `aegis-macos-x86_64`
- `aegis-macos-aarch64`

### Source install

If you are installing before the first release is published, or you prefer to
build from source:

```bash
cargo install --git https://github.com/IliasAlmerekov/aegis aegis
```

This installs only the binary. You still need shell/session setup yourself.

### Uninstall

For installer-managed installs:

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/uninstall.sh | sh
```

If you used a custom `AEGIS_SHELL_RC`, run uninstall with the same override so
it removes the managed block from the same file.

For manual installs:

- remove the `aegis` binary from your `PATH`
- remove any shell configuration you added manually

For source installs:

```bash
cargo uninstall aegis
```

---

## Try It Now

After installation, verify that Aegis is available:

```bash
aegis --help
```

Then try:

```bash
# Should show a confirmation prompt — type n to deny
aegis -c 'rm -rf /tmp/aegis-test'

# Should pass through instantly with no prompt
aegis -c 'echo hello'
```

---

## Shell / Agent Setup

If you installed with `install.sh`, the shell-wrapper setup for `bash` / `zsh`
should already be written for you.

If you installed manually or from source, you still need to point your session
or agent tooling at `aegis`.

### Track all agent commands (global setup)

When Aegis is configured as your shell wrapper, the installer writes a managed
bash/zsh rc block that exports `$SHELL` to the Aegis binary and keeps that value
available to shells started from those rc files.

For other shells, manual setup may be required; if you want the
installer/uninstaller to edit a POSIX-style rc file, set `AEGIS_SHELL_RC`.

This helps tools and sessions that honor `$SHELL` or an explicit shell-path
setting use Aegis, but it does not replace a terminal's actual login shell.

**Claude Code**

1. Open Claude Code settings
2. Set the shell field to `$(which aegis)`

**Other AI agents**

Tools that respect `$SHELL` or an explicit shell path (Codex CLI, etc.) can use
Aegis when configured that way.

If Aegis is already being used as the shell wrapper, a project `.aegis.toml`
can override policy for that directory or project:

```toml
mode = "Strict"  # block non-safe commands in this directory only
```

---

## Security Model

Aegis is a **heuristic command guardrail** for common destructive shell commands.

- It is **not a sandbox**
- It is **not a complete security boundary**
- Approved commands run with **your normal user permissions**
- Detection is based on the **raw command text**, not full shell execution tracing
- Snapshots and rollback are **best-effort** when configured

Aegis is designed to reduce accidental damage from direct, recognisable
commands. It should not be treated as protection against a determined bypass.

See [Threat model](docs/threat-model.md) for the fuller security-positioning
document.

---

## Limitations

Aegis may not catch:

- obfuscated or encoded shell input
- runtime-assembled commands such as `eval "$(some_function)"`
- indirect execution where one command writes a script and a later command runs it
- alias/function expansion that changes what a command does after parsing

It also does not:

- restrict filesystem, network, or syscall access after you approve a command
- guarantee lossless snapshots or perfect rollback fidelity
- support native Windows shells; Linux and macOS are supported, and WSL2 terminal
  usage is expected to work as a Linux environment

---

## Key Commands

```bash
# Policy evaluation (no execution, no audit entry)
aegis -c '<cmd>' --output json

# Config
aegis config init   # write .aegis.toml in the current directory
aegis config show   # print merged active config

# Audit log
aegis audit --last 20         # last 20 entries
aegis audit --risk Danger     # filter by risk level
aegis audit --format json     # export as JSON

# Snapshots
aegis rollback '<snapshot-id>'  # restore a snapshot taken before a Danger command
```

---

## Custom Patterns

Add your own rules to `~/.config/aegis/config.toml` or `.aegis.toml`:

```toml
[[custom_patterns]]
id          = "USR-001"
category    = "Cloud"
risk        = "Danger"
pattern     = "my-nuke-script\\.sh"
description = "Internal teardown script"
safe_alt    = "my-nuke-script.sh --dry-run"
```

Patterns are Rust regex strings. Use `(?i)` for case-insensitive matching.

---

## Built-in Pattern Categories

60+ patterns across 7 categories: **Filesystem**, **Git**, **Database**,
**Cloud**, **Docker**, **Process**, **Package**.

---

## Docs

- [Config schema](docs/config-schema.md) — modes, allowlists, snapshot policy, `--output json`
- [Threat model](docs/threat-model.md) — security boundaries, attacker model, mitigations, residual risk
- [Platform support](docs/platform-support.md) — Linux and macOS supported; WSL2 expected to work as Linux; native Windows unsupported
- [CI and release](docs/ci.md) — workflow guarantees and release notes

---

## Contributing

Bug reports and pull requests are welcome. Open an issue before starting large changes.  
See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## License

MIT — see [LICENSE](LICENSE).
