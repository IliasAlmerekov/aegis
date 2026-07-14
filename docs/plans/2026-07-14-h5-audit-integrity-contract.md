# H5 — Audit integrity contract alignment

## Status

Draft — requires a finding-specific `grill-with-docs` session before TDD. This
is a wording and public-contract correction, not a cryptographic anchoring
project.

## Finding

`ChainSha256` is an unkeyed chain stored with the audit log. It can detect
accidental corruption, broken links, and edits that are not followed by a full
re-chain. An actor who can rewrite or truncate the complete local log can produce
a new internally consistent chain. Calling this adversarially “tamper-evident”
exceeds Aegis' local heuristic-guardrail contract.

## Scope

1. Introduce and consistently use the public phrase **audit integrity chain** or
   **integrity check**.
2. Replace stronger claims in CLI help, config templates/schema, architecture,
   release-readiness, threat-model, source docs, and public copy.
3. State the residual explicitly: no keyed MAC, remote witness, monotonic counter,
   or external anchor exists.
4. Keep serialized names (`ChainSha256`, `entry_hash`, `prev_hash`, `chain_alg`)
   and verification behavior backward-compatible.
5. Do not add HMAC/key management or external services in this slice. Either
   would require a separate security-model decision and ADR.

## TDD seams

- CLI seam: help and verification output use integrity language.
- Documentation-contract seam: repository tests reject the phrase
  `tamper-evident` for the local chain outside historical/limitation context.
- Audit seam: existing mutation/link-break tests remain green and are described
  as integrity failures rather than proof of adversarial tampering.

## Implementation sequence

1. Add a failing contract test for user-visible wording.
2. Update CLI/config/source terminology without changing data formats.
3. Update `CONTEXT.md`, `ARCHITECTURE.md`, `docs/config-schema.md`,
   `docs/threat-model.md`, `docs/release-readiness.md`, and ADR-004 wording.
4. Run the full repository search again; allow stronger wording only where it
   describes the rejected claim or residual risk.

## Verification

- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
- `rtk git grep -n "tamper-evident\|tamper evidence"`
