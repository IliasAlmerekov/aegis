# Aegis Post-v1 Ideas

This document tracks ideas that may be valuable after `v1.0.0`, but are not part
of the production-readiness path.

These items are intentionally separated from `ROADMAP.md` so the path to
`v1.0.0` stays focused on hardening, release trust, compatibility, and
operational maturity.

---

## Product and platform expansion

### Native Windows shell support

Explore interception for native Windows environments such as PowerShell and
`cmd.exe` after the Unix-first contract is stable.

### Web dashboard

A local web UI (`aegis serve`) for browsing the audit log, visualizing risk
trends, and replaying sessions.

### Remote audit sink

Stream audit log entries to remote endpoints such as HTTP webhooks, Datadog, or
Elasticsearch in addition to the local JSONL log.

---

## Additional integrations

### Slack / PagerDuty notifications

Send notifications when `Danger` or `Block` commands are intercepted in shared
or automated environments.

### Team approval workflows

Explore approval models that integrate with GitHub teams, LDAP groups, or other
organization-level identity systems.

---

## Future snapshot and rollback expansion

### Cloud storage snapshot plugins

Add snapshot providers for S3, GCS, Azure Blob, or similar remote storage
targets.

### Additional database and managed-service providers

Expand provider support beyond the current set when the rollback and integrity
contracts are stable enough to generalize safely.

---

## Policy language evolution

### Policy DSL

Consider replacing or complementing TOML pattern tables with a more expressive
policy language for environment-aware and time-aware rules.
