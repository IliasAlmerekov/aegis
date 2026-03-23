# Plan

- ticket_id: P1-T1.1-custom-patterns-runtime-contract
- owner: lead_orchestrator
- status: PLANNED

## Task Graph

1. Extract runtime pattern-build pipeline into unified builder: builtin + custom + validation.
2. Wire scanner assessment to config `custom_patterns` in runtime path.
3. Add tests for merge/duplicate-id policy and for end-to-end classification impact.
4. Expose source labeling for custom patterns in audit + keep UI source labels.
5. Run verification and reviewer/security stages.

## Acceptance Criteria

- `custom_patterns` from config change `Assessment` result in runtime.
- Decision/match source is visible as `custom` in UI and audit.
- Duplicate `id` is rejected.
- Unit tests cover merge + duplicate checks.
- Integration test demonstrates config custom pattern changes classification.

## Risks

- Touches scanner/pattern pipeline (hot path initialization): avoid runtime slow-path changes.
- Backward compatibility of audit schema: source field must be optional.

## Convention Check

Planned changes keep scanner/parser synchronous, preserve fail-closed fallback, and avoid dependency/CI changes per `CONVENTION.md`.

## Next Owner

coder
