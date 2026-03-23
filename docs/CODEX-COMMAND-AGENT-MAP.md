# Codex Command-Agent Map (Aegis)

Mapping style mirrors `everything-claude-code/docs/COMMAND-AGENT-MAP.md`, adapted to Aegis.

| Command Intent           | Primary Agent(s)    | Notes                                                   |
| ------------------------ | ------------------- | ------------------------------------------------------- |
| ticket intake + planning | `lead_orchestrator` | Defines plan, task graph, acceptance criteria           |
| implementation           | `coder`             | One coherent task per iteration                         |
| test validation          | `tester`            | Adds/runs tests and reports regressions                 |
| correctness review       | `reviewer`          | Prioritizes bugs, behavioral regressions, missing tests |
| security review          | `security_reviewer` | Reviews bypass/fail-open/audit/snapshot risks           |
| codebase exploration     | `explorer`          | Read-only traces, contracts, and references             |
| docs/fact verification   | `docs_researcher`   | Verifies API/version claims with sources                |

## Default Route

`lead_orchestrator -> coder -> tester -> reviewer -> security_reviewer -> lead_orchestrator`

If any stage reports `CHANGES_REQUESTED`, route back to `coder` with explicit findings.

## Mandatory Constraint

All agents in this map must follow `CONVENTION.md` as the repository-level contract.

## Completion Rule

Ticket is closed only when all of these are true:

- tester: `PASSED`
- reviewer: `PASSED`
- security_reviewer: `PASSED`
- lead_orchestrator: final `summary.md` with residual risks and verification evidence
