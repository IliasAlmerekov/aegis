# TODO — Aegis Senior Review

## P0 — Critical before trusting this as a security tool

- [x] Change scanner initialization failure from **fail-open** to **fail-closed** or at least **warn-and-deny by default**.
  - Current behavior falls back to `RiskLevel::Safe` when scan initialization fails.
  - Required outcome:
    - either block execution with clear error,
    - or require explicit user approval for every command until scanner is healthy.

- [x] Decide and document the exact security model.
  - Clarify that Aegis is:
    - a heuristic command guardrail,
    - not a sandbox,
    - not a complete security boundary.
  - Add explicit non-goals:
    - obfuscated shell,
    - indirect execution,
    - script-generated commands,
    - alias/function expansion bypasses,
    - encoded payloads.

- [x] Fix the `Block` flow to match product behavior.
  - README says `Block` means immediate denial without dialog.
  - Code currently calls confirmation UI for `Block`.
  - Pick one behavior and make code + docs consistent.

- [x] Add regression tests for security-critical failure modes.
  - scanner init failure
  - config parse failure
  - snapshot runtime init failure
  - audit logger failure
  - confirmation UI failure
  - shell resolution failure

---

## P1 — Correctness and trustworthiness

- [x] Fix config loading semantics.
  - README says config is merged from:
    1. project `.aegis.toml`
    2. global `~/.config/aegis/config.toml`
    3. defaults
  - Current implementation selects the first existing file.
  - Implement real layered merge or rewrite docs to reflect true behavior.

- [x] Add tests for layered config precedence.
  - global only
  - project only
  - both present
  - partial override cases
  - malformed project config with valid global fallback behavior

- [x] Remove all README claims that are not yet proven by tests/benchmarks.
  - “< 2ms overhead”
  - throughput numbers
  - “55 built-in patterns”
  - agent compatibility claims
  - any public incident references that are not sourced

- [x] Add exact source links for incident claims in README.
  - Replit case
  - DataTalks.Club case
  - Prisma/community case
  - If unverifiable, remove them.

- [x] Audit all docs for implementation drift.
  - `Block` behavior
  - merged config wording
  - rollback semantics
  - Docker snapshot guarantees
  - “works without friction” claims

---

## P1 — Snapshot system hardening

### Git plugin

- [x] Replace string-based clean-tree detection with a locale-independent mechanism.
  - Do not rely on `"No local changes to save"` text matching.
  - Prefer a deterministic check such as:
    - `git status --porcelain`
    - or other structured signal.

- [x] Improve Git repository detection.
  - Current check only tests `cwd/.git`.
  - Support:
    - worktrees,
    - nested repos,
    - submodules,
    - `.git` file pointers,
    - running inside subdirectories of a repo.

- [x] Add tests for Git edge cases.
  - running from repo subdirectory
  - worktree
  - clean repo
  - untracked files
  - staged + unstaged changes
  - stash conflict on rollback

- [x] Define rollback conflict strategy.
  - What happens if `git stash pop --index` conflicts?
  - Surface clear recovery instructions.
  - Log enough context for manual restore.

### Docker plugin

- [x] Redesign Docker rollback semantics.
  - `docker commit` + `docker run -d image` is **not** a true environment rollback.
  - It does not restore:
    - ports,
    - volumes,
    - env vars,
    - network attachments,
    - restart policy,
    - labels,
    - container name,
    - compose metadata.
  - Either:
    - reduce claims in docs,
    - or implement real metadata capture + replay.

- [x] Capture container configuration before snapshot.
  - Inspect and persist:
    - image
    - command / entrypoint
    - env
    - mounts
    - ports
    - network mode
    - labels
    - restart policy
    - name

- [x] Add rollback strategy for named containers.
  - Handle name collisions.
  - Handle already removed containers.
  - Handle networks that no longer exist.

- [ ] Add integration tests against real Docker, not only mocked CLI.
  - Mock tests are useful but insufficient for lifecycle correctness.

- [ ] Make Docker snapshot behavior opt-in until real rollback guarantees exist.

---

## P1 — Command classification quality

- [ ] Add explicit parser/normalization tests for bypass-prone command forms.
  - subshells
  - `sh -c`
  - `bash -lc`
  - heredocs
  - pipes
  - command substitution
  - env-prefixed commands
  - multiline input
  - quoted fragments
  - semicolon/&&/|| chains

- [ ] Define how classification works for compound commands.
  - Example:
    - `echo ok && rm -rf /tmp/x`
  - Must classify by highest-risk segment, not first token only.

- [ ] Add coverage for encoded/indirect execution patterns.
  - `echo <payload> | sh`
  - `python -c`
  - `node -e`
  - `perl -e`
  - `eval "$VAR"`
  - process substitution

- [ ] Review allowlist design.
  - Ensure allowlist cannot silently neutralize catastrophic patterns too broadly.
  - Add support for previewing why a command matched allowlist.

- [ ] Add “why matched” diagnostics.
  - show matched pattern IDs
  - matched substring
  - category
  - safe alternative
  - final decision source:
    - built-in pattern
    - custom pattern
    - allowlist
    - fallback

---

## P2 — Reliability and UX

- [ ] Add structured exit-code contract.
  - distinguish:
    - command denied
    - command blocked
    - internal Aegis failure
    - underlying shell failure
  - document these exit codes.

- [ ] Harden shell resolution.
  - Current fallback to `/bin/sh` is pragmatic but should be documented.
  - Add tests for:
    - `SHELL` pointing to Aegis itself
    - invalid shell path
    - missing `AEGIS_REAL_SHELL`
    - recursive invocation prevention

- [ ] Add non-interactive mode handling.
  - If stdin is not a TTY:
    - define exact behavior for `Warn`, `Danger`, `Block`.
  - Important for CI and agent runners.

- [ ] Implement explicit CI policy behavior.
  - Example:
    - block destructive commands in CI by default,
    - or require policy override.

- [ ] Improve audit timestamps.
  - Current logger stores Unix seconds only.
  - Prefer RFC 3339 / ISO 8601 with timezone and maybe monotonic sequence info.

- [ ] Improve audit querying scalability.
  - Current implementation reads full JSONL into memory.
  - Add streaming/tail-oriented querying for large logs.

- [ ] Add log rotation strategy.
  - size-based or date-based
  - optional compression
  - retention settings

- [ ] Add machine-readable audit export options.
  - JSON
  - NDJSON filtering
  - maybe jq-friendly modes

---

## P2 — Release and supply-chain readiness

- [ ] Validate release workflow end to end with a real tag.
  - README/TODO already mention this as unfinished.

- [ ] Add checksum verification to installer.
  - Do not only download artifacts.
  - Verify SHA256 before install.

- [ ] Reconsider the `curl | sh` install path in a security-oriented product.
  - At minimum:
    - publish checksums,
    - document manual install,
    - recommend verification path first.

- [ ] Add reproducible release notes.
  - artifact list
  - checksums
  - supported targets
  - changelog

- [ ] Add supply-chain checks to CI.
  - `cargo audit`
  - `cargo deny`
  - license review
  - minimal versions / outdated dependency checks

- [ ] Add crate publishing validation.
  - `cargo publish --dry-run`
  - package content review
  - README rendering check

---

## P2 — Product clarity

- [ ] Rework README positioning.
  - Current messaging is strong, but some claims sound more mature than implementation is.
  - Rewrite around:
    - “MVP”
    - “local guardrail”
    - “human approval layer”
    - “best-effort snapshots”

- [ ] Add a clear limitations section.
  - This is mandatory for trust.

- [ ] Add architecture diagram based on actual code paths.
  - shell wrapper
  - scanner
  - decision engine
  - snapshots
  - audit logger
  - exec path

- [ ] Add a threat model document.
  - assets protected
  - attacker model
  - trust assumptions
  - bypasses not handled
  - operational recommendations

---

## P3 — Nice to have

- [ ] Add Windows support only after shell interception model is clearly redesigned.
- [ ] Add rollback CLI only after snapshot fidelity is trustworthy.
- [ ] Add remote audit sinks only after local audit format is stable.
- [ ] Add web dashboard only after core security semantics are stable.
- [ ] Add policy DSL only after current policy engine semantics are proven.

---

## Suggested release policy

### Not ready for “security product” messaging until these are done

- [ ] fail-open removed
- [ ] `Block` semantics fixed
- [ ] config/docs consistency fixed
- [x] Docker claims reduced or implementation upgraded
- [ ] limitations/threat-model documented
- [ ] critical regression tests added

### Acceptable for “public MVP” once these are done

- [ ] installer checksum verification
- [ ] real release tag tested
- [ ] docs aligned with implementation
- [ ] non-interactive behavior documented
- [ ] audit/query basics stable

---

## Final recommendation

**Current state:** strong prototype / public MVP candidate  
**Not yet:** trustworthy security boundary or production-grade protection layer
