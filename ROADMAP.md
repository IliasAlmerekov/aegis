# Aegis — v2 Roadmap

This document tracks features planned for Aegis v2. Community input is welcome — open a GitHub Discussion to vote on priorities or propose new ideas.

---

## Snapshot backends

### Cloud storage plugin

Back up files to S3 / GCS / Azure Blob before destructive operations. Useful for teams that do not use Git for all project files.

```toml
[[plugins]]
name = "s3"
bucket = "my-aegis-backups"
prefix = "snapshots/"
```

### PostgreSQL / MySQL plugin

Run `pg_dump` / `mysqldump` before any `DROP`, `DELETE`, or `TRUNCATE` command that targets a local or tunneled database.

### Slack / PagerDuty notify plugin

Send a notification to a Slack channel or PagerDuty incident when a `Danger` or `Block` command is intercepted. Useful for shared CI/CD environments.

---

## Policy DSL

Replace the TOML pattern table with a structured policy language for expressing complex rules:

```
when command matches "terraform destroy"
  and env.CI is not set
  then require-approval from "ops-team"
```

Policies would support:
- Environment variable conditions (`env.ENVIRONMENT == "production"`)
- Time-based rules ("deny during business hours")
- Team-based approval flows (integration with GitHub teams or LDAP groups)

---

## Rollback command

```bash
aegis rollback              # roll back the most recent snapshot
aegis rollback --list       # list available snapshots
aegis rollback <id>         # roll back to a specific snapshot
```

Rollback is already supported internally by each snapshot plugin. v2 exposes it as a first-class CLI command with an interactive picker.

---

## Web dashboard

A local web UI (`aegis serve`) for browsing the audit log, visualizing risk trends over time, and replaying sessions.

---

## Remote audit sink

Stream audit log entries to a remote endpoint (HTTP webhook, Datadog, Elasticsearch) in addition to the local JSONL file. Useful for compliance and team-level visibility.

---

## Windows support

Aegis currently targets Linux and macOS. Windows support (via `cmd.exe` and PowerShell interception) is tracked as a v2 goal. The main blocker is the absence of a `$SHELL` equivalent — the approach will use a different integration point.

---

## Feedback

Open a [GitHub Discussion](https://github.com/IliasAlmerekov/aegis/discussions) to share what you need. Label your request with the relevant category above.
