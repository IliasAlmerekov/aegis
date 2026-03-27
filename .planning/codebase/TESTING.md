# TESTING.md — Test Structure & Practices

## Test Framework

- **Unit tests**: Standard Rust `#[test]` in `#[cfg(test)]` modules within each source file
- **Integration tests**: `tests/` directory, compiled as separate crates with access to the binary
- **Benchmarks**: `benches/scanner_bench.rs` via `criterion 0.5`
- **Fuzz testing**: Required for `parser.rs` (not yet present — planned with `libfuzzer-sys`)

## Test Locations

| Type | Location | Purpose |
|---|---|---|
| Unit | `src/**/*.rs` (`#[cfg(test)]` module) | Pure logic, edge cases |
| Integration | `tests/full_pipeline.rs` | Full binary subprocess tests |
| Integration | `tests/docker_integration.rs` | Docker snapshot plugin |
| Benchmarks | `benches/scanner_bench.rs` | Scanner hot-path latency |

## Integration Test Pattern (`tests/full_pipeline.rs`)

Tests spawn the actual compiled binary as a subprocess:

```rust
fn aegis_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aegis"))
}

fn base_command(home: &Path) -> Command {
    let mut command = Command::new(aegis_bin());
    command.env("AEGIS_REAL_SHELL", "/bin/sh");
    command.env("AEGIS_CI", "0");  // Disable CI fast-path in tests
    command.env("HOME", home);
    command
}
```

Key conventions:
- `AEGIS_CI=0` to prevent CI host environment from affecting expected exit codes
- `AEGIS_FORCE_INTERACTIVE=1` to exercise the TUI dialog path even in non-interactive shells
- `TempDir::new()` for home isolation — each test gets a fresh `~/.aegis/` directory
- Audit log read-back via `read_audit_entries(home)` for decision verification

## Exit Code Contract (tested explicitly)

| Code | Meaning |
|---|---|
| 0 | Success — command approved and exited 0 |
| 1-N | Pass-through from child process |
| 2 | Denied — user pressed 'n' |
| 3 | Blocked — Block-level pattern matched |
| 4 | Internal error — Aegis itself failed |

## Coverage Requirements

- All `RiskLevel` variants must have positive AND negative test cases
- Every `Pattern` must have at least one test asserting it fires correctly
- `parser.rs` edge cases: heredoc, inline scripts, escaped quotes, pipes
- `scanner.rs`: safe, warn, danger, block paths tested separately

## Unit Test Style (`main.rs` example)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_resolution_aegis_real_shell_takes_priority() {
        let result = resolve_shell_inner(
            Some(std::ffi::OsStr::new("/usr/bin/zsh")),
            Some(std::ffi::OsStr::new("/bin/bash")),
            None,
        );
        assert_eq!(result, PathBuf::from("/usr/bin/zsh"));
    }
}
```

Pure inner functions extracted from env-reading outer functions to enable deterministic unit testing.

## Benchmarks (`benches/scanner_bench.rs`)

Three benchmark groups:

1. **`1000_safe_commands`** — Aho-Corasick only path, no regex; target > 500k ops/sec
2. **`100_dangerous_commands`** — Full pipeline (AC + regex), all 7 categories
3. **`heredoc_worst_case`** — 200-line inline script, dangerous pattern near the end

Run with:
```bash
rtk cargo criterion
```

## Fuzz Targets (Required, Not Yet Implemented)

```toml
# fuzz/Cargo.toml — planned
[[bin]]
name = "fuzz_scanner"
path = "fuzz_targets/scanner.rs"
```

Targets required:
- `parser::parse` — shell tokenizer edge cases
- `scanner::assess` — full pipeline fuzzing
- Heredoc unwrapping

## Security Auditing (CI-enforced)

```bash
rtk cargo audit   # CVE scan — must pass before merge
rtk cargo deny check  # License + duplicate policy
```

Both must pass in CI. A build with known CVEs does not ship.

## Fixture Data

- `tests/fixtures/commands.toml` — 70 test cases minimum for v1 (planned)
- Each fixture: raw command string → expected `RiskLevel`

## Test Invocation

```bash
rtk cargo test                      # All tests
rtk cargo test --test full_pipeline # Integration only
rtk cargo criterion                 # Benchmarks
```
