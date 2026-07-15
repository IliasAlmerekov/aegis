# ADR-017 — Audit integrity chain has no external anchor

## Status

Accepted

## Context

`ChainSha256` links local audit entries and rotated segments with unkeyed
SHA-256 hashes. It detects corruption, broken links, and inconsistent edits.
An actor able to rewrite or truncate the complete local log can create a new,
internally consistent chain.

## Decision

For the 1.0 contract, call `ChainSha256` an **audit integrity chain** and its
verification an **integrity check**. State that it has no keyed or external
anchor and does not establish adversarial resistance to a complete local-log
rewrite.

Keyed MACs, remote witnesses, monotonic counters, and other cryptographic
anchors are explicit non-goals for 1.0. Any future anchoring mechanism requires
a separate security-model ADR defining its key custody, trust boundary,
availability, and verification semantics.

## Consequences

- Data formats and the `ChainSha256` serialized name remain unchanged.
- CLI, configuration, documentation, and landing copy describe the bounded
  integrity-check capability consistently.
- Operators who need an adversarially anchored audit trail must use a separately
  designed future mechanism rather than infer that guarantee from local hashes.
