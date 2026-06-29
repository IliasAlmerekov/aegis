# H2 TDD Plan — SQL inside `psql -c` / `mysql -e` is not scanned

## Task

Implement `TASKS.md` finding **H2 — SQL inside `psql -c` / `mysql -e` is not scanned**.

`psql -c 'DROP TABLE users'` → `Safe`, while bare `DROP TABLE users` → `Danger`.

Root cause (confirmed in code): the `Database` `Category` is split across two
mechanisms. `DB-003`/`DB-004`/`DB-005` are regex `Pattern`s (`patterns.toml`) that
match **anywhere** in the `Normalized command`, so they already catch embedded SQL.
`DB-001`/`DB-002`/`DB-007`/`DB-008` were migrated (with C4 / ADR-014) into
`Token-prefix rule`s in `crates/aegis-scanner/src/patterns/builtins_a.rs`, keyed on
the `Effective program` (`tokens[0]`). SQL passed via `psql -c '…'` tokenizes to
`["psql", "-c", "DROP TABLE users"]`; the effective program is `psql`, so
`prefix_lookup` finds no rule and the SQL verb (mid-string, never `tokens[0]`) is
never matched. This is a regression of the same migration as C4.

Concrete asymmetry, confirmed: `psql -c 'TRUNCATE TABLE x'` is already caught
(`DB-004` regex), but `psql -c 'DROP TABLE x'` is not (`DB-001` prefix rule).

## Decision (agreed during grilling — see ADR-015)

Approach **(B)**: revert the destructive-SQL `Token-prefix rule`s to **match-anywhere
regex `Pattern`s**, restoring parity with `DB-003`/`DB-004`/`DB-005`. SQL verbs are
not program tokens; regex is delivery-agnostic (catches `-c`, `-e`, `--command`,
`--execute`, heredoc, stdin pipe, and `;`-compound statements) without enumerating
interpreters/flags. Rejected approach (A) (treat `psql`/`mysql` as interpreters and
extract the `-c`/`-e` body as an `Inline script`): requires an interpreter/flag
allowlist (a coverage trap) and still needs `;`-segmentation for compound SQL.

## Threat model (agreed)

- **Real, not theoretical**, on `assess()`: `psql -c 'DROP TABLE users'`,
  `rtk psql -c 'DROP TABLE users'`, `mysql -e 'DROP DATABASE app'` return `Safe`
  today and execute under `Protect` without a prompt.
- `Intrinsic Block` is unaffected. The change only adds matches → fail-closed.

## Scope

- Convert exactly **four** rules: `DB-001`, `DB-002`, `DB-007`, `DB-008`.
- **Keep IDs and risk levels unchanged**: `DB-001`/`002`/`007` = `Danger`,
  `DB-008` = `Warn`.
- `DB-008` stays narrow: `\balter\s+table\s+\S+\s+drop\s+column\b` (table name = one
  `\S+`, not `.*`).
- **Out of scope** (do not touch, note as follow-up):
  - `DB-006` (`FLUSHALL`/`FLUSHDB`) — same prefix gap for `redis-cli FLUSHALL`, separate ticket.
  - `TRUNCATE` without `TABLE` — that is finding M5.

## False-positive policy (agreed)

`\b`-anchoring only. Accept match-anywhere false positives (e.g.
`git commit -m "drop table feature"` → `Danger`) as fail-safe — an extra prompt is
cheaper than a missed `DROP`. Parity with `DB-004` (`truncate table`). Do **not** add
heuristics to distinguish "real SQL" from a string (ADR-010: no full shell/SQL parser).

Regex forms:

- `DB-001`: `\bdrop\s+table\b`
- `DB-002`: `\bdrop\s+database\b`
- `DB-007`: `\bdrop\s+schema\b`
- `DB-008`: `\balter\s+table\s+\S+\s+drop\s+column\b`

Case-insensitivity is automatic (built-in regexes compile with
`case_insensitive(true)` — the C1 fix).

## Mechanics (verified from code — no extra changes needed)

- `extract_keywords("\\bdrop\\s+table\\b")` → `["drop"]`, `\balter…` → `["alter"]`,
  so the Aho-Corasick `Quick scan` gate still fires on `drop`/`alter`. No
  false-negative, hot path stays on the keyword gate.
- `derive_program_keys` returns empty for `\b…` patterns (not `^`-anchored), so they
  land in the `universal` set → matched for every command (match-anywhere). Correct.
- Duplicate-ID validation is preserved: we **move** the four IDs from prefix to regex
  (remove from `builtins_a.rs`, add to `patterns.toml`), never duplicate.

## Constraints

- All shell commands run through `rtk`.
- Do **not** add dependencies; do not touch `Cargo.toml` / `Cargo.lock` / `deny.toml`.
- Preserve the **fail-closed invariant**: regex match-anywhere ⊇ the old prefix match
  on bare SQL → only more matches, never fewer. `Intrinsic Block` untouched.
- Hot path stays synchronous (ADR-002); no new allocations on the safe path.

## Definition of Done

1. `DB-001`/`002`/`007`/`008` are regex `Pattern`s in `patterns.toml`; their
   `PrefixRule`s are removed from `builtins_a.rs`.
2. `psql -c 'DROP TABLE users'`, `mysql -e 'DROP DATABASE app'`,
   `rtk psql -c 'DROP TABLE users'`, and the `;`-compound form all reach `Danger`.
3. Bare `DROP TABLE users;` remains `Danger` (anti-regression).
4. Narrowness guards green: `drop_table_log` and `DROP INDEX` do not raise `Danger`.
5. New tests fail on the old code and pass on the new code.
6. Local gates pass: `rtk cargo fmt --check`, `rtk cargo clippy --all-targets -- -D warnings`,
   `rtk cargo test --workspace`.

## Iteration 0 — Baseline inspection

```bash
rtk sed -n '195,300p' crates/aegis-scanner/src/patterns/builtins_a.rs   # DB prefix rules
rtk sed -n '110,150p' crates/aegis-scanner/patterns.toml                # DB-003/004/005 regex
rtk grep -rn 'DB-001\|DB-002\|DB-007\|DB-008' crates/aegis-scanner/src  # find tests asserting these IDs
rtk cargo test -p aegis-scanner
```

Checks:

- Confirm the four `PrefixRule`s and their `match_examples`.
- Locate the table-driven `Database` block in `basic.rs` (around the existing
  `DROP TABLE users;` row).
- Find any test that calls `prefix_scan(...)` expecting these IDs (analogous to
  `cloud_prefix_rules_fire_on_tokenized_inline_script_bodies`) — it must be rewritten
  to use `assess`/`full_scan` or removed in Iteration 2.

## Iteration 1 — RED: regression tests

### File: `crates/aegis-scanner/src/scanner/tests/basic.rs` (Database table-driven block)

Positive (→ expected risk):

1. `psql -c 'DROP TABLE users'` → `Danger`
2. `mysql -e 'DROP DATABASE app'` → `Danger`
3. `psql --command='DROP SCHEMA public CASCADE'` → `Danger`
4. `mysql --execute 'DROP TABLE t'` → `Danger`
5. `rtk psql -c 'DROP TABLE users'` → `Danger`
6. `psql -c 'SELECT 1; DROP TABLE users'` → `Danger`
7. `psql -c 'ALTER TABLE users DROP COLUMN email'` → `Warn`
8. (anti-regression, likely already present) bare `DROP TABLE users;` → `Danger`

### File: `crates/aegis-scanner/src/scanner/tests/edge_cases.rs` (narrowness guards)

9. `psql -c 'SELECT * FROM drop_table_log'` → not `Danger` (no `\s+`, has `_`).
10. `psql -c 'DROP INDEX idx'` → not `Danger` (DROP INDEX intentionally uncovered).

Deliberately **no** `not_match` for `git commit -m "drop table x"` — that is an
accepted FP per the FP policy.

### RED command

```bash
rtk cargo test -p aegis-scanner
```

Cases 1–7 must be RED on current code (`Safe` instead of `Danger`/`Warn`).

## Iteration 2 — GREEN: move prefix rules to regex

### `crates/aegis-scanner/patterns.toml` — add 4 patterns in the `Database` section

TOML single-quoted literals (match existing style). Reuse description/safe_alt/
justification text verbatim from the removed prefix rules:

```toml
[[patterns]]
id = "DB-001"
category = "Database"
risk = "Danger"
pattern = '\bdrop\s+table\b'
description = "DROP TABLE — permanently deletes a database table and all its data"
safe_alt = "Back up the table first: 'CREATE TABLE backup AS SELECT * FROM <table>'"
justification = "Destroys the table and all data. In most engines this is immediate and irreversible without a backup."

# DB-002  pattern = '\bdrop\s+database\b'   risk = "Danger"
# DB-007  pattern = '\bdrop\s+schema\b'     risk = "Danger"
# DB-008  pattern = '\balter\s+table\s+\S+\s+drop\s+column\b'   risk = "Warn"
```

### `crates/aegis-scanner/src/patterns/builtins_a.rs`

Remove the `PrefixRule`s for `DB-001`, `DB-002`, `DB-007`, `DB-008`. Keep `DB-006`.

### Fix any broken existing tests

If Iteration 0 found a `prefix_scan` test expecting these IDs, rewrite it to assert via
`assess`/`full_scan`, or remove it. End-to-end `assess` rows for bare SQL stay green
(regex matches).

### GREEN command

```bash
rtk cargo test -p aegis-scanner
```

## Iteration 3 — ADR + docs

- Create `docs/adr/adr-015-destructive-sql-detected-by-regex-not-token-prefix.md`
  (Status: Accepted; sections Status / Context / Decision / Consequences from this
  plan's Decision + Threat model + FP policy). Note ADR-014 still holds for command
  families (git / db-cli / cloud / docker), just not SQL verbs.
- Add the ADR-015 row to `docs/adr/README.md`.
- `CONTEXT.md` already updated during grilling (Token-prefix rule no longer lists
  Database; Pattern entry notes Database is regex, ref ADR-015) — verify it is present.
- Mark `TASKS.md` H2 `[ ]` → `[x]` with a one-line resolution, only after tests pass.

## Iteration 4 — Final verification

```bash
rtk cargo fmt --check
rtk cargo clippy --all-targets -- -D warnings
rtk cargo test --workspace
rtk cargo bench --bench scanner_bench 2>/dev/null || true   # safe path stays < 2ms
```

Hot-path check: the change adds two `universal` regexes and removes four prefix rules;
the keyword gate keeps `drop`/`alter`. Confirm no regression in `scanner_bench`.

Security/dependency gates (if environment allows; diff changes neither deps nor graph):

```bash
rtk cargo audit
rtk cargo deny check
```

## Review checklist

- [ ] `DB-001`/`002`/`007`/`008` are regex `Pattern`s; prefix rules removed.
- [ ] IDs and risk levels unchanged; `DB-008` is narrow (`\S+`, not `.*`).
- [ ] `psql -c` / `mysql -e` / `--command` / `--execute` / `rtk psql` / `;`-compound all reach the expected risk.
- [ ] Bare `DROP TABLE users;` still `Danger`.
- [ ] `drop_table_log` and `DROP INDEX` do not raise `Danger`.
- [ ] New tests fail on old code, pass on new code.
- [ ] `DB-006` and `TRUNCATE`/M5 untouched.
- [ ] Fail-closed preserved; `Intrinsic Block` untouched.
- [ ] ADR-015 added + indexed; `CONTEXT.md` verified; `TASKS.md` marked only after green.

## Suggested commit

```text
fix(scanner): detect destructive SQL via match-anywhere regex (H2)
```

Files:

- `crates/aegis-scanner/patterns.toml`
- `crates/aegis-scanner/src/patterns/builtins_a.rs`
- `crates/aegis-scanner/src/scanner/tests/basic.rs`
- `crates/aegis-scanner/src/scanner/tests/edge_cases.rs`
- `docs/adr/adr-015-destructive-sql-detected-by-regex-not-token-prefix.md`
- `docs/adr/README.md`
- `CONTEXT.md` (already edited during grilling)
- `TASKS.md` (mark H2 complete)
- this plan file
