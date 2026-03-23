---
name: research_codebase
description: Pure read-only research phase for a ticket. Spawns 4 parallel researcher sub-agents covering architecture, domain logic, integration boundaries, and conventions. No code changes. Outputs structured facts to docs/{ticket_id}/research.md.
allowed_tools: ["Read", "Grep", "Glob", "Bash", "Agent"]
---

# Command: research_codebase

## PURPOSE

Execute a pure research phase for a ticket. No code changes. No suggestions.
Only structured facts about the current Aegis codebase relative to the ticket's scope.

## INVOCATION

```
/research_codebase $INPUT
```

Where `$INPUT` is the raw ticket text (description, acceptance criteria, context).
Example: `/research_codebase "AEGIS-042: Add unicode normalization to command parser"`

## PRECONDITIONS

None. This is always the first command run for any new ticket.

---

## LEAD AGENT BEHAVIOR

### Step 1 — Ticket Parsing

Extract from `$INPUT`:

- `ticket_id` — string identifier (e.g. `AEGIS-042`). If absent, generate `RESEARCH-{timestamp}`.
- `feature_domain` — which part of the system is involved:
  - `interceptor` — scanner, parser, patterns (Aho-Corasick + regex, `assess()`, `RiskLevel`)
  - `snapshot` — `SnapshotPlugin` trait, `GitPlugin`, `DockerPlugin`, rollback logic
  - `ui` — crossterm TUI confirmation dialog (`ui/confirm.rs`)
  - `audit` — append-only JSONL logger (`AuditEntry`, `~/.aegis/audit.jsonl`)
  - `config` — `AegisConfig`, TOML loading (`aegis.toml` / `.aegis.toml`)
  - `cli` — `main.rs`, clap arg parsing, shell proxy wiring
- `affected_modules` — best-guess from ticket text; to be verified by researcher agents.
- `user_facing_impact` — what changes for the end user or the calling AI agent.

Create `docs/{ticket_id}/` if it does not exist.

### Step 2 — Spawn 4 Parallel Researcher Sub-Agents

Each researcher receives the full ticket text and **read-only** repository access.
Each operates with a distinct, non-overlapping investigation mandate.

---

**researcher:architecture** — _module and type structure_

Investigate:

- Which modules are involved? (`src/interceptor/`, `src/snapshot/`, `src/audit/`, `src/config/`, `src/ui/`, `src/error.rs`)
- Which public types and traits are touched? Key types to check: `RiskLevel`, `Pattern`, `BuiltinPattern`, `UserPattern`, `Assessment`, `ParsedCommand`, `SnapshotPlugin`, `AuditEntry`, `AegisConfig`, `AegisError`.
- Which trait impls are affected? (`SnapshotPlugin` implementors: `GitPlugin`, `DockerPlugin`)
- What is the call graph from `main.rs` through to the affected area?
- Are any `Cow<'static, str>` / `Arc<Pattern>` ownership boundaries crossed?

---

**researcher:domain** — _command interception logic_

Investigate:

- Trace the full execution path from raw command string to final decision:
  `main.rs` → shell arg parsing → `Scanner::assess()` → `RiskLevel` → TUI dialog or passthrough → `tokio::process` child spawn → stdin/stdout/stderr relay → exit code forwarding.
- How does the two-pass scan work today? Aho-Corasick first pass (keyword match), regex second pass (precision filter via `LazyLock<Regex>`).
- How does `ParsedCommand` tokenize the input? What does the parser handle: heredoc, inline scripts, pipes, escaped quotes, env var expansion?
- What are all `RiskLevel` variants and how does the current logic decide among `Safe`, `Warn`, `Danger`, `Block`?
- How does the snapshot subsystem integrate with the interception flow? When is `snapshot()` called relative to the confirmation dialog?

---

**researcher:integration** — _external boundary scope_

Investigate all integration points and their failure modes:

| Boundary                               | Location                                | Fail-open or fail-closed? |
| -------------------------------------- | --------------------------------------- | ------------------------- |
| Shell stdin/stdout/stderr relay        | `main.rs` / tokio child                 | ?                         |
| Child process spawn                    | `tokio::process::Command`               | ?                         |
| Audit log append                       | `~/.aegis/audit.jsonl`                  | ?                         |
| Config file load                       | `aegis.toml`, `.aegis.toml`             | ?                         |
| Terminal raw mode (crossterm)          | `ui/confirm.rs`                         | ?                         |
| Signal handling (Ctrl-C during dialog) | ?                                       | ?                         |
| Snapshot backends (git, docker)        | `snapshot/git.rs`, `snapshot/docker.rs` | ?                         |

For each boundary: document the current failure mode. Does Aegis fail open (let command through) or fail closed (block command) if the boundary is unavailable?

---

**researcher:patterns** — _conventions in this codebase_

Investigate what conventions exist today so the implementation follows them exactly:

- **Error handling**: `thiserror` in lib modules, `anyhow` in `main.rs`. How are errors currently propagated across async boundaries? Are there `?` chains across `async fn` in trait impls?
- **Async**: `#[async_trait]` on `SnapshotPlugin`. Any other traits with async methods? How is `tokio` runtime started in `main.rs`?
- **Static data**: How are `BuiltinPattern` entries defined? `&'static str` fields, `LazyLock<Regex>` for regex patterns. Show one complete existing example.
- **Testing**: Are unit tests in `#[cfg(test)]` blocks inline or in `tests/`? What fixture format is used in `tests/fixtures/commands.toml`? Show the schema of one existing fixture entry.
- **Tracing**: What `tracing` events exist today? What span names and field names are used? What log level is used for which events?
- **Naming**: Show 3 existing pattern IDs and their format. Show how `Category` variants are named.

Produce a **conventions card** — a compact reference the implement agent can paste into its context.

---

### Step 3 — Merge and Write

1. Collect outputs from all 4 agents.
2. Deduplicate file paths in `## Files Involved` — union, no duplicates.
3. Merge all `## Open Questions` — renumber sequentially, deduplicate by meaning.
4. Append researcher:patterns output as `## Conventions to Follow`.
5. Write to `docs/{ticket_id}/research.md` with exactly these 8 sections:

```markdown
# Research: {ticket_id}

## Ticket Summary

## Feature Domain

## Affected Modules

## Files Involved

## Execution Path

## Integration Boundaries

## Open Questions

## Conventions to Follow
```

### Step 4 — Strip Recommendations

Before writing, scan merged output for suggestion language:
"could", "should", "would be better", "recommend", "improve", "suggest", "consider".
Rewrite as neutral factual statements or remove entirely.

### Step 5 — Completion Check

Verify `docs/{ticket_id}/research.md` exists and has all 8 sections.

Print:

```
✅ research_codebase complete
Ticket:              {ticket_id}
Files investigated:  {count}
Open questions:      {count}
Conventions noted:   {count}
Next step:           /plan {ticket_id}
```

---

## HARD RULES

- Phase is complete ONLY when `docs/{ticket_id}/research.md` is written with all 8 sections.
- **ZERO** code changes permitted during this phase.
- **ZERO** suggestions or recommendations in output — strip before writing.
- If any researcher returns "you should" or "I recommend", strip before merging.
- If the repository is inaccessible, write `BLOCKED: repository inaccessible — {reason}` to `docs/{ticket_id}/research.md` and halt.
