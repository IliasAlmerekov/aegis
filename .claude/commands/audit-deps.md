---
name: audit-deps
description: Run cargo audit (CVE scan) and cargo deny (license + duplicate policy) and produce a pass/fail report.
allowed_tools: ["Bash", "Read", "Grep"]
---

# /audit-deps

Use this workflow before any release or when adding/updating dependencies.

## Goal

Confirm the dependency chain is CVE-free and complies with the license and duplicate policy defined in `deny.toml`.

## Common Files

- `Cargo.toml` — direct dependencies
- `Cargo.lock` — resolved dependency tree
- `deny.toml` — cargo-deny policy (licenses, bans, duplicates)

## Suggested Sequence

1. **CVE scan** — `rtk cargo audit`
   - Zero vulnerabilities required to proceed.
   - If vulnerabilities found: check if a patched version exists, update `Cargo.toml`, re-run.

2. **License + policy check** — `rtk cargo deny check`
   - Must pass all checks: `licenses`, `bans`, `advisories`, `sources`.
   - If duplicate crate versions detected: investigate whether a common version can be pinned.

3. **Outdated check (informational)** — `rtk cargo outdated` *(if installed)*
   - Not a blocker, but flag major version gaps for follow-up.

## Output Format

```
AUDIT REPORT
============
cargo audit:   [PASS / FAIL — N vulnerabilities]
cargo deny:    [PASS / FAIL — N issues]
  licenses:    [PASS/FAIL]
  bans:        [PASS/FAIL]
  advisories:  [PASS/FAIL]
  sources:     [PASS/FAIL]

Action required: [NONE / <list issues>]
```

## Notes

- A build with known CVEs in dependencies does not ship — this is a hard gate.
- `once_cell` is explicitly banned in `deny.toml` — do not add it.
- Permitted licenses: MIT, Apache-2.0, ISC. Any other license requires explicit justification.
- This workflow is also run automatically in CI on every push.
