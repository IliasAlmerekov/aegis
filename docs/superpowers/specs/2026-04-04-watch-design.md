# Design: `aegis watch` — NDJSON framed watch mode

**Date:** 2026-04-04
**Status:** Approved
**Ticket:** 1.5 — Удалить или реализовать `watch`

---

## Overview

`aegis watch` implements a long-lived, stdin-driven command interception loop.
The caller (e.g. an AI agent) sends NDJSON frames describing commands; Aegis
assesses each command, optionally prompts a human for approval via `/dev/tty`,
executes approved commands, and streams results back as NDJSON frames on stdout.

This replaces the current stub that prints "watch: not yet implemented" and
exits 0.

---

## I/O Contract

The watch mode process has three distinct I/O channels:

| Channel | Role |
|---|---|
| **stdin** | Control stream — NDJSON command frames in |
| **stdout** | Event stream — NDJSON chunk/result frames out |
| **stderr** | Aegis internal diagnostics only (silent by default) |
| **`/dev/tty`** | All human-facing UI (dialog, block notices, policy messages) |

stdout is a machine-readable protocol. Nothing human-facing ever goes there.

---

## Wire Protocol

### Input frame (stdin, one JSON object per line)

```json
{"cmd":"git status"}
{"cmd":"rm -rf /tmp/foo","cwd":"/home/user","source":"claude","id":"2"}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `cmd` | string | yes | Shell command to intercept. Non-empty. |
| `cwd` | string | no | Working directory for execution, policy, snapshots, and audit. |
| `interactive` | bool | no | Reserved — ignored in v1. |
| `source` | string | no | Metadata written to audit log (e.g. agent name). |
| `id` | string | no | Correlation ID echoed on all output frames for this command. |

**Frame size limit:** 1 MiB. Enforced with a bounded codec at read time — not
after allocation. Oversized frames are rejected with an error result frame.

**Unknown fields** are silently ignored for forward compatibility.

### Output frames (stdout, NDJSON)

#### Chunk frames (emitted during child execution)

```json
{"type":"stdout","id":"1","data_b64":"T24gYnJhbmNoIG1haW4K"}
{"type":"stderr","id":"1","data_b64":"d2FybmluZzogTEYgd2lsbCBiZSByZXBsYWNlZAo="}
```

Child output is base64-encoded (`data_b64`) to safely handle non-UTF-8 bytes.

**Ordering contract:** ordering is preserved _within_ each stream (all stdout
chunks are in order; all stderr chunks are in order). Cross-stream ordering
between stdout and stderr is not guaranteed.

#### Result frame (one per input frame, always the last frame for that command)

```json
{"type":"result","id":"1","decision":"approved","exit_code":0}
{"type":"result","id":"2","decision":"denied","exit_code":2}
{"type":"result","id":"3","decision":"blocked","exit_code":3}
{"type":"result","decision":"error","exit_code":4,"message":"missing cmd field"}
```

| `decision` | `exit_code` | Meaning |
|---|---|---|
| `approved` | child exit code | Command executed and completed |
| `denied` | 2 | User denied at confirmation dialog |
| `blocked` | 3 | Command matched a Block-level pattern |
| `error` | 4 | Aegis could not process the frame |

The `id` field is omitted on error frames when the input frame had no `id` or
was unparseable.

---

## Error Handling

| Condition | Behavior |
|---|---|
| Invalid JSON | Emit error result frame, continue loop |
| Missing or empty `cmd` | Emit error result frame, continue loop |
| Frame exceeds 1 MiB | Emit error result frame, continue loop |
| `cwd` invalid / not a directory | Emit `decision:"error"`, `message:"invalid cwd"`, continue loop |
| `/dev/tty` unavailable, dialog needed | Fail-closed: `Warn`/`Danger` → `denied` (exit 2), `Block` → `blocked` (exit 3); emit correct result frame |
| Child spawn fails | Emit error result frame, continue loop |
| Child killed by signal | `exit_code` = 128 + signal number (Unix convention) |
| **stdout write failure** | **Terminal — end watch mode immediately** (broken control channel) |
| stdin EOF | Drain any in-flight command, then exit 0 |

**Audit behavior for malformed frames:** malformed/invalid frames are audited
as watch-mode errors even when no command executes.

---

## Architecture

### Module structure

- **`src/watch.rs`** — new module; owns the watch loop and frame I/O
- **`src/ui/confirm.rs`** — updated to open `/dev/tty` for both input
  (keystrokes) and output (dialog rendering) instead of process stdin
- **`src/main.rs`** — remains thin; `Commands::Watch` arm calls `watch::run()`
- **`src/runtime.rs`** — `RuntimeContext` grows an async snapshot path to
  avoid nested `block_on` inside the watch loop

### Async runtime

Watch mode owns the tokio runtime (`tokio::runtime::Builder::new_multi_thread`). `RuntimeContext` currently creates
its own runtime internally for `create_snapshots()`; in watch mode this is
replaced with an async-native path to avoid the `block_on`-inside-async
problem.

### Per-command flow

```
read bounded line from stdin
  → parse JSON frame
  → validate: cmd non-empty, cwd exists (if given), frame size
  → assess(cmd, cwd) — scanner, policy evaluation
  → if dialog needed:
      → open /dev/tty
      → if /dev/tty unavailable → fail-closed result frame
      → else show TUI dialog
  → if approved:
      → spawn child:
          stdin  = /dev/null
          stdout = piped → pump task A → channel
          stderr = piped → pump task B → channel
          cwd    = frame.cwd or process cwd
      → emitter task: drain channel, serialize NDJSON, write to stdout
      → wait for child exit
  → emit result frame
  → append audit entry
  → loop
```

### Single stdout emitter

Stdout-pump task and stderr-pump task each send `WatchEvent` enum variants
into a `tokio::sync::mpsc` channel. One emitter task serializes all NDJSON
output to process stdout in order. This prevents frame interleaving.

```rust
enum WatchEvent {
    Stdout { id: Option<String>, data: Vec<u8> },
    Stderr { id: Option<String>, data: Vec<u8> },
    Result { id: Option<String>, decision: Decision, exit_code: i32 },
    Error  { id: Option<String>, message: String },
}
```

### Child stdin policy

Child stdin is `/dev/null` by default in v1. Future versions may support
`stdin_mode: "tty"` (child gets `/dev/tty`) or streamed stdin frames.

### `/dev/tty` fallback

All human-facing output in watch mode (confirmation dialog, block notices,
policy messages) goes to `/dev/tty`. If `/dev/tty` cannot be opened:

- `Warn` / `Danger` commands → `decision:"denied"`, `exit_code:2`
- `Block` commands → `decision:"blocked"`, `exit_code:3`
- Safe commands are unaffected (no dialog needed)

---

## Testing

### Unit tests

- Frame parser: valid frame, invalid JSON, missing `cmd`, empty `cmd`,
  oversized frame, unknown fields ignored, `id` round-trip, `cwd` field
- Bounded reader: frame at exactly 1 MiB passes; frame at 1 MiB + 1 byte
  rejected before allocation
- TUI `/dev/tty` fallback: when `open("/dev/tty")` fails, confirm returns the
  correct fail-closed decision
- `WatchEvent` serialization: `data_b64` is valid base64, `id` echoed correctly

### Integration tests

- `echo '{"cmd":"echo hello","id":"1"}' | aegis watch` — stdout contains
  `stdout` chunk with base64 of "hello\n" and result frame
  `{decision:"approved",exit_code:0,id:"1"}`
- Denied command (user presses 'n' at dialog) — result frame
  `{decision:"denied",exit_code:2}`
- Blocked command — result frame `{decision:"blocked",exit_code:3}`
- Invalid JSON mid-stream — error result emitted, next valid frame processed
- Missing `cmd` field — error result emitted, loop continues
- Invalid `cwd` — error result with `message:"invalid cwd"`, loop continues
- Non-UTF-8 child output — `data_b64` round-trips correctly
- Oversized frame — rejected before allocation, loop continues
- `/dev/tty` unavailable (mocked) — `Warn` command → `denied`, `Block`
  command → `blocked`; correct result frames emitted
- Signal termination: spawn `sleep 10`, send SIGTERM, expect `exit_code:143`
  (128 + 15) in result frame
- stdout sink failure / broken pipe — watch mode exits, does not loop
- Audit entries written for denied, blocked, and error watch frames

---

## What is not in v1

- `interactive` field (reserved, ignored)
- `stdin_mode` / streamed stdin frames for child input
- Concurrent command execution (strictly serial in v1)
- Windows support (`/dev/tty` is Unix-only; watch mode is Unix-only for now)
