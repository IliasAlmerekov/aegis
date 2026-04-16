# RTK execution helper

This repository expects shell commands run by Codex CLI to go through `rtk`.

## Rules

- Prefix shell commands with `rtk`.
- Do not run raw `cargo`, `git`, `rg`, `sed`, or similar commands directly.
- Keep command output focused and reproducible.

## Examples

```bash
rtk cargo test
rtk cargo fmt --check
rtk git status --short
rtk rg -n "pattern" src tests
```

## Scope

This is a local execution convention for agent-assisted work in this repository.
It is not part of the runtime product contract.
