# Aegis architecture decisions

This document has moved.

The monolithic ADR file was split into one record per decision under
[`docs/adr/`](adr/README.md).

Use the new ADR index:

- [`docs/adr/README.md`](adr/README.md)

Direct links to the current records:

- [`ADR-001 — Keep the CLI entrypoint thin`](adr/adr-001-keep-cli-entrypoint-thin.md)
- [`ADR-002 — The interception hot path stays synchronous`](adr/adr-002-the-interception-hot-path-stays-synchronous.md)
- [`ADR-003 — Aegis is a heuristic guardrail, not a sandbox`](adr/adr-003-aegis-is-a-heuristic-guardrail-not-a-sandbox.md)
- [`ADR-004 — Snapshots are best-effort; audit is append-only`](adr/adr-004-snapshots-are-best-effort-audit-is-append-only.md)
- [`ADR-005 — Global toggle at command boundaries`](adr/adr-005-global-toggle-at-command-boundaries.md)
- [`ADR-006 — CI detection has an explicit override contract`](adr/adr-006-ci-detection-has-an-explicit-override-contract.md)
- [`ADR-007 — Shell hooks share one managed helper, but must not fail open`](adr/adr-007-shell-hooks-share-one-managed-helper-but-must-not-fail-open.md)
- [`ADR-008 — Installer is global-first and rejects removed mode controls`](adr/adr-008-installer-is-global-first-and-rejects-removed-mode-controls.md)
- [`ADR-010 — Full shell evaluation and deferred execution remain non-goals`](adr/adr-010-full-shell-evaluation-and-deferred-execution-remain-non-goals.md)

This compatibility shim stays in place so older links — including changelog and
release-history references — do not break.
