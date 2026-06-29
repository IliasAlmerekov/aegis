# ADR-015: Destructive SQL is detected by match-anywhere regex, not token-prefix rules

## Status

Accepted

## Context

The C4 / ADR-014 migration moved several detection families from regex
`Pattern`s to `Token-prefix rule`s keyed on a scan target's `Effective
program`. The Database family was migrated along with the rest: `DB-001`
(`DROP TABLE`), `DB-002` (`DROP DATABASE`), `DB-007` (`DROP SCHEMA`), and
`DB-008` (`ALTER TABLE … DROP COLUMN`) became token-prefix rules matched against
the token sequence with the SQL verb expected at the leading program position.

That assumption does not hold for SQL. A SQL verb is almost never the program
token — it is *delivered* through a database client:

```
psql -c 'DROP TABLE users'     → tokens ["psql", "-c", "DROP TABLE users"]
mysql -e 'DROP DATABASE app'   → tokens ["mysql", "-e", "DROP DATABASE app"]
```

The `Effective program` is `psql` / `mysql`, so `prefix_lookup` finds no rule
and the SQL verb — mid-string, never `tokens[0]` — is never matched. The
result is a real, no-preparation false-negative on `assess()`:
`psql -c 'DROP TABLE users'`, `rtk psql -c 'DROP TABLE users'`, and
`mysql -e 'DROP DATABASE app'` all returned `Safe` and executed under `Protect`
without a prompt (TASKS.md H2).

The asymmetry was internal and confirmed: `DB-003`/`DB-004`/`DB-005` were left
as regex `Pattern`s, which match **anywhere** in the `Normalized command`, so
`psql -c 'TRUNCATE TABLE x'` (`DB-004`) was already caught while
`psql -c 'DROP TABLE x'` (`DB-001`) was not. The verbs reach the scanner via
`-c` / `-e` / `--command` / `--execute`, heredocs, stdin pipes, and
`;`-compound statements — none of which surface the verb as a program token.

## Decision

Revert the four destructive-SQL rules (`DB-001`, `DB-002`, `DB-007`, `DB-008`)
to **match-anywhere regex `Pattern`s** in `patterns.toml`, restoring parity with
`DB-003`/`DB-004`/`DB-005`. SQL verbs are not program tokens, so regex is the
correct mechanism: it is delivery-agnostic and catches every embedding (`-c`,
`-e`, `--command`, `--execute`, heredoc, stdin pipe, `;`-compound) without
enumerating interpreters or flags.

- IDs and risk levels are unchanged: `DB-001`/`002`/`007` = `Danger`,
  `DB-008` = `Warn`.
- The rules are `\b`-anchored with a mandatory `\s+` between verb and object to
  stay narrow:
  - `DB-001`: `\bdrop\s+table\b`
  - `DB-002`: `\bdrop\s+database\b`
  - `DB-007`: `\bdrop\s+schema\b`
  - `DB-008`: `\balter\s+table\s+.+?\s+drop\s+column\b` (the table reference is
    a non-greedy `.+?` bounded by the mandatory `\s+drop\s+column\b`, so it
    tolerates a space-containing or quoted identifier — `ALTER TABLE "my table"
    DROP COLUMN x` — while staying anchored to the `drop column` clause; it does
    not float free like a bare `.*`).
- Case-insensitivity is automatic: built-in regexes compile with
  `case_insensitive(true)` (the C1 fix).
- The four IDs are **moved**, not duplicated — removed from
  `builtin_prefix_rules()` and added to `patterns.toml` — so duplicate-ID
  validation still holds.

The rejected alternative (treat `psql`/`mysql` as interpreters and extract the
`-c`/`-e` body as an `Inline script`) requires an interpreter/flag allowlist —
a coverage trap that misses any unlisted client or flag — and still needs
`;`-segmentation for compound SQL. Regex avoids both.

ADR-014 still holds for the command families whose verb genuinely *is* the
program token (Git, Cloud, Docker, and the Process rules, plus the Redis
`DB-006` `FLUSHALL`/`FLUSHDB`, whose verb is the command). This ADR carves out
only SQL verbs, which are arguments, not programs.

## Consequences

`psql -c` / `mysql -e` / `--command=` / `--execute` / `rtk psql` /
`;`-compound forms of `DROP TABLE` / `DROP DATABASE` / `DROP SCHEMA` and
`ALTER TABLE … DROP COLUMN` now reach their intended risk level. Bare SQL
(`DROP TABLE users;`) is unchanged.

The change is fail-closed: a match-anywhere regex is a superset of the old
first-token prefix match on bare SQL, so it only adds matches, never removes
them. `Intrinsic Block` is untouched.

`\b`-anchoring accepts match-anywhere false positives as a fail-safe — e.g.
`git commit -m "drop table feature"` raises `Danger`. This is parity with the
long-standing `DB-004` (`truncate table`) behavior: an extra prompt is cheaper
than a missed `DROP`. Per ADR-010, we do **not** add a shell/SQL parser to
distinguish "real SQL" from a quoted string.

Hot path is unaffected: `extract_keywords` yields `drop` / `alter`, so the
Aho-Corasick `Quick scan` gate still fires; the `\b…`-anchored (non-`^`)
patterns land in the `universal` set and run on the post-`quick_scan` target
loop only, preserving the < 2 ms `Safe`-path budget (ADR-002).

Out of scope (tracked separately):

- `DB-006` prefix gap for `redis-cli FLUSHALL`.
- `TRUNCATE` without `TABLE` (TASKS.md M5).
- **Non-whitespace separators between verb and object.** The rules require
  `\s+` between `drop`/`alter` and the following keyword, so a SQL comment used
  as a separator evades them: `DROP/**/TABLE users` is treated by PostgreSQL as
  whitespace and executes, but `\s` does not match `/`. Closing this would need
  a SQL-aware normalizer, which ADR-010 rules out (no full shell/SQL parser).
- **Uncovered destructive SQL verbs.** `DROP VIEW`, `DROP INDEX`, and the
  `dropdb <name>` shell client (a verb-is-program form, so a `DB-006`-style
  prefix-rule candidate rather than a regex one) are not detected. These are
  pre-existing coverage gaps, not regressions of this change.

These SQL coverage gaps are filed as a backlog item in `TASKS.md`.
