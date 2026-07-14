# ADR-016 — Effect-opaque execution uses recovery backstops without raising risk

## Status

Accepted

## Context

Aegis' scanner classifies command text before execution. Some command shapes reveal
that another execution layer will decide the eventual filesystem, database, or network
effect, but do not reveal the effect itself. `Script-file execution` is the cleanest
case: `sh ./cleanup.sh`, `python3 ./x.py`, or `node ./x.js` can be ordinary agent work,
yet the destructive effect, if any, lives in the referenced file rather than in argv,
an inline body, a heredoc, or a pipe input that the scanner already assesses.

Raising every such command from `Safe` to `Warn` would reintroduce approval fatigue on
common build/test workflows. Making sandbox confinement the primary answer would also
force an awkward `allow_write` trade-off: a profile permissive enough for legitimate
builds is permissive enough for many harmful script effects, while a strict profile
breaks normal work.

## Decision

Keep `RiskLevel` and backstop requirements as orthogonal axes. `Effect-opaque execution`
does not raise `RiskLevel` by itself. Instead, it records an `effect_opaque` marker and
requires a recovery backstop by setting `snapshots_required` under the default
`SnapshotPolicy::Selective` / `Full` path. A successful snapshot preserves the happy path:
the command can run without an extra prompt when policy would otherwise allow it.

If a required snapshot cannot be created, Aegis must degrade loudly: non-interactive
execution fails closed, and interactive execution must present the missing-recovery
reason instead of silently running unprotected. `SnapshotPolicy::None` is a trusted
global opt-out only; project config cannot weaken recovery because the project security
ratchet applies. Runtime confinement remains an optional stricter tier represented by a
separate `confinement_required` axis, not the default mitigation for effect opacity.

For v1 detection, only bounded script/interpreter shapes are in scope: shell and common
language interpreters invoked with a script-file-looking argv token, plus interpreter
stdin forms such as `sh -s` and pipe-to-shell shapes that are already classified by
existing rules. V1 does not perform filesystem `stat()` on the hot path and does not
treat package runners (`npm run`, `make`, `cargo xtask`, etc.) as effect-opaque by
default.

## Consequences

- A command can remain `Safe` for prompt UX while still requiring snapshot recovery.
- Audit entries need to explain both why a command was effect-opaque and which backstops
  were required or degraded.
- Snapshot-before-Danger becomes snapshot-before-Danger-or-opaque; the snapshot contract
  remains best-effort unless a required snapshot fails closed.
- Sandbox availability on non-Linux platforms does not block this decision because the
  primary v1 backstop is recovery, not confinement.
- Package/script runners remain an explicit follow-up decision rather than a noisy v1
  default.
- The v1 inline-vs-script-file resolution is a bounded heuristic, not exhaustive. To
  distinguish an interpreter's executed payload from a value-consuming option's argument
  (`node --require ./preload.js -e "code"` — `./preload.js` is `--require`'s value, not the
  executed script), the resolver carries a bounded per-interpreter table of value-consuming
  options. Real flags outside that table (`node --conditions ./preload.js -e "code"`) can
  still spoof the script-file slot and read as effect-opaque. This is accepted as a v1
  limitation because the error direction is fail-safe: when recovery is available, a
  misclassified benign command only earns an extra pre-exec snapshot without risk
  escalation or another confirmation; when required recovery is unavailable, the loud /
  fail-closed rule above applies and may interrupt it. The opposite error — conservatively
  treating every unknown option as value-consuming — would
  skip a real script file (`node --inspect ./script.js`) and drop its recovery snapshot,
  which is fail-open and is therefore rejected. A general resolver would need per-flag
  arity knowledge no text-only heuristic can supply.
