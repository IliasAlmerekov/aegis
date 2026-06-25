# TASKS — Reviewer Security Findings Blocking Aegis 1.0

> Source: ultra-deep security audit report from reviewer, dated 2026-06-23.
> Branch: `feat/shell-security` · version path: `0.5.8` → `1.0.0`.
>
> Method summary: 7 parallel adversarial agents across critical surfaces plus
> independent verification of key findings against real `assess()`, policy engine,
> and git state.
>
> **Second source (appended 2026-06-24):** manual crash-test of the released
> `aegis 0.5.9`. Dangerous commands from `PROMPTS.md` were fed one-by-one as
> top-level `Bash` calls so the aegis hook assessed each independently; reactions
> were observed on an isolated git-backed stand (fully reversible). This run
> surfaced the C4 token-prefix anchoring bypass below and confirmed several
> pattern-coverage gaps.

## Release verdict

**Do not ship to production.** The core product promise — "a dangerous command
will not execute without confirmation" — is currently bypassable in at least five
trivial ways that require no preparation. Several bypasses were reproduced against
the real scanner.

The architectural core is still sound: intrinsic `Block` is unbreakable and policy
precedence is strong. The blockers are coverage and normalization gaps, not a
fundamental design failure. They are fixable with targeted work.

---

## Important local-machine context

1. **Aegis is currently disabled on this machine.** `~/.aegis/disabled` exists
   (`aegis off`). In this mode every command executes directly without scanning.
   During review, one test agent really executed destructive
   `git reset --hard HEAD~5` commands. The repository survived: the agent restored
   it to `84879eb`, the tree was clean, and no commits were lost. Commit
   `1ea55c8` (`chore: replace aegis gif`) above that is an operator commit by
   Ilias Almerekov, not damage.
2. **Rejected false alarm:** `export CI=1` does **not** auto-approve dangerous
   commands. Code review and re-checking show the default `ci_policy = Block`
   makes Aegis stricter (`Danger` → `Block`, exit 3). Do not track this as a
   vulnerability.

---

## P0 — Critical release blockers

### [x] C1 — Uppercase bypasses all regex patterns

- **Risk:** Critical.
- **Status:** confirmed on real `assess()`.
- **Evidence:** `RM -RF /` → `Safe` while lowercase `rm -rf /` → `Block`.
  Similar uppercase variants of `DD if=/dev/zero of=/dev/sda`,
  `MKFS.ext4 /dev/sda`, `SHRED`, `FIND / -delete`, `CHMOD 777 ...` also return
  `Safe`.
- **Root cause:** `scanner/mod.rs:141-144` builds Aho-Corasick with
  `ascii_case_insensitive(true)`, so the fast pass hits. Regexes from
  `patterns.toml` are compiled without `(?i)` / case-insensitive mode
  (`scanner/mod.rs:91`), so the verification pass silently misses and falls
  through to `Safe`.
- **Fix:** compile all built-in regexes with `RegexBuilder::case_insensitive(true)`
  or add `(?i)` consistently. Add regression tests for uppercase variants of each
  `Danger` / `Block` pattern.
- **Resolution:** built-in regex patterns are compiled case-insensitively, with regression tests for uppercase destructive commands and custom-pattern case sensitivity.

### [x] C2 — `$IFS` obfuscation bypasses most patterns

- **Risk:** Critical.
- **Status:** resolved.
- **Evidence:** `rm$IFS-rf$IFS/` → `Safe`; `rm${IFS}-rf${IFS}/` → `Safe`;
  `dd${IFS}of=/dev/sda` → `Safe`. In a real shell, `$IFS` is whitespace, so these
  execute as destructive commands. The bypass composes through `bash -c`, heredoc,
  and process substitution.
- **Root cause:** `tokenizer.rs` keeps `rm$IFS-rf$IFS/` as a single token; the
  normalized command has no spaces, so regexes do not match. `$IFS` is a
  deterministic shell word-splitting primitive, not an unknown variable.
- **Fix:** treat literal `$IFS` and `${IFS}` as separators in tokenization or
  normalization. This is cheap and does not require full variable expansion.
- **Resolution:** `split_tokens` now treats unquoted literal `$IFS` / `${IFS}` as
  shell word-separators via the `ifs_marker_len` helper (bare `$IFS` only at an
  identifier boundary; braced `${IFS}` self-delimited). The fix flows through
  `Parser::parse` and `logical_segments` into direct, nested-shell (`bash -c` /
  `sh -c`), heredoc, and process-substitution scan paths. Quoted (`'$IFS'`,
  `"$IFS"`), escaped (`\$IFS`), and non-IFS variable forms (`$PATH`, `$IFSHOME`)
  stay opaque. Covered by tokenizer positive/negative tests, parser
  normalized-form tests, and scanner regressions across PS-006, FS-002, FS-003,
  FS-004, and FS-006.

### [ ] C3 — Project-local `.aegis.toml` can weaken Aegis to audit-only

- **Risk:** Critical.
- **Status:** confirmed via live `config show` / `config validate` and merge code
  review (`model.rs:353`, `overlay.mode.unwrap_or(base.mode)`).
- **Attack config:** a repository entered by an AI agent can include:

  ```toml
  mode = "Audit"
  allowlist_override_level = "Danger"
  snapshot_policy = "None"
  ```

  Result: `engine.rs:45` auto-approves all non-intrinsic-`Block` `Warn`/`Danger`
  commands without prompt and without snapshot. `config validate` reports the
  config as valid with no warning.

- **Root cause:** layered config merge is pure "last layer wins" for
  security-critical scalar fields; the project layer is applied last.
- **Fix:** add a restrictive ratchet. Project config may only tighten:
  - `mode` only toward `Strict`
  - `allowlist_override_level`, `ci_policy`, `snapshot_policy`, and
    `sandbox.required` use most-restrictive global/project semantics
  - minimum fallback: loud `config validate` warning for weakening attempts
- **Positive note:** intrinsic `Block` remains unbreakable even under this config.

### [ ] C4 — Token-prefix rules are anchored to the first token; any prefix bypasses them

- **Risk:** Critical.
- **Status:** crash-test confirmed (destructively proven) + root-caused in code.
- **Evidence (commands that really executed in 0.5.9):**

  | Command                       | Why it passed                              |
  | ----------------------------- | ------------------------------------------ |
  | `/usr/bin/git reset --hard`   | absolute path → first token is not `git`   |
  | `cd dir && git reset --hard`  | `git` is not the first token of the line   |
  | `rtk git push` (force push)   | RTK wrapper prefix → first token is `rtk`  |
  | `rtk git stash clear`         | RTK wrapper prefix                         |
  | `rtk git branch -D`           | RTK wrapper prefix                         |
  | `rtk psql … DROP TABLE`       | RTK wrapper prefix (see also H2)           |

  For contrast, the regex-based families (`rm`, `shred`, `chmod`, `docker …`)
  matched correctly even with a prefix: `/usr/bin/rm -rf` and `echo …; rm -rf`
  were both blocked. So the asymmetry is real: **regex patterns match anywhere in
  the normalized string, token-prefix rules only match at the first token.**
- **Destructive proof:** a prefixed `git reset --hard` in a throwaway repo erased an
  uncommitted line (`PRECIOUS UNCOMMITTED WORK`) — Aegis stayed silent and the
  command executed.
- **Root cause:** GIT-001..008, DB-001/2/6/7/8, CL-*, DK-*, PS-001/2/5 were migrated
  from regex patterns to **token-prefix rules** (`builtin_prefix_rules()` in
  `crates/aegis-scanner/src/patterns.rs`). `Scanner::prefix_scan`
  (`crates/aegis-scanner/src/scanner/mod.rs:240-266`) looks the rule up by the
  literal `tokens[0]` of each scan target — there is **no basename normalization**
  (so `/usr/bin/git` ≠ `git`) and **no skipping of launcher/wrapper prefixes**
  (`rtk`, `sudo`, `env`, `command`, `nice`, `xargs`, …, so `rtk git` ≠ `git`).
  Compound forms (`cd dir && git …`, `true; git …`) only get caught if
  segmentation happens to expose `git` as a segment's first token; in practice it
  does not (related to H1's segmentation gaps). The regex families were left in
  `patterns.toml` and still match by substring, which is why they survive prefixes.
  - The RTK-wrapper case is the most dangerous in practice: this environment's RTK
    hook auto-rewrites `git push` / `git branch` / `git stash` / `psql` to
    `rtk <cmd>`, so these destructive operations are unprotected during normal
    work with no attacker effort at all.
- **Fix:**
  1. Normalize the lookup key before `prefix_scan`: take the **basename** of
     `tokens[0]` (strip directory components) so `/usr/bin/git` resolves to `git`.
  2. **Strip known launcher/wrapper prefixes** (`rtk`, `sudo`, `env`, `command`,
     `nice`, `nohup`, `time`, `xargs`, `doas`, …) and re-resolve the real program
     token before lookup — handle chained wrappers (`sudo env … git`).
  3. Ensure compound/segmented commands expose the inner program as a segment
     first token (couple this with the H1 segmentation fix) so `cd … && git …`
     and `… ; git …` are evaluated.
  4. Add explicit regression coverage for RTK-wrapped forms (`rtk git push`,
     `rtk psql …`) and absolute-path / `sudo` / `env` prefixes for every
     token-prefix family, not just git.
- **Note:** this is a structural regression introduced by the regex→token-prefix
  migration. The migrated families (git/db/cloud/docker/process) are all affected,
  not just git; the same `tokens[0]` literal lookup gates them all.

---

## P1 — High severity

### [ ] H1 — Single `&` command segmentation gap

- **Problem:** command segmentation handles major operators but review found a
  gap around single `&` background separators.
- **Status:** reviewer finding.
- **File:** `segmentation.rs:156-165`.
- **Fix:** segment on standalone `&` consistently with other shell control
  operators and add regression tests.

### [ ] H2 — SQL inside `psql -c` / `mysql -e` is not scanned

- **Problem:** `psql -c 'DROP TABLE users'` → `Safe` while bare
  `DROP TABLE users` → `Danger`.
- **Status:** confirmed by reviewer.
- **File:** `nested_shells.rs:39-45`.
- **Fix:** recursively scan SQL passed to `psql -c` / `mysql -e`, or remove overly
  strict prefix anchoring from destructive SQL rules so embedded `DROP` is caught.

### [ ] H3 — Pattern database has dangerous gaps

- **Problem:** the following currently classify as `Safe`: `wipefs -a /dev/sda`,
  `aws s3 rb --force`, `aws s3 sync --delete`, `gsutil rm -r`, appending keys to
  `~/.ssh/authorized_keys`, truncating shell rc files such as `> ~/.bashrc`, and
  `unlink`.
- **Status:** agent-confirmed.
- **Files:** `patterns.toml`, `builtins_a.rs`.
- **Fix:** extend built-in patterns and run through the eval harness.

### [ ] H4 — `claude-code.sh` hook fails open

- **Problem:** when `jq` or `aegis` is missing, or JSON is invalid, the Claude hook
  exits 0, allowing the command to pass without scanning. The Codex hook already
  denies in these cases. This violates ADR-007.
- **Status:** confirmed by reviewer.
- **File:** `scripts/hooks/claude-code.sh:64-77`.
- **Fix:** emit a deny result on missing dependencies / invalid JSON, matching the
  Codex hook behavior.

### [ ] H5 — Audit hash chain is not true tamper-evidence

- **Problem:** the audit hash chain is not keyed and has no external anchor. Anyone
  who can write `audit.jsonl` can rewrite entries and recompute a valid chain;
  truncation from the end and complete reset are not detected. Public
  "tamper-evident" wording is misleading: this is an integrity/corruption check,
  not adversarial tamper-evidence.
- **Status:** agent-confirmed via tests.
- **File:** `logger/integrity.rs:90-133`.
- **Fix:** add HMAC/external anchoring, or change public wording to
  "integrity/corruption check".

### [ ] H6 — Snapshot store lacks containment checks

- **Problem:** `validate_snapshot_path` checks absolute paths and rejects `..`, but
  does not prove the path is contained inside `~/.aegis/snapshots`. This creates an
  arbitrary overwrite/delete primitive. Today it is partially mitigated because
  `snapshot_id` comes from the audit log, not directly from CLI input.
- **Status:** agent-confirmed.
- **Files:** `sqlite.rs:99-115`, `postgres/mod.rs:249`.
- **Fix:** add containment validation, using `supabase/runtime/rollback.rs` as a
  reference pattern.

### [ ] H7 — Database dumps, snapshots, and audit files are too permissive

- **Problem:** DB dumps and snapshot directories are created world-readable
  (`0644` / `0755`) without explicit mode; audit log is similar and follows
  symlinks. Dumps can contain full database contents and credentials.
- **Status:** agent-confirmed.
- **Files:** `postgres/mod.rs:91-110`, `audit/writer.rs:236`.
- **Fix:** directories `0700`, files `0600`, and avoid symlink following for audit
  writes (for example `O_NOFOLLOW` where available).

### [ ] H8 — Git token-prefix rules miss `git push --force`, `git stash clear`, `git branch -D`

- **Problem:** even with the *first token* correctly `git` (no prefix), several
  destructive git subcommands were not flagged in the crash-test:
  - `git push --force` → executed. Note `crates/aegis-scanner/src/scanner/tests/edge_cases.rs:375`
    **explicitly asserts** `["git", "push", "--force"]` must NOT match the prefix
    rules — this is a deliberate decision that needs revisiting, not just a missing
    pattern. A force-push is irreversible history destruction on the remote.
  - `git stash clear` → executed (drops all stashed work, unrecoverable).
  - `git branch -D <branch>` → executed (force-deletes an unmerged branch).
- **Status:** crash-test confirmed.
- **Files:** `crates/aegis-scanner/src/patterns.rs` (`builtin_prefix_rules()` —
  GIT-001..008), `crates/aegis-scanner/src/scanner/tests/edge_cases.rs:375`.
- **Fix:** add token-prefix rules for `git push --force` / `--force-with-lease` /
  `-f`, `git stash clear` (and `git stash drop`), and `git branch -D` / `-D`
  combined `-d --force`; decide and document the intended risk level for force-push
  (likely `Danger`), and update/replace the edge-case test that currently locks in
  the pass-through. Run through the eval harness alongside H3/M5.
- **Depends on / amplified by C4:** until C4 lands, every one of these is *also*
  reachable via `rtk git …` / path prefix regardless of pattern coverage.

---

## P2 — Medium severity

### [ ] M1 — Sandbox degradation is too quiet

- **Problem:** when sandboxing is configured but unavailable and `required = false`
  (default), execution silently degrades to unsandboxed. Warning is only
  `tracing::warn`; without a subscriber the user may not see it. Profiles also
  allow broad `file-read*` / `process*`, so this is a write/network guard, not a
  confidentiality boundary.
- **Files:** `sandbox/lib.rs`, `linux.rs`, `profiles/*.sbpl`.
- **Fix:** make `required = true` the default or always print a loud stderr
  warning; narrow read permissions and document the confidentiality boundary.

### [ ] M2 — User/custom regexes lack size limits

- **Problem:** `Regex::new` without `size_limit` lets project config patterns
  inflate memory/CPU during compilation on the hot path, creating a DoS vector.
- **Files:** `scanner/mod.rs:91`, `allowlist/compile.rs`.
- **Fix:** use `RegexBuilder::size_limit` / `dfa_size_limit` and cap pattern
  length.

### [ ] M3 — In-band kill-switch and wrapper bypass

- **Problem:** `aegis off` is itself treated as safe and creates
  `~/.aegis/disabled`, fully bypassing Aegis. Hook `is_already_wrapped` skips any
  command starting with `aegis`, so an agent can prefix commands to bypass the
  wrapper.
- **Files:** `shell_wrapper.rs:37-41`, `install/hook.rs:72`.
- **Fix:** gate and audit `aegis off`; narrow `is_already_wrapped` to the exact
  supported `aegis --command ...` form.

### [ ] M4 — Hook panics can fail open

- **Problem:** `run_hook` lacks `catch_unwind`; a panic can produce no deny JSON,
  and consumers may allow the tool call.
- **File:** `install/hook.rs`.
- **Fix:** wrap hook execution in `catch_unwind` and emit deny on panic.

### [ ] M5 — Additional point pattern gaps

- **Problem:** missing or weak coverage for `chmod -R 000 /`, `TRUNCATE users;`
  without `TABLE`, `docker volume rm`, and `npm publish`.
- **Fix:** extend rules and add regression tests.

### [ ] M6 — Project config can disable recovery

- **Problem:** same merge issue as C3 lets project config set
  `snapshot_policy = "None"` and `sandbox.required = false`.
- **Fix:** covered by C3 restrictive merge ratchet.

### [ ] M7 — Latent structural fail-open around shell audit readiness

- **Problem:** `append_shell_audit` returns `Ok(())` on `SetupFailure`, and
  `execute_with_snapshots` executes after a "successful" audit. Today this appears
  unreachable by construction, but the invariant is fragile.
- **File:** `shell_flow.rs:165-235`.
- **Fix:** make execution type-safe on an explicit `Ready` state.

### [ ] M8 — Snapshot does not recover the dangerous command's effect on committed/clean files

- **Problem:** the snapshot promised as the "rollback the dangerous action" safety
  net is a `git stash push --include-untracked`, which captures only *uncommitted*
  changes + untracked files present at snapshot time. A `Danger` command that
  deletes **committed/clean** files (e.g. `rm -rf scripts` on tracked files) runs
  after the stash, so its deletion is never in the snapshot — `aegis rollback`
  restores the stashed delta but does **not** undo the deletion. Outside a git
  repo, `GitPlugin` is not applicable so no snapshot is taken at all. Net effect:
  the headline "delete → rollback" promise does not hold for the most common case.
- **Status:** agent-confirmed via live test — approved `rm -rf scripts` in a git
  repo; the resulting stash contained only the untracked `scratch.txt`, not the
  deleted (committed) `scripts/`; recovery required `git restore`, not
  `aegis rollback`.
- **Files:** `crates/aegis-snapshot/src/git.rs:67-99` (`git stash push` only
  captures the working-tree delta), `is_applicable` (git repo only),
  `src/shell_flow.rs:343` (snapshot taken on `Danger` approve).
- **Fix:** either back the snapshot with a real copy of the targeted paths so
  deletions of committed/clean and out-of-repo data are recoverable, or narrow the
  feature's promise in docs/UX to "preserves uncommitted work at snapshot time"
  and surface clearly when no snapshot is possible (non-git cwd).

### [ ] M9 — `aegis rollback` is unusable from `aegis snapshot list` output

- **Problem:** the git `snapshot_id` is the composite `"<cwd>\t<hash>"`
  (`SEP = '\t'`). `aegis snapshot list` renders it as `<path>␉<hash>`, which looks
  like two columns; `aegis rollback <SNAPSHOT_ID>` takes a single argument, so the
  bare hash is "not found in the audit log" and `path<space>hash` is rejected as
  two arguments. There is no practical way for a user to copy the tab-bearing id
  from the listing into a working `rollback` invocation — the recovery CLI is
  effectively non-functional.
- **Status:** agent-confirmed via live test.
- **Files:** `crates/aegis-snapshot/src/git.rs:20` (`SEP = '\t'`), `:97`
  (`snapshot_id = format!("{}{SEP}{hash}", cwd.display())`), `:108`/`:179`
  (`split_once(SEP)` on rollback), `src/rollback.rs` (single-arg `<SNAPSHOT_ID>`).
- **Fix:** make the id round-trip-safe — store the repo path as a separate audit
  field and use the bare hash as the id (or have `rollback` resolve a bare hash
  against the audit log), and/or have `snapshot list` print a ready-to-paste
  `aegis rollback '<exact-id>'` line.

---

## P3 — Low / informational

- [ ] SQLite snapshot TOCTOU: `exists()` + `copy` instead of `create_new`.
- [ ] Backslash-newline tokenization edge cases.
- [ ] C-next — IFS parameter-expansion modifiers: the C2 fix normalizes only the
      literal `$IFS` / `${IFS}` default spellings. Parametric forms
      (`${IFS:-x}`, `${IFS:+x}`) and runtime `IFS=` reassignment are not
      normalized and remain opaque. Decide whether broader shell-state analysis
      is warranted.
- [ ] `stdout_renderer` final `_ => Approve` arm is future fail-open for new risk
      variants; currently unreachable.
- [ ] Sandbox status TOCTOU.
- [ ] `current_dir()` failure can snapshot against `.`.
- [ ] `cargo audit` reports 4 unmaintained advisories
      (`atomic-polyfill`, `derivative`, `fxhash`, `paste`) only under opt-in
      `--features starlark-policy`; not default build and no CVE.

---

## Confirmed strengths

- Intrinsic `Block` is unbreakable: checked first in `engine.rs:22`, before
  allowlist/rules/mode. Covered by `block_risk_is_never_bypassable` tests.
- `CI=1` is not a bypass; default `ci_policy = Block` makes behavior stricter.
- Policy precedence is correct: blocklist > allowlist; allowlist override is gated
  by risk; allowlist glob `*` maps to `[^;&|]+` and does not cross `;`, `&&`, or
  `|`.
- Starlark is constrained: `max_tick_count = 100k`, heap 10MiB, callstack 500;
  opt-in; path only global `~/.aegis/policy.star`; fail-closed.
- `unsafe` is limited to documented libc syscalls for Landlock plus
  edition-mandated `env::set_var` in tests; no transmute/FFI problems found.
- No command-input panics found; parser uses `Vec<char>` and guarded logic;
  `$SHELL` proxy is fail-closed on panic.
- Installer has strict path validation, JSON/TOML serializers, atomic writes; amend
  escapes TOML.
- `cargo deny check` is green.
- Audit-log newline injection is not exploitable because `serde` escapes it.

---

## Fix plan by priority

### Sprint 1 — required before release: core bypass closure

1. [ ] C1 — `RegexBuilder::case_insensitive(true)` for built-in patterns plus
       uppercase regression tests.
2. [x] C2 — normalize `$IFS` / `${IFS}` as separators in tokenizer or
       normalization plus fixtures.
3. [ ] C3 / M6 — restrictive merge ratchet for security fields:
       `mode`, `*_override_level`, `ci_policy`, `snapshot_policy`,
       `sandbox.required`; warn on weakening in `config validate`.
4. [ ] C4 — normalize the prefix-rule lookup key: basename of `tokens[0]` + strip
       launcher/wrapper prefixes (`rtk`, `sudo`, `env`, `command`, …); cover
       RTK-wrapped and absolute-path forms for every token-prefix family.
5. [ ] H1 — segment on standalone `&` (couples with C4 so `cd … && git …` is seen).
6. [ ] H2 — recurse into `psql -c` / `mysql -e` or relax destructive SQL prefix
       anchors.
7. [ ] H8 — add git prefix rules for `push --force`, `stash clear`, `branch -D`;
       revisit the edge-case test that whitelists force-push.
8. [ ] H4 — make `claude-code.sh` deny on missing `jq` / `aegis` / invalid JSON.

### Sprint 2 — required before release: defense in depth

7. [ ] H3 / M5 — expand pattern database for `wipefs`, S3 delete flows, `gsutil`,
       `~/.ssh`, shell rc truncation, `unlink`, `chmod 000`, `TRUNCATE`,
       `docker volume rm`, and `npm publish`; run eval harness.
8. [ ] H6 / H7 — add snapshot path containment checks; create snapshot directories
       as `0700`, dumps/logs as `0600`; avoid following audit-log symlinks.
9. [ ] M1 — default `sandbox.required = true` or unconditional stderr warning on
       degradation.
10. [ ] M2 — add regex size limits.
11. [ ] M4 — add `catch_unwind` in `run_hook` and emit deny.

### Sprint 3 — honesty and resilience

12. [ ] H5 — add HMAC/external anchor for audit chain, or change public wording
        from "tamper-evident" to "integrity/corruption check".
13. [ ] M3 — gate/audit `aegis off`; narrow `is_already_wrapped`.
14. [ ] M7 + P3 — type-safe audit readiness, fail-closed renderer fallback,
        SQLite `create_new`, and sandbox confidentiality documentation.

---

## Cross-cutting conclusion

Aegis' foundation is correctly designed: unbreakable `Block`, precedence rules,
and fail-closed behavior around errors, panics, and toggle I/O are strong. The
current release blocker is scanner input normalization and coverage: case,
`$IFS`, `&`, nested SQL, and — newly found in the 0.5.9 crash-test — the
token-prefix anchoring bypass (C4), where any path/launcher/wrapper prefix
(`/usr/bin/git`, `rtk git`, `sudo git`, `cd … && git`) defeats the migrated
git/db/cloud/docker/process rules. These false-negatives are unacceptable for a
security tool. The open 1.0 gate for "zero false-negatives" is factually not met.
Sprint 1 must block release.
