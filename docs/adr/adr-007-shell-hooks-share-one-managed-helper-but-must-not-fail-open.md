# ADR-007 — Shell hooks share one managed helper, but must not fail open

## Status

Accepted

## Decision

Installed Claude / Codex hooks use the shared helper path:

- `~/.aegis/lib/toggle-state.sh`

The repository template lives at:

- `scripts/hooks/toggle-state.sh`

However, the installed hooks also embed minimal fallback CI / disabled-state
logic so helper loss does not create a fail-open enforcement bypass.

## Why

- shared logic keeps the normal path consistent
- inline fallback prevents helper-missing regressions from silently disabling
  Codex / Claude enforcement behavior

## Implication

- any semantic change to hook-side toggle detection must keep the shared helper
  and inline fallback behavior aligned
