# CONCERNS.md — Technical Debt & Known Issues

## Critical Issues

### 1. `Watch` Subcommand Not Implemented

**File:** `src/main.rs:110`
**Impact:** High — `aegis watch` silently exits with code 0, misleading users
**Detail:**

```rust
Some(Commands::Watch) => {
    println!("watch: not yet implemented");
    0
}
```

**Fix:** Implement or gate behind a feature flag with a clear error.

### 2. `Mode::Audit` and `Mode::Strict` Are Dead Code

**File:** `src/config/model.rs`
**Impact:** High — config file accepts `mode = "Audit"` and `mode = "Strict"` but runtime only implements `Protect`
**Detail:** The `Mode` enum is defined and deserialized but no code path branches on `Audit` or `Strict`; the `#[allow(dead_code)]` in `parser.rs` hints at this pattern
**Fix:** Either implement the modes or reject them at config load time with a clear error rather than silently falling back to Protect behavior

### 3. Silent Snapshot Failures

**File:** `src/runtime.rs`, `src/snapshot/`
**Impact:** Medium — snapshot creation errors are logged but the command dialog still proceeds; user may believe rollback is available when it isn't
**Detail:** The concerns agent noted that `create_snapshots` returns an empty Vec on runtime failure — this is intentional per the fail-open design, but it means snapshot unavailability is invisible to the user at decision time
**Fix:** Surface snapshot failures in the confirmation dialog so users can make an informed decision

---

## Security Concerns

### 4. Allowlist Bypass at Danger Level

**File:** `src/main.rs` (`decide_command`)
**Impact:** Medium — allowlisted commands at `Danger` level skip the confirmation dialog and execute automatically; a misconfigured allowlist silently approves destructive commands
**Detail:** `Decision::AutoApproved` path for allowlisted Danger commands
**Fix:** Consider audit-only auto-approve (log + warn) rather than silent execution; at minimum the audit log records it

### 5. No Allowlist Pattern Validation at Load Time

**File:** `src/config/`
**Impact:** Low-Medium — invalid regex in allowlist entries is not caught until the first command is assessed; misconfigured patterns silently fail to match
**Fix:** Validate allowlist patterns at `Config::load()` and surface errors immediately

### 6. `parser.rs` Has No Fuzz Coverage

**File:** `src/interceptor/parser.rs`
**Impact:** Medium — the shell tokenizer is security-critical (heredoc, inline scripts, quoting) but has no fuzz targets yet
**Fix:** Add `fuzz/` directory with `fuzz_targets/scanner.rs`; this is listed as required in `CONVENTION.md`

---

## Performance Concerns

### 7. Pattern Compilation at `Scanner::new()` Is Unbounded

**File:** `src/interceptor/scanner.rs`
**Impact:** Low — all pattern regexes are compiled at Scanner construction; as the pattern set grows, startup time increases
**Detail:** Current approach is correct (compile once, reuse), but there is no cap on pattern count or compilation timeout
**Fix:** Add a cap or lazy-compile individual patterns on first use once pattern count grows

---

## Technical Debt

### 8. `#[allow(dead_code)]` in `parser.rs`

**File:** `src/interceptor/parser.rs:3`
**Impact:** Low — suppresses warnings for several helper functions that are implemented but not yet wired up (e.g., some heredoc helpers)
**Fix:** Either wire up or remove the unused code before v1 release

### 9. Fixture File Not Present

**File:** `tests/fixtures/commands.toml`
**Impact:** Medium — `CONVENTION.md` requires 70 fixture test cases for v1; this file does not yet exist
**Fix:** Create `tests/fixtures/commands.toml` with ≥70 entries covering all categories

### 10. `tests/docker_integration.rs` Likely Skipped in Most Environments

**File:** `tests/docker_integration.rs`
**Impact:** Low — Docker integration tests require a running Docker daemon; they will be silently skipped or fail in environments without Docker, reducing effective coverage
**Fix:** Gate with `#[cfg_attr(not(feature = "docker-tests"), ignore)]` or document the requirement clearly

### 11. `DockerPlugin` May Not Be Tested End-to-End

**File:** `src/snapshot/docker.rs`
**Impact:** Low-Medium — Docker snapshot path is opt-in (`auto_snapshot_docker = false` default) and likely receives less test coverage than GitPlugin
**Fix:** Add integration tests or mocked unit tests covering the Docker snapshot and rollback path

### 12. `TODO.md` Is the Authoritative Backlog (in Russian)

**File:** `TODO.md`
**Impact:** Low — the main backlog is written in Russian with a multi-phase structure; contributors unfamiliar with Russian may miss context
**Note:** This is by design per project conventions; all P1 work is tracked there

---

## Fragile Areas

### 13. Shell Resolution Recursion Guard (`resolve_shell_inner`)

**File:** `src/main.rs`
**Impact:** Medium — if Aegis is installed as `$SHELL` and the recursion guard fails (e.g., due to symlinks or path canonicalization failure), infinite recursion results
**Mitigation:** `same_file()` uses `fs::canonicalize` to catch symlink cases; tested in unit tests
**Risk:** Canonicalization can fail on network filesystems or unusual mounts

### 14. Exit-Code Leakage at Block Level

**File:** `src/main.rs`
**Impact:** Low — `EXIT_BLOCKED=3` is returned for both Block-level pattern matches and CI policy blocks; callers cannot distinguish the two cases without reading the audit log

---

## Dependency Risks

### 15. `aho-corasick` and `regex` Are Core Security Dependencies

**Files:** `Cargo.toml`
**Impact:** Medium — bugs in these crates (pattern bypass, panic on malformed input) directly affect Aegis correctness
**Mitigation:** `cargo audit` and `cargo deny` are CI-enforced
