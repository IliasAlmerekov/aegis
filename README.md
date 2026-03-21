# Aegis

A lightweight Rust terminal proxy that intercepts AI agent commands and requires human confirmation before destructive operations.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
```

The installer detects `linux`/`macos` and `x86_64`/`aarch64`, downloads the matching `aegis` binary, and installs it to `/usr/local/bin/aegis`.

After install:

```bash
echo 'export SHELL=$(which aegis)' >> ~/.bashrc
echo 'export SHELL=$(which aegis)' >> ~/.zshrc
```

If you use Claude Code, open Claude settings and set the shell path to `$(which aegis)`.
