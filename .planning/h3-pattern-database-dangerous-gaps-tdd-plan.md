# H3 TDD Plan — Pattern database has dangerous gaps

## Task

Implement `TASKS.md` finding **H3 — Pattern database has dangerous gaps**.

Seven commands currently classify as `Safe` and execute under `Protect` with no
prompt. Add seven built-in rules so each reaches its intended `RiskLevel`.

| Command (today → `Safe`) | New ID | Risk |
|--------------------------|--------|------|
| `wipefs -a /dev/sda` | FS-011 | `Danger` |
| `unlink <file>` | FS-012 | `Warn` |
| `>> ~/.ssh/authorized_keys` (and `>`) | FS-013 | `Danger` |
| `> ~/.bashrc` (shell-rc clobber) | FS-014 | `Warn` |
| `aws s3 rb --force` | CL-011 | `Danger` |
| `aws s3 sync --delete` | CL-012 | `Warn` |
| `gsutil rm -r` | CL-013 | `Danger` |

Threat confirmed real on `assess()` (traced through `quick_scan` →
`prefix_scan`/`full_scan`): `wipefs`/`gsutil`/`unlink` have no keyword and no prefix
program → `quick_scan` returns false → `Safe`. `aws s3 rb`/`sync` pass `quick_scan`
(`aws` is an indexed prefix program) but match no rule — the only `aws s3` rule is
`CL-005` (`aws s3 rm … --recursive`). `authorized_keys`/`bashrc` are in no pattern;
`FS-009` only covers `> /dev/sd[a-z]`.

## Decision (agreed during grilling)

**Mechanism split follows ADR-014 / ADR-015**: the deciding test is *does the
dangerous signature arrive as the `Effective program` token (`tokens[0]`), or
embedded mid-command?*

- **Token-prefix rules** (`builtins_a.rs`), keyed on the effective program:
  `wipefs`, `unlink`, `aws s3 rb`, `aws s3 sync`, `gsutil rm`. The verb *is*
  `tokens[0]` (ADR-014 already strips `sudo`/`rtk`/abs-paths); the three cloud rules
  extend the existing `CL-*` prefix family. `wipefs`/`unlink` are the **first
  Filesystem-category token-prefix rules** — accepted because their dangerous form
  has no match-anywhere delivery variety (unlike `rm`, which is regex `FS-001`).
- **Match-anywhere regex** (`patterns.toml`): `authorized_keys` write and shell-rc
  clobber. The danger is a *redirect operator + target path* that can appear
  anywhere (`echo k >> ~/.ssh/authorized_keys`), never as `tokens[0]` — the FS-009 /
  ADR-015 shape.

**No new ADR.** This is a direct application of ADR-014/015. The only mildly
surprising call — `wipefs` = `Danger` while `mkfs` (`FS-006`) = `Block` — is an
easily reversible risk-level tuning, documented with an inline code comment, not an
ADR. Rationale: `wipefs` erases filesystem/partition *signatures* only (data blocks
survive, often recoverable), so it is strictly less final than `mkfs`, and it has
legitimate interactive disk-prep use where a prompt + snapshot is the right
checkpoint.

## Risk levels (agreed)

- `Danger`: FS-011 (`wipefs`), FS-013 (`authorized_keys` — access-control file
  write / lockout), CL-011 (`aws s3 rb --force` — parity `CL-005`), CL-013
  (`gsutil rm -r` — recursive object delete, GCS twin of `CL-005`).
- `Warn`: FS-012 (`unlink` — single file/link, less than `rm -rf`), FS-014 (rc
  clobber — recoverable config file), CL-012 (`aws s3 sync --delete` — common
  deploy op, reversible with bucket versioning).

## Scope

- Add exactly **seven** rules with the IDs above. Next free numbers: `FS-010` and
  `CL-010` are the current maxima.
- All five **prefix rules** go in `crates/aegis-scanner/src/patterns/builtins_a.rs`
  (`wipefs`/`unlink` under a new `── Filesystem ──` block; `aws`/`gsutil` in the
  existing `── Cloud ──` block).
- Both **regex patterns** (FS-013, FS-014) go in
  `crates/aegis-scanner/patterns.toml` under `── Filesystem ──`.
- **Out of scope** (recorded in `TASKS.md` → `H3-followups`, do not implement):
  `wipefs -af`/`-fa` bundled flags; `aws --profile … s3 …` pre-service global
  flags; `tee`/`tee -a` to `authorized_keys`; `gcloud storage rm -r`;
  `rsync --delete`; `blkdiscard`/`sgdisk`/`parted`; `redis-cli FLUSHALL`.

## Token shapes (prefix rules — verified against `aegis_parser::matches_prefix`)

`matches_prefix` supports leading/multiple `AnyStar` (backtracking); `Alts` match
flags **exactly and case-sensitively**; non-flag tokens are case-insensitive.

```text
FS-011  wipefs   [ s("wipefs"), any_star(), a(&["-a","--all"]) ]
FS-012  unlink   [ s("unlink") ]
CL-011  aws rb   [ s("aws"), s("s3"), s("rb"),   any_star(), s("--force") ]
CL-012  aws sync [ s("aws"), s("s3"), s("sync"), any_star(), s("--delete") ]
CL-013  gsutil   [ s("gsutil"), any_star(), s("rm"), any_star(), a(&["-r","-R","--recursive"]) ]
```

- `gsutil` leading `any_star()` catches the idiomatic `gsutil -m rm -r gs://b`.
- `aws` rules mirror `CL-005` (no leading `any_star()`); the pre-service global-flag
  gap is a recorded follow-up.
- `wipefs` `AnyStar` lets `-a` appear after a dry-run flag, so `wipefs -n -a /dev/sda`
  still prompts — an **accepted fail-safe FP**. Bundled `-af`/`-fa` are an **accepted
  known FN** (follow-up).

## Regex shapes (verified against `keywords::extract_keywords` / `derive_program_keys`)

```toml
# FS-013 — authorized_keys (append backdoor + truncate lockout)
pattern = '>+\s*\S*authorized_keys'
```

- Leading `>+` (not a 2-char literal) is deliberate: a literal `>>` would become the
  Aho-Corasick keyword and fire the gate on every append redirect. With `>+`,
  `extract_keywords` falls through to the embedded literal **`authorized_keys`** — a
  clean, specific gate. Confirmed: `leading_literal(">+…") == ">"` (len 1) →
  `find_embedded_literal` → `"authorized_keys"`.
- Matches both `>` and `>>`. No FP on read/backup
  (`cat ~/.ssh/authorized_keys > /tmp/bak` — filename precedes the `>`).
- `derive_program_keys` returns empty (not `^`-anchored) → lands in `universal`
  (match-anywhere). Correct.

```toml
# FS-014 — shell-rc clobber (single `>` truncate only; `>>` append must NOT fire)
pattern = '(^|[^>])>[^>]\s*\S*\.bashrc|(^|[^>])>[^>]\s*\S*\.zshrc|(^|[^>])>[^>]\s*\S*\.bash_profile|(^|[^>])>[^>]\s*\S*\.zprofile|(^|[^>])>[^>]\s*\S*\.profile|(^|[^>])>[^>]\s*\S*\.zshenv|(^|[^>])>[^>]\s*\S*\.bash_login'
```

- The Rust `regex` crate has **no lookbehind**, so a single `>` is isolated with the
  `(^|[^>])>[^>]` guard. Verified: both `>` positions of `>> ~/.bashrc` are rejected;
  `> ~/.bashrc` and `echo x > ~/.bashrc` match.
- `extract_keywords` extracts only **one** embedded literal per *top-level*
  alternative. A single inline `(bashrc|zshrc|…)` group would gate on `bashrc` only →
  `> ~/.zshrc` would stay `Safe` (FN). Therefore the alternation is **top-level, per
  filename**, so each branch yields its own keyword
  (`bashrc`,`zshrc`,`bash_profile`,`zprofile`,`profile`,`zshenv`,`bash_login`).
  (Iteration 0 must confirm this extraction empirically.)
- Filename set: `.bashrc .zshrc .bash_profile .zprofile .profile .zshenv .bash_login`.

## Constraints

- All shell commands run through `rtk`.
- Do **not** add dependencies; do not touch `Cargo.toml` / `Cargo.lock` / `deny.toml`.
- **Fail-closed preserved:** every change only *adds* matches → only more prompts,
  never fewer. `Intrinsic Block` untouched.
- Hot path stays synchronous (ADR-002); no `has_uncovered` regression — every new
  regex has a clean extractable keyword, so the safe path stays on the keyword gate.
- New prefix rules carry `match_examples` / `not_match_examples`; debug builds panic
  on a rule that fails its own examples (`validate_examples`).

## Definition of Done

1. Seven rules exist with the IDs/risks/mechanisms above; IDs unique across regex +
   prefix namespaces.
2. `wipefs -a /dev/sda`, `aws s3 rb s3://b --force`, `gsutil -m rm -r gs://b`,
   `gsutil rm -R gs://b`, `>> ~/.ssh/authorized_keys`, `> ~/.ssh/authorized_keys`
   reach `Danger`; `unlink x`, `aws s3 sync ./d s3://b --delete`, `> ~/.bashrc`,
   `> ~/.zshrc` reach `Warn`.
3. Narrowness green: `wipefs /dev/sda` (no `-a`), `aws s3 rb s3://b` (no `--force`),
   `aws s3 sync ./d s3://b` (no `--delete`), `gsutil rm gs://b/file` (no `-r`),
   `>> ~/.bashrc` (append), `cat ~/.ssh/authorized_keys > /tmp/bak`, `readlink x`,
   `ln -s a b` do **not** raise their rule.
4. New tests fail on old code, pass on new code.
5. Local gates pass: `rtk cargo fmt --check`,
   `rtk cargo clippy --all-targets -- -D warnings`, `rtk cargo test --workspace`.

## Iteration 0 — Baseline inspection

```bash
rtk sed -n '1,60p'   crates/aegis-scanner/src/patterns/builtins_a.rs   # Git/DB/Cloud prefix blocks
rtk sed -n '1,120p'  crates/aegis-scanner/patterns.toml                # FS regex block (FS-001..010)
rtk grep -n 'FS-010\|CL-010\|CL-005' crates/aegis-scanner/src crates/aegis-scanner/patterns.toml
rtk cargo test -p aegis-scanner
```

Empirically confirm keyword extraction for the two regexes (guards against an FN
gate) — add a throwaway unit assertion or inspect via an existing keywords test:

```text
extract_keywords(r">+\s*\S*authorized_keys")            == ["authorized_keys"]
extract_keywords(FS-014 pattern) ⊇ {bashrc, zshrc, bash_profile, zprofile,
                                    profile, zshenv, bash_login}
```

If FS-014 extraction does **not** yield every filename, fall back to one
`[[patterns]]` entry per filename (each trivially gated). Locate the table-driven
`Filesystem` / `Cloud` blocks in `basic.rs` and the narrowness rows in
`edge_cases.rs`.

## Iteration 1 — RED: regression tests

### `crates/aegis-scanner/src/scanner/tests/basic.rs`

Positive rows (→ expected risk):

1. `wipefs -a /dev/sda` → `Danger`
2. `wipefs --all /dev/sdb` → `Danger`
3. `sudo wipefs -a /dev/nvme0n1` → `Danger` (launcher strip, ADR-014)
4. `unlink important.txt` → `Warn`
5. `aws s3 rb s3://my-bucket --force` → `Danger`
6. `aws s3 sync ./dist s3://my-bucket --delete` → `Warn`
7. `gsutil rm -r gs://my-bucket/data` → `Danger`
8. `gsutil -m rm -r gs://my-bucket/data` → `Danger`
9. `gsutil rm -R gs://my-bucket/data` → `Danger`
10. `echo "ssh-ed25519 AAAA" >> ~/.ssh/authorized_keys` → `Danger`
11. `> ~/.ssh/authorized_keys` → `Danger` (truncate / lockout)
12. `> ~/.bashrc` → `Warn`
13. `echo unset PATH > ~/.zshrc` → `Warn`

### `crates/aegis-scanner/src/scanner/tests/edge_cases.rs` (narrowness guards)

14. `wipefs /dev/sda` → not `Danger` (no `-a`)
15. `wipefs -n -a /dev/sda` → `Danger` (accepted fail-safe FP — assert it *does* fire)
16. `aws s3 rb s3://my-bucket` → not `Danger` (no `--force`)
17. `aws s3 sync ./dist s3://my-bucket` → `Safe` (no `--delete`)
18. `gsutil rm gs://my-bucket/file.txt` → not `Danger` (single object, no `-r`)
19. `echo export PATH=$PATH:/x >> ~/.bashrc` → not `Warn` from FS-014 (append)
20. `cat ~/.ssh/authorized_keys > /tmp/backup` → not `Danger` (filename precedes `>`)
21. `readlink mylink` → `Safe`; `ln -s a b` → `Safe` (not FS-012)

### RED command

```bash
rtk cargo test -p aegis-scanner
```

Cases 1–13 must be RED on current code (`Safe` instead of `Danger`/`Warn`).

## Iteration 2 — GREEN: add the rules

### `crates/aegis-scanner/src/patterns/builtins_a.rs`

Add five `PrefixRule`s with the token shapes above, each with `description`,
`safe_alt`, `justification`, `match_examples`, `not_match_examples`. New
`── Filesystem ──` block for FS-011/FS-012; FS-011 carries the inline comment
explaining `Danger` vs `mkfs`=`Block`. CL-011/012/013 in the `── Cloud ──` block.

### `crates/aegis-scanner/patterns.toml`

Add FS-013 and FS-014 in the `── Filesystem ──` section with the verified regexes.

### GREEN command

```bash
rtk cargo test -p aegis-scanner
```

All of 1–21 green; debug-build `validate_examples` passes (no rule fails its own
examples).

## Iteration 3 — Docs / glossary / TASKS

- `CONTEXT.md` — already updated during grilling (`Token-prefix rule` now lists
  Filesystem `wipefs`/`unlink`; embedded signatures stay regex, ADR-014/015). Verify
  present.
- `TASKS.md` — H3 scope + `H3-followups` already recorded during grilling. Mark H3
  `[ ]` → `[x]` with a one-line resolution **only after** tests pass.
- No ADR. No `README` / `config-schema` change (built-in patterns are not part of the
  documented config surface).

## Iteration 4 — Final verification

```bash
rtk cargo fmt --check
rtk cargo clippy --all-targets -- -D warnings
rtk cargo test --workspace
rtk cargo bench --bench scanner_bench 2>/dev/null || true   # safe path stays < 2ms
```

Hot-path check: five prefix rules add tiny `prefix_by_program` entries; two regexes
add `universal` patterns with clean keywords (`authorized_keys`, rc filenames) — the
quick-scan gate keeps its keyword set, no `has_uncovered`. Confirm no `scanner_bench`
regression.

Security/dependency gates (if environment allows; diff changes neither deps nor graph):

```bash
rtk cargo audit
rtk cargo deny check
```

## Review checklist

- [ ] Seven rules added: FS-011/012/013/014, CL-011/012/013; IDs unique.
- [ ] Mechanisms correct: `wipefs`/`unlink`/`aws`/`gsutil` prefix; `authorized_keys`/
      rc-clobber regex.
- [ ] Risk levels: FS-011/013/CL-011/013 = `Danger`; FS-012/014/CL-012 = `Warn`.
- [ ] FS-013 regex starts `>+` (keyword anchors on `authorized_keys`, not `>>`).
- [ ] FS-014 is single-`>` only (append `>>` does NOT fire) and uses top-level
      alternation per filename (every rc file gates).
- [ ] `gsutil -m rm -r` caught; `aws` rules mirror `CL-005`.
- [ ] Narrowness guards green (no `-a` / no `--force` / no `--delete` / no `-r` /
      append / read-redirect all clear).
- [ ] New tests fail on old code, pass on new code; `validate_examples` green.
- [ ] Fail-closed preserved; `Intrinsic Block` untouched; no `has_uncovered`.
- [ ] `CONTEXT.md` verified; `TASKS.md` H3 marked only after green; `H3-followups`
      present.

## Suggested commit

```text
fix(scanner): close H3 pattern gaps — wipefs/unlink/authorized_keys/rc + s3 rb/sync/gsutil
```

Files:

- `crates/aegis-scanner/src/patterns/builtins_a.rs`
- `crates/aegis-scanner/patterns.toml`
- `crates/aegis-scanner/src/scanner/tests/basic.rs`
- `crates/aegis-scanner/src/scanner/tests/edge_cases.rs`
- `CONTEXT.md` (already edited during grilling)
- `TASKS.md` (scope + follow-ups already added; mark H3 complete)
- this plan file
