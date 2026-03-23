# Aegis Codex Guardrails

Codex adaptation of repository `.claude` rules.

## Mandatory

- Use `rtk` prefix for every shell command.
- Follow `CONVENTION.md` and `.claude/CLAUDE.md` as authoritative.
- Keep `src/main.rs` as CLI wiring only.
- Keep `src/interceptor/*` synchronous.
- Preserve exit-code contract.
- Preserve append-only audit JSONL behavior.

## Forbidden

- Raw shell commands without `rtk`
- New `unsafe {}`
- Silent suppression of clippy warnings
- Dependency or CI policy changes without explicit approval
- Any change that weakens risk classification or confirmation gates

## Validation Baseline

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
```

Add benches/audit/deny when changes touch performance, security, or dependencies.
