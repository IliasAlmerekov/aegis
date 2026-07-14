# M9 — Rollback identifier round-trip

## Status

Draft — requires a finding-specific design grill before TDD. Preserve
compatibility with existing audit entries before choosing a new identifier
format.

## Finding

Some plugins encode context and an internal ID as a tab-separated opaque
`snapshot_id`. Listing renders the tab like a column separator, while
`aegis rollback` expects one shell argument. A user cannot reliably copy the
displayed value back into the recovery command.

## Constraints

- Existing Git/MySQL/PostgreSQL audit entries must remain recoverable.
- Audit JSONL is a public format; do not silently reinterpret IDs without a
  compatibility reader.
- The visible CLI path must not require users to reconstruct tabs or quoting.
- Plugin context (repo/database/target) must remain available for rollback.

## Candidate slices

1. Minimum recovery seam: `aegis rollback --last` resolves the latest successful
   applicable snapshot directly from audit.
2. Listing seam: emit a shell-ready rollback command or a stable display ID that
   maps to the opaque stored ID.
3. Data-model seam: in a later compatible schema revision, separate plugin
   context from the opaque snapshot identifier.

The grill must choose whether slice 1 alone closes the release blocker. Avoid
changing every plugin ID before the user path is proven.

## TDD seams

- A listed Git snapshot can be rolled back through a copied/generated command.
- The same is true for MySQL/PostgreSQL composite IDs.
- Legacy tab-bearing audit entries resolve.
- Ambiguous short IDs fail with actionable candidates rather than choosing one.

## Verification

- Focused CLI/list/rollback and live snapshot tests
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
