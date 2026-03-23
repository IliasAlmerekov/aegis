# Workflow: feature-development (Aegis Codex)

Goal: implement a feature with minimal, reviewable diff while preserving Aegis safety contracts.

## Sequence

1. Read `.claude/CLAUDE.md`, `.claude/AGENTS.md`, and `CONVENTION.md`.
2. Confirm impacted modules and contracts before editing.
3. Apply smallest coherent implementation.
4. Add/update targeted tests.
5. Run relevant verification via `rtk`.
6. Summarize behavior changes and residual risk.
