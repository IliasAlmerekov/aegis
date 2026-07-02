# H3 Followups Scanner Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close `H3-followups` by adding regression-tested built-in scanner detections for the remaining destructive CLI false negatives.

**Architecture:** Keep this as additive scanner hardening: add token-prefix rules for command-led destructive operations and one local `FS-011` short flag bundle predicate for `wipefs`. Do not change the public prefix matcher/API, tokenizer, parser, config model, or policy engine.

**Tech Stack:** Rust 2024, `aegis-scanner` built-in `PrefixRule`s, `aegis-parser::matches_prefix`, existing scanner test helpers.

---

## Scope

Implement exactly these detections:

- `FS-011`: extend existing `wipefs` detection so any short flag bundle containing `a` matches (`-af`, `-fa`, `-av`, `-fav`, and longer bundles), plus existing `-a` / `--all`.
- `CL-014`: `gcloud any* storage rm any* (-r|-R|--recursive)` → `RiskLevel::Danger`.
- `FS-015`: `rsync any* (--delete|--delete-before|--delete-during|--delete-delay|--delete-after|--delete-excluded)` → `RiskLevel::Warn`.
- `FS-016`: `blkdiscard` effective program → `RiskLevel::Block`.
- `FS-017`: `sgdisk any* (--zap-all|-Z)` → `RiskLevel::Danger`.
- `FS-018`: `parted any* (mklabel|rm)` → `RiskLevel::Danger`.
- `DB-006`: add `redis-cli any* (FLUSHALL|FLUSHDB)` → `RiskLevel::Danger`, reusing the existing `DB-006` ID.

Non-goals:

- Do not add a generic short-flag-bundle `PatternToken`.
- Do not change `aegis_parser::matches_prefix` semantics globally.
- Do not add universal regex patterns for these detections.
- Do not add an ADR.

## Files

- Modify: `crates/aegis-scanner/src/scanner/prefix_rule.rs`
  - Add a local `FS-011` special case for `wipefs` short flag bundles.
- Modify: `crates/aegis-scanner/src/patterns/builtins_a.rs`
  - Add new `FS-*`, `CL-014`, and duplicate-ID `DB-006` prefix rules.
  - Update `FS-011` examples and comments.
- Modify: `crates/aegis-scanner/src/scanner/tests/basic.rs`
  - Add positive must-fire regression cases.
- Modify: `crates/aegis-scanner/src/scanner/tests/h3_gaps.rs`
  - Add near-miss/narrowness regression cases.
- Modify: `TASKS.md`
  - Mark `H3-followups` closed and replace active list with a resolution note.
- Modify: `CHANGELOG.md`
  - Add one `Security` entry under `[Unreleased]`.
- Modify: `PROJECT_STATE.md`
  - Record the plan/glossary update now, then implementation results after `/implement`.

## Task 1: Red tests for `wipefs` short flag bundles

**Files:**
- Modify: `crates/aegis-scanner/src/scanner/tests/basic.rs`
- Modify: `crates/aegis-scanner/src/scanner/tests/h3_gaps.rs`

- [ ] **Step 1: Add failing positive tests**

In `crates/aegis-scanner/src/scanner/tests/basic.rs`, extend `assess_h3_wipefs_unlink_prefix_rules`:

```rust
#[test]
fn assess_h3_wipefs_unlink_prefix_rules() {
    let cases: &[(&str, RiskLevel, &str)] = &[
        ("wipefs -a /dev/sda", RiskLevel::Danger, "FS-011"),
        ("wipefs --all /dev/sdb", RiskLevel::Danger, "FS-011"),
        ("wipefs -af /dev/sda", RiskLevel::Danger, "FS-011"),
        ("wipefs -fa /dev/sda", RiskLevel::Danger, "FS-011"),
        ("wipefs -fav /dev/sda", RiskLevel::Danger, "FS-011"),
        ("wipefs -av /dev/sda", RiskLevel::Danger, "FS-011"),
        // launcher strip (ADR-014): sudo is removed before the prefix scan.
        ("sudo wipefs -a /dev/nvme0n1", RiskLevel::Danger, "FS-011"),
        ("sudo wipefs -fa /dev/nvme0n1", RiskLevel::Danger, "FS-011"),
        ("unlink important.txt", RiskLevel::Warn, "FS-012"),
    ];
    for (cmd, risk, id) in cases {
        assert_assessment_matches_pattern(cmd, *risk, id);
    }
}
```

- [ ] **Step 2: Add failing narrowness tests**

In `crates/aegis-scanner/src/scanner/tests/h3_gaps.rs`, add:

```rust
#[test]
fn h3_wipefs_short_flags_without_all_flag_do_not_fire_fs011() {
    let s = scanner();
    for cmd in ["wipefs -n /dev/sda", "wipefs -f /dev/sda", "wipefs -vn /dev/sda"] {
        let assessment = s.assess(cmd);
        assert!(
            !assessment
                .matched
                .iter()
                .any(|m| m.pattern.id.as_ref() == "FS-011"),
            "FS-011 must not fire without -a/--all: {cmd:?} => {:?}",
            assessment
                .matched
                .iter()
                .map(|m| m.pattern.id.as_ref())
                .collect::<Vec<_>>()
        );
        assert!(
            assessment.risk < RiskLevel::Danger,
            "{cmd:?} must not reach Danger without -a/--all (got {:?})",
            assessment.risk
        );
    }
}
```

- [ ] **Step 3: Run focused tests and confirm RED**

Run:

```bash
rtk cargo test -p aegis-scanner assess_h3_wipefs_unlink_prefix_rules
rtk cargo test -p aegis-scanner h3_wipefs
```

Expected: `assess_h3_wipefs_unlink_prefix_rules` fails on `wipefs -af` / `-fa` / `-fav` / `-av`; narrowness test passes or remains green.

## Task 2: Implement local `FS-011` short flag bundle support

**Files:**
- Modify: `crates/aegis-scanner/src/scanner/prefix_rule.rs`
- Modify: `crates/aegis-scanner/src/patterns/builtins_a.rs`

- [ ] **Step 1: Add local helper in `prefix_rule.rs`**

In `impl PrefixRule`, change `matches_tokens` to:

```rust
pub fn matches_tokens(&self, tokens: &[&str]) -> bool {
    if self.id.as_ref() == "FS-011" && wipefs_all_flag_present(tokens) {
        return true;
    }

    aegis_parser::matches_prefix(&self.pattern, tokens)
}
```

Add this private helper below the `impl PrefixRule` block:

```rust
fn wipefs_all_flag_present(tokens: &[&str]) -> bool {
    if !tokens
        .first()
        .is_some_and(|program| program.eq_ignore_ascii_case("wipefs"))
    {
        return false;
    }

    tokens.iter().skip(1).any(|token| {
        if *token == "--all" {
            return true;
        }

        token
            .strip_prefix('-')
            .is_some_and(|short_flags| !short_flags.starts_with('-') && short_flags.contains('a'))
    })
}
```

- [ ] **Step 2: Add unit tests for the helper behavior**

In `crates/aegis-scanner/src/scanner/prefix_rule.rs` test module, add:

```rust
#[test]
fn fs011_matches_wipefs_short_flag_bundle_containing_all_flag() {
    let rule = PrefixRule {
        id: Cow::Borrowed("FS-011"),
        category: Category::Filesystem,
        pattern: vec![single("wipefs"), PatternToken::AnyStar, alts(&["-a", "--all"])],
        risk: RiskLevel::Danger,
        description: Cow::Borrowed("test"),
        safe_alt: None,
        justification: None,
        source: PatternSource::Builtin,
        match_examples: &[],
        not_match_examples: &[],
    };

    assert!(rule.matches_tokens(&["wipefs", "-af", "/dev/sda"]));
    assert!(rule.matches_tokens(&["wipefs", "-fa", "/dev/sda"]));
    assert!(rule.matches_tokens(&["wipefs", "-fav", "/dev/sda"]));
    assert!(rule.matches_tokens(&["wipefs", "-av", "/dev/sda"]));
    assert!(rule.matches_tokens(&["wipefs", "--all", "/dev/sda"]));
    assert!(!rule.matches_tokens(&["wipefs", "-f", "/dev/sda"]));
    assert!(!rule.matches_tokens(&["wipefs", "-vn", "/dev/sda"]));
    assert!(!rule.matches_tokens(&["wipefs", "--almost", "/dev/sda"]));
}
```

- [ ] **Step 3: Update `FS-011` examples/comment**

In `crates/aegis-scanner/src/patterns/builtins_a.rs`, update the `FS-011` comment and examples to reflect the local bundle predicate:

```rust
// AnyStar lets `-a` follow other flags (e.g. `wipefs -n -a`), an
// accepted fail-safe FP. `FS-011` also has a local matcher-side predicate
// for wipefs short flag bundles containing `a` (`-af`, `-fa`, `-fav`);
// this is intentionally not a generic prefix-rule feature.
pattern: vec![s("wipefs"), any_star(), a(&["-a", "--all"])],
```

Set:

```rust
match_examples: &[
    "wipefs -a /dev/sda",
    "wipefs --all /dev/sdb",
    "wipefs -af /dev/sda",
    "wipefs -fa /dev/sda",
],
not_match_examples: &["wipefs /dev/sda", "wipefs -n /dev/sda", "wipefs -f /dev/sda"],
```

- [ ] **Step 4: Run focused tests and confirm GREEN**

Run:

```bash
rtk cargo test -p aegis-scanner fs011_matches_wipefs_short_flag_bundle_containing_all_flag
rtk cargo test -p aegis-scanner assess_h3_wipefs_unlink_prefix_rules
rtk cargo test -p aegis-scanner h3_wipefs
```

Expected: all focused tests pass.

## Task 3: Red tests for cloud/storage and rsync followups

**Files:**
- Modify: `crates/aegis-scanner/src/scanner/tests/basic.rs`
- Modify: `crates/aegis-scanner/src/scanner/tests/h3_gaps.rs`

- [ ] **Step 1: Add failing positive tests**

In `crates/aegis-scanner/src/scanner/tests/basic.rs`, add after `assess_h3_cloud_prefix_rules`:

```rust
// ── H3-followups: gcloud storage recursive delete and rsync delete ─────────
#[test]
fn assess_h3_followup_storage_sync_delete_rules() {
    let cases: &[(&str, RiskLevel, &str)] = &[
        (
            "gcloud storage rm -r gs://my-bucket/data",
            RiskLevel::Danger,
            "CL-014",
        ),
        (
            "gcloud storage rm --recursive gs://my-bucket",
            RiskLevel::Danger,
            "CL-014",
        ),
        (
            "gcloud --project prod storage rm gs://my-bucket --recursive",
            RiskLevel::Danger,
            "CL-014",
        ),
        (
            "rsync -av --delete ./dist/ deploy:/srv/site/",
            RiskLevel::Warn,
            "FS-015",
        ),
        (
            "rsync --dry-run --delete-after ./dist/ deploy:/srv/site/",
            RiskLevel::Warn,
            "FS-015",
        ),
        (
            "rsync -n --delete-excluded ./dist/ deploy:/srv/site/",
            RiskLevel::Warn,
            "FS-015",
        ),
    ];
    for (cmd, risk, id) in cases {
        assert_assessment_matches_pattern(cmd, *risk, id);
    }
}
```

- [ ] **Step 2: Add failing narrowness tests**

In `crates/aegis-scanner/src/scanner/tests/h3_gaps.rs`, add:

```rust
#[test]
fn h3_followup_gcloud_storage_rm_without_recursive_stays_below_danger() {
    let s = scanner();
    let assessment = s.assess("gcloud storage rm gs://my-bucket/file.txt");
    assert!(
        !assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "CL-014"),
        "CL-014 must not fire without recursive flag: {:?}",
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref())
            .collect::<Vec<_>>()
    );
    assert!(
        assessment.risk < RiskLevel::Danger,
        "single-object gcloud storage rm must stay below Danger (got {:?})",
        assessment.risk
    );
}

#[test]
fn h3_followup_rsync_without_delete_stays_safe() {
    let s = scanner();
    let assessment = s.assess("rsync -av ./dist/ deploy:/srv/site/");
    assert_eq!(
        assessment.risk,
        RiskLevel::Safe,
        "rsync without delete flags must stay Safe: got {:?} / {:?}",
        assessment.risk,
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref())
            .collect::<Vec<_>>()
    );
}
```

- [ ] **Step 3: Run focused tests and confirm RED**

Run:

```bash
rtk cargo test -p aegis-scanner assess_h3_followup_storage_sync_delete_rules
rtk cargo test -p aegis-scanner h3_followup_gcloud_storage_rm_without_recursive_stays_below_danger
rtk cargo test -p aegis-scanner h3_followup_rsync_without_delete_stays_safe
```

Expected: positive test fails for missing `CL-014` / `FS-015`; narrowness tests pass or remain green.

## Task 4: Implement `CL-014` and `FS-015`

**Files:**
- Modify: `crates/aegis-scanner/src/patterns/builtins_a.rs`

- [ ] **Step 1: Add `FS-015` in the Filesystem prefix-rule block**

Add after `FS-012`:

```rust
PrefixRule {
    id: Cow::Borrowed("FS-015"),
    category: Category::Filesystem,
    pattern: vec![
        s("rsync"),
        any_star(),
        a(&[
            "--delete",
            "--delete-before",
            "--delete-during",
            "--delete-delay",
            "--delete-after",
            "--delete-excluded",
        ]),
    ],
    risk: RiskLevel::Warn,
    description: Cow::Borrowed(
        "rsync --delete — removes destination files that are absent from the source",
    ),
    safe_alt: Some(Cow::Borrowed(
        "Dry-run first: 'rsync -n --delete <source> <destination>' and verify the destination path before syncing",
    )),
    justification: Some(Cow::Borrowed(
        "With delete flags, rsync removes files from the destination that are missing from the source. A wrong source or destination path can erase deployed or remote files.",
    )),
    source: PatternSource::Builtin,
    match_examples: &[
        "rsync -av --delete ./dist/ deploy:/srv/site/",
        "rsync --delete-after ./dist/ deploy:/srv/site/",
        "rsync --delete-excluded ./dist/ deploy:/srv/site/",
    ],
    not_match_examples: &["rsync -av ./dist/ deploy:/srv/site/"],
},
```

- [ ] **Step 2: Add `CL-014` in the Cloud prefix-rule block**

Add after `CL-013`:

```rust
PrefixRule {
    id: Cow::Borrowed("CL-014"),
    category: Category::Cloud,
    pattern: vec![
        s("gcloud"),
        any_star(),
        s("storage"),
        s("rm"),
        any_star(),
        a(&["-r", "-R", "--recursive"]),
    ],
    risk: RiskLevel::Danger,
    description: Cow::Borrowed(
        "gcloud storage rm --recursive — recursively deletes Cloud Storage objects or buckets",
    ),
    safe_alt: Some(Cow::Borrowed(
        "List objects first: 'gcloud storage ls gs://<bucket>/<prefix>' and enable object versioning before recursive deletes",
    )),
    justification: Some(Cow::Borrowed(
        "Recursive Cloud Storage deletion can remove all objects under a prefix and may delete the bucket when aimed at a bucket URL. Versioning is the primary recovery path.",
    )),
    source: PatternSource::Builtin,
    match_examples: &[
        "gcloud storage rm -r gs://my-bucket/data",
        "gcloud storage rm --recursive gs://my-bucket",
        "gcloud --project prod storage rm gs://my-bucket --recursive",
    ],
    not_match_examples: &["gcloud storage rm gs://my-bucket/file.txt"],
},
```

- [ ] **Step 3: Run focused tests and example validation**

Run:

```bash
rtk cargo test -p aegis-scanner assess_h3_followup_storage_sync_delete_rules
rtk cargo test -p aegis-scanner h3_followup_gcloud_storage_rm_without_recursive_stays_below_danger
rtk cargo test -p aegis-scanner h3_followup_rsync_without_delete_stays_safe
```

Expected: all focused tests pass.

## Task 5: Red tests for device wipers

**Files:**
- Modify: `crates/aegis-scanner/src/scanner/tests/basic.rs`
- Modify: `crates/aegis-scanner/src/scanner/tests/h3_gaps.rs`

- [ ] **Step 1: Add failing positive tests**

In `crates/aegis-scanner/src/scanner/tests/basic.rs`, add:

```rust
// ── H3-followups: device wipers ────────────────────────────────────────────
#[test]
fn assess_h3_followup_device_wiper_rules() {
    let cases: &[(&str, RiskLevel, &str)] = &[
        ("blkdiscard /dev/sda", RiskLevel::Block, "FS-016"),
        ("sudo blkdiscard -f /dev/nvme0n1", RiskLevel::Block, "FS-016"),
        ("/usr/sbin/blkdiscard /dev/sdb", RiskLevel::Block, "FS-016"),
        ("sgdisk --zap-all /dev/sda", RiskLevel::Danger, "FS-017"),
        ("sgdisk -Z /dev/sda", RiskLevel::Danger, "FS-017"),
        ("parted /dev/sda mklabel gpt", RiskLevel::Danger, "FS-018"),
        ("parted -s /dev/sda mklabel msdos", RiskLevel::Danger, "FS-018"),
        ("parted /dev/sda rm 1", RiskLevel::Danger, "FS-018"),
    ];
    for (cmd, risk, id) in cases {
        assert_assessment_matches_pattern(cmd, *risk, id);
    }
}
```

- [ ] **Step 2: Add failing narrowness tests**

In `crates/aegis-scanner/src/scanner/tests/h3_gaps.rs`, add:

```rust
#[test]
fn h3_followup_device_inspection_commands_stay_safe() {
    let s = scanner();
    for cmd in ["parted /dev/sda print", "parted -l", "sgdisk --print /dev/sda"] {
        let assessment = s.assess(cmd);
        assert_eq!(
            assessment.risk,
            RiskLevel::Safe,
            "{cmd:?} must stay Safe: got {:?} / {:?}",
            assessment.risk,
            assessment
                .matched
                .iter()
                .map(|m| m.pattern.id.as_ref())
                .collect::<Vec<_>>()
        );
    }
}
```

- [ ] **Step 3: Run focused tests and confirm RED**

Run:

```bash
rtk cargo test -p aegis-scanner assess_h3_followup_device_wiper_rules
rtk cargo test -p aegis-scanner h3_followup_device_inspection_commands_stay_safe
```

Expected: positive test fails for missing `FS-016` / `FS-017` / `FS-018`; inspection narrowness passes or remains green.

## Task 6: Implement device wiper prefix rules

**Files:**
- Modify: `crates/aegis-scanner/src/patterns/builtins_a.rs`

- [ ] **Step 1: Add `FS-016`, `FS-017`, `FS-018` in the Filesystem block**

Add after `FS-015`:

```rust
PrefixRule {
    id: Cow::Borrowed("FS-016"),
    category: Category::Filesystem,
    pattern: vec![s("blkdiscard")],
    risk: RiskLevel::Block,
    description: Cow::Borrowed(
        "blkdiscard — discards all blocks on a block device, effectively wiping stored data",
    ),
    safe_alt: Some(Cow::Borrowed(
        "Verify the target with 'lsblk' and use a dedicated disk-wipe workflow outside Aegis if this is intentional",
    )),
    justification: Some(Cow::Borrowed(
        "blkdiscard can make device data unrecoverable immediately. Aegis treats this as an intrinsic Block-level wipe operation.",
    )),
    source: PatternSource::Builtin,
    match_examples: &["blkdiscard /dev/sda", "sudo blkdiscard -f /dev/nvme0n1"],
    not_match_examples: &["lsblk /dev/sda"],
},
PrefixRule {
    id: Cow::Borrowed("FS-017"),
    category: Category::Filesystem,
    pattern: vec![s("sgdisk"), any_star(), a(&["--zap-all", "-Z"])],
    risk: RiskLevel::Danger,
    description: Cow::Borrowed(
        "sgdisk --zap-all — destroys GPT and MBR partition table data on a disk",
    ),
    safe_alt: Some(Cow::Borrowed(
        "Back up the partition table first: 'sgdisk --backup=table.gpt /dev/sdX' and verify the device with 'lsblk'",
    )),
    justification: Some(Cow::Borrowed(
        "Zapping partition metadata can make all partitions inaccessible. Recovery depends on having the original layout saved.",
    )),
    source: PatternSource::Builtin,
    match_examples: &["sgdisk --zap-all /dev/sda", "sgdisk -Z /dev/sda"],
    not_match_examples: &["sgdisk --print /dev/sda"],
},
PrefixRule {
    id: Cow::Borrowed("FS-018"),
    category: Category::Filesystem,
    pattern: vec![s("parted"), any_star(), a(&["mklabel", "rm"])],
    risk: RiskLevel::Danger,
    description: Cow::Borrowed(
        "parted mklabel/rm — rewrites a partition table label or removes a partition",
    ),
    safe_alt: Some(Cow::Borrowed(
        "Print and back up the partition layout first: 'parted /dev/sdX print' and 'sfdisk -d /dev/sdX > table.bak'",
    )),
    justification: Some(Cow::Borrowed(
        "Partition table changes can make data inaccessible immediately. Confirm the target disk and partition number before proceeding.",
    )),
    source: PatternSource::Builtin,
    match_examples: &[
        "parted /dev/sda mklabel gpt",
        "parted -s /dev/sda mklabel msdos",
        "parted /dev/sda rm 1",
    ],
    not_match_examples: &["parted /dev/sda print", "parted -l"],
},
```

- [ ] **Step 2: Run focused tests**

Run:

```bash
rtk cargo test -p aegis-scanner assess_h3_followup_device_wiper_rules
rtk cargo test -p aegis-scanner h3_followup_device_inspection_commands_stay_safe
```

Expected: all focused tests pass.

## Task 7: Red tests for `redis-cli FLUSHALL` / `FLUSHDB`

**Files:**
- Modify: `crates/aegis-scanner/src/scanner/tests/basic.rs`
- Modify: `crates/aegis-scanner/src/scanner/tests/h3_gaps.rs`

- [ ] **Step 1: Add failing positive tests**

In `crates/aegis-scanner/src/scanner/tests/basic.rs`, add:

```rust
// ── H3-followups: redis-cli DB-006 delivery form ──────────────────────────
#[test]
fn assess_h3_followup_redis_cli_flush_rules() {
    let cases: &[(&str, RiskLevel, &str)] = &[
        ("redis-cli FLUSHALL", RiskLevel::Danger, "DB-006"),
        ("redis-cli -h cache.local -n 0 FLUSHDB", RiskLevel::Danger, "DB-006"),
        ("redis-cli --raw FLUSHALL ASYNC", RiskLevel::Danger, "DB-006"),
        ("sudo redis-cli FLUSHDB", RiskLevel::Danger, "DB-006"),
    ];
    for (cmd, risk, id) in cases {
        assert_assessment_matches_pattern(cmd, *risk, id);
    }
}
```

- [ ] **Step 2: Add failing narrowness test**

In `crates/aegis-scanner/src/scanner/tests/h3_gaps.rs`, add:

```rust
#[test]
fn h3_followup_redis_cli_non_flush_commands_stay_safe() {
    let s = scanner();
    for cmd in ["redis-cli GET mykey", "redis-cli --raw INFO", "redis-cli DBSIZE"] {
        let assessment = s.assess(cmd);
        assert_eq!(
            assessment.risk,
            RiskLevel::Safe,
            "{cmd:?} must stay Safe: got {:?} / {:?}",
            assessment.risk,
            assessment
                .matched
                .iter()
                .map(|m| m.pattern.id.as_ref())
                .collect::<Vec<_>>()
        );
    }
}
```

- [ ] **Step 3: Run focused tests and confirm RED**

Run:

```bash
rtk cargo test -p aegis-scanner assess_h3_followup_redis_cli_flush_rules
rtk cargo test -p aegis-scanner h3_followup_redis_cli_non_flush_commands_stay_safe
```

Expected: positive test fails for `redis-cli` invocations containing `FLUSHALL` or `FLUSHDB`; narrowness passes or remains green.

## Task 8: Implement duplicate-ID `DB-006` redis-cli prefix rule

**Files:**
- Modify: `crates/aegis-scanner/src/patterns/builtins_a.rs`

- [ ] **Step 1: Add a second `DB-006` PrefixRule after the existing bare-command `DB-006`**

Add:

```rust
PrefixRule {
    id: Cow::Borrowed("DB-006"),
    category: Category::Database,
    pattern: vec![s("redis-cli"), any_star(), a(&["FLUSHALL", "FLUSHDB"])],
    risk: RiskLevel::Danger,
    description: Cow::Borrowed(
        "redis-cli FLUSHALL / FLUSHDB — wipes all keys in the cache or selected Redis database",
    ),
    safe_alt: Some(Cow::Borrowed(
        "Use key-pattern-based deletion: 'SCAN + DEL' to remove only the intended keys",
    )),
    justification: Some(Cow::Borrowed(
        "Wipes Redis keys instantly through redis-cli. Redis has no undo; if persistence backups are missing the data is gone forever.",
    )),
    source: PatternSource::Builtin,
    match_examples: &[
        "redis-cli FLUSHALL",
        "redis-cli -h cache.local -n 0 FLUSHDB",
        "redis-cli --raw FLUSHALL ASYNC",
    ],
    not_match_examples: &["redis-cli GET mykey", "redis-cli --raw INFO"],
},
```

- [ ] **Step 2: Run focused tests**

Run:

```bash
rtk cargo test -p aegis-scanner assess_h3_followup_redis_cli_flush_rules
rtk cargo test -p aegis-scanner h3_followup_redis_cli_non_flush_commands_stay_safe
```

Expected: all focused tests pass.

## Task 9: Documentation and backlog updates

**Files:**
- Modify: `TASKS.md`
- Modify: `CHANGELOG.md`
- Modify: `PROJECT_STATE.md`

- [ ] **Step 1: Close `H3-followups` in `TASKS.md`**

Change:

```markdown
#### [ ] H3-followups — siblings deferred from the H3 grill
```

to:

```markdown
#### [x] H3-followups — siblings deferred from the H3 grill
```

Replace the active unresolved bullet list with a dated resolution paragraph:

```markdown
- **Resolution (2026-07-02):** closed the remaining siblings as additive scanner
  hardening: `FS-011` now handles `wipefs` short flag bundles containing `a`;
  `CL-014` covers `gcloud storage rm --recursive`; `FS-015` covers
  `rsync --delete*`; `FS-016` blocks `blkdiscard`; `FS-017` covers
  `sgdisk --zap-all`/`-Z`; `FS-018` covers destructive `parted mklabel`/`rm`;
  `DB-006` now also covers `redis-cli` invocations containing `FLUSHALL` or `FLUSHDB`. Existing resolved
  H3 review follow-ups (`aws` global flags, `tee` to `authorized_keys`) remain
  recorded in the H3 remediation note above.
```

- [ ] **Step 2: Add changelog entry**

Under `## [Unreleased]` in `CHANGELOG.md`, add a `Security` entry if absent:

```markdown
### Security

- Hardened H3-followups scanner coverage for missed destructive CLI forms:
  `wipefs` short flag bundles, `gcloud storage rm --recursive`, `rsync --delete*`,
  `blkdiscard`, `sgdisk --zap-all`/`-Z`, destructive `parted`, and
  `redis-cli FLUSHALL`/`FLUSHDB`.
```

If `### Security` already exists under `[Unreleased]`, prepend the bullet under it.

- [ ] **Step 3: Update `PROJECT_STATE.md`**

Set `Last updated` to `2026-07-02` and add this session summary above the previous current session:

```markdown
## What was done last session (2026-07-02)

- Grilled and planned `H3-followups` scanner hardening. Agreed to close the remaining
  false negatives via additive built-in token-prefix detections (`wipefs` short flag
  bundles, `gcloud storage rm --recursive`, `rsync --delete*`, `blkdiscard`,
  `sgdisk --zap-all`/`-Z`, destructive `parted`, `redis-cli FLUSHALL`/`FLUSHDB`)
  without changing the public prefix matcher/API. Added `Short flag bundle` to
  `CONTEXT.md` and wrote the implementation plan in
  `docs/superpowers/plans/2026-07-02-h3-followups-scanner-hardening.md`.
```

After implementation, replace the wording with actual files changed and verification output.

## Task 10: Verification

**Files:**
- No source edits unless verification surfaces a defect.

- [ ] **Step 1: Run focused scanner follow-up tests**

Run:

```bash
rtk cargo test -p aegis-scanner h3_followup
```

Expected: all `h3_followup_*` tests pass.

- [ ] **Step 2: Run focused original H3 scanner tests**

Run:

```bash
rtk cargo test -p aegis-scanner h3
```

Expected: original H3 and new H3-followup tests pass.

- [ ] **Step 3: Run formatting**

Run:

```bash
rtk cargo fmt --check
```

Expected: exits 0 with no diff.

- [ ] **Step 4: Run clippy**

Run:

```bash
rtk cargo clippy -- -D warnings
```

Expected: exits 0 with no warnings.

- [ ] **Step 5: Run full tests**

Run:

```bash
rtk cargo test
```

Expected: full workspace test suite passes.

- [ ] **Step 6: Decide whether benchmark is required**

If implementation changed generic prefix matching, tokenizer, parser, or added universal regex patterns, run:

```bash
rtk cargo bench --bench scanner_bench
```

Expected: safe-command hot path remains within the project budget. If implementation stayed limited to built-in prefix rules plus the local `FS-011` predicate, record that benchmark was not required by the grill decision.

## Self-Review Checklist

- [ ] Spec coverage: each unresolved `H3-followups` bullet has a positive test and implementation task.
- [ ] Narrowness coverage: non-destructive `parted`/`sgdisk`/`rsync`/`gcloud`/`redis-cli` examples remain `Safe` or below the new rule's risk.
- [ ] Fail-closed invariant: every change is additive or stricter; no existing `Block` or prompt path is weakened.
- [ ] Hot-path boundary: no parser/tokenizer changes, no generic matcher API changes, no universal regex additions.
- [ ] Docs state: `TASKS.md`, `CHANGELOG.md`, `PROJECT_STATE.md`, and existing `CONTEXT.md` glossary update are consistent.

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-02-h3-followups-scanner-hardening.md`.

Two execution options:

1. **Subagent-Driven (recommended)** — dispatch a fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — execute tasks in this session using executing-plans, batch execution with checkpoints.
