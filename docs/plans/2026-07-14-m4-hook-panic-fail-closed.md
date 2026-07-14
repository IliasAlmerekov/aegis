# M4 — Hook panic containment

## Status

Draft — requires a finding-specific `grill-with-docs` session before TDD.

## Finding

The hook entry point converts normal parse/validation errors into deny JSON, but
an unwind can terminate before any protocol response is emitted. Agent clients
may interpret missing output as permission to continue.

## Scope

- Put `catch_unwind` at the outer Rust hook boundary, not around individual
  helpers.
- Convert a panic payload into the existing agent-compatible deny shape without
  exposing sensitive internals.
- Keep the panic hook/logging behavior deterministic and avoid double-printing
  protocol output.
- Do not use panics for expected hook errors; existing typed paths remain primary.

## TDD seams

- Inject a test-only panic behind the public hook dispatch seam and assert valid
  deny JSON with exit behavior expected by Claude/Codex.
- Ordinary allow/noop/deny inputs remain byte/structure compatible.
- A non-string panic payload still produces a stable generic reason.

## Implementation sequence

1. Add one failing boundary-panic integration test.
2. Wrap dispatch with `AssertUnwindSafe` only if the captured inputs require it;
   document why.
3. Reuse `hook_deny_output` and existing render/exit flow.
4. Add parity coverage for both installed hook shims.

## Verification

- Focused hook tests
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
