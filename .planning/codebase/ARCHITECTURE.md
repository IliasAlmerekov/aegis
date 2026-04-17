# ARCHITECTURE

Generated: 2026-04-17
Focus: tech+arch

## High-level model
Aegis is a local shell-proxy CLI that sits between an AI agent (or human operator) and the real shell.

Core flow:
1. Receive raw command text
2. Parse/segment command structure
3. Scan and classify risk (`Safe`, `Warn`, `Danger`, `Block`)
4. Build a typed interception plan
5. Require approval or block when policy says so
6. Optionally create best-effort snapshots for dangerous commands
7. Execute via the real shell when allowed
8. Append audit entries

## Architectural strengths
- Clear separation between parsing/scanning, planning/policy, runtime wiring, UI, snapshots, and audit
- Honest security posture in docs: heuristic guardrail, not a sandbox
- Typed planning boundary (`planning/`) centralizes policy semantics instead of duplicating them across CLI surfaces
- Hot path remains synchronous while slow subprocess work is async
- Audit log treated as a first-class security artifact

## Architectural constraints visible in repo rules
- `main.rs` should remain thin, but is currently large (~1563 LOC)
- `interceptor/` must stay synchronous and benchmark-sensitive
- `Block` must remain non-bypassable
- Failures are intended to be fail-closed

## Current architectural pressure points
- Several security-sensitive files are very large:
  - `src/audit/logger.rs` ~2158 LOC
  - `src/config/model.rs` ~1891 LOC
  - `src/ui/confirm.rs` ~1739 LOC
  - `src/main.rs` ~1563 LOC
  - `src/snapshot/supabase.rs` ~1596 LOC
- This increases review cost and regression risk, especially in a security-adjacent product

## Design trade-offs
- Broad product ambition: shell guard + policy engine + audit system + multi-provider snapshot/rollback + watch mode
- This makes the project more differentiated, but also expands the trusted code surface significantly before 1.0
- Snapshot provider breadth is ahead of maturity signaling; core guardrail value is stronger than the current multi-provider recovery story

## Release-readiness architecture verdict
- Architecture is strong for an MVP/public beta guardrail
- Architecture is not yet “small trusted core” enough for a high-trust security-tool positioning
- Best next architecture move is reduction/refactoring of oversized modules and clearer maturity tiers for snapshot providers
