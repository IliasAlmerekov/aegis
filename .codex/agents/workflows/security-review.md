# Workflow: security-review (Aegis Codex)

Goal: verify that a change does not introduce bypasses or fail-open behavior.

## Focus Areas

- `RiskLevel` semantics and ordering
- Scanner/parser classification integrity
- Non-interactive confirmation behavior
- Snapshot rollback behavior for dangerous commands
- Append-only audit logger guarantees

## Sequence

1. Trace changed execution paths.
2. Enumerate potential bypass or downgrade points.
3. Verify test coverage for positive/negative paths.
4. Record concrete findings with file references.
