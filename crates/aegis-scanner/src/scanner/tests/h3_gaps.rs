use super::*;

// H3 — Pattern database has dangerous gaps: narrowness guards for the seven
// rules added to close the gap (FS-011/012/013/014, CL-011/012/013). The
// positive (must-fire) cases live alongside the other `assess_*` cases in
// `basic.rs`; these guard the opposite direction — the near-miss invocations
// that must NOT raise their rule — so the additions stay narrow and fail-closed
// rather than fail-open.

// ── Filesystem token-prefix narrowness (FS-011 / FS-012) ───────────────────
//
// FS-011 requires the `-a`/`--all` flag; the plain signature-only invocation
// must stay clear. `wipefs -n -a` (dry-run + all) is an accepted fail-safe FP:
// the AnyStar lets `-a` follow `-n`, so it still prompts. FS-012 keys on the
// exact `unlink` program token, so neighbouring link commands stay Safe.
#[test]
fn h3_wipefs_without_all_flag_does_not_fire_fs011() {
    let s = scanner();
    let assessment = s.assess("wipefs /dev/sda");
    assert!(
        !assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "FS-011"),
        "FS-011 must not fire for 'wipefs /dev/sda' (no -a/--all): {:?}",
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref())
            .collect::<Vec<_>>()
    );
    assert!(
        assessment.risk < RiskLevel::Danger,
        "'wipefs /dev/sda' (no -a) must not reach Danger (got {:?})",
        assessment.risk
    );
}

#[test]
fn h3_wipefs_dry_run_with_all_flag_still_fires_fs011() {
    // Accepted fail-safe false positive: `-a` after `-n` still prompts.
    assert_assessment_matches_pattern("wipefs -n -a /dev/sda", RiskLevel::Danger, "FS-011");
}

#[test]
fn h3_unlink_neighbours_stay_safe() {
    let s = scanner();
    for cmd in ["readlink mylink", "ln -s a b"] {
        let assessment = s.assess(cmd);
        assert_eq!(
            assessment.risk,
            RiskLevel::Safe,
            "{cmd:?} must stay Safe (not FS-012): got {:?} / {:?}",
            assessment.risk,
            assessment
                .matched
                .iter()
                .map(|m| m.pattern.id.as_ref())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn h3_wipefs_short_flags_without_all_flag_do_not_fire_fs011() {
    let s = scanner();
    for cmd in [
        "wipefs -n /dev/sda",
        "wipefs -f /dev/sda",
        "wipefs -vn /dev/sda",
    ] {
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

// ── Cloud token-prefix narrowness (CL-011 / CL-012 / CL-013) ───────────────
//
// Each cloud rule keys on the destructive flag (`--force`/`--delete`/`-r`),
// mirroring CL-005. Without that flag the command is an ordinary bucket op and
// must not raise the rule. `aws s3 sync` and `gsutil rm <obj>` (single object)
// stay Safe entirely.
#[test]
fn h3_aws_s3_rb_without_force_does_not_fire_cl011() {
    let s = scanner();
    let assessment = s.assess("aws s3 rb s3://my-bucket");
    assert!(
        !assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "CL-011"),
        "CL-011 must not fire without --force: {:?}",
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref())
            .collect::<Vec<_>>()
    );
    assert!(
        assessment.risk < RiskLevel::Danger,
        "'aws s3 rb s3://my-bucket' (no --force) must not reach Danger (got {:?})",
        assessment.risk
    );
}

#[test]
fn h3_aws_s3_sync_without_delete_stays_safe() {
    let s = scanner();
    let assessment = s.assess("aws s3 sync ./dist s3://my-bucket");
    assert_eq!(
        assessment.risk,
        RiskLevel::Safe,
        "'aws s3 sync' without --delete must be Safe: got {:?} / {:?}",
        assessment.risk,
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn h3_gsutil_rm_single_object_does_not_fire_cl013() {
    let s = scanner();
    let assessment = s.assess("gsutil rm gs://my-bucket/file.txt");
    assert!(
        !assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "CL-013"),
        "CL-013 must not fire for a single-object 'gsutil rm' (no -r): {:?}",
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref())
            .collect::<Vec<_>>()
    );
    assert!(
        assessment.risk < RiskLevel::Danger,
        "'gsutil rm gs://my-bucket/file.txt' (no -r) must not reach Danger (got {:?})",
        assessment.risk
    );
}

// ── H3-followups: cloud storage and rsync narrowness ───────────────────────
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

// ── H3-followups: device inspection narrowness ────────────────────────────
#[test]
fn h3_followup_device_inspection_commands_stay_safe() {
    let s = scanner();
    for cmd in [
        "parted /dev/sda print",
        "parted -l",
        "sgdisk --print /dev/sda",
    ] {
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

// ── H3-followups: redis-cli non-flush narrowness ──────────────────────────
#[test]
fn h3_followup_redis_cli_non_flush_commands_stay_safe() {
    let s = scanner();
    for cmd in [
        "redis-cli GET mykey",
        "redis-cli --raw INFO",
        "redis-cli DBSIZE",
    ] {
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

// ── H3-followups: redis-cli DB-006 over-match narrowness ─────────────────
//
// `redis-cli GET FLUSHALL` must NOT fire DB-006: here FLUSHALL is a key-name
// argument to the GET command, not the Redis flush verb. The current any_star
// pattern causes DB-006 to fire anywhere FLUSHALL/FLUSHDB appears after
// `redis-cli`, which is an over-match. This test will be RED until the DB-006
// redis-cli rule is narrowed to only match when FLUSHALL/FLUSHDB is the actual
// Redis command token (i.e. immediately follows option flags, not another verb).
#[test]
fn test_db006_redis_cli_get_flushall_stays_safe() {
    let s = scanner();
    let assessment = s.assess("redis-cli GET FLUSHALL");
    assert!(
        !assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "DB-006"),
        "DB-006 must not fire when FLUSHALL is a key argument to GET, not the Redis command: {:?}",
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        assessment.risk,
        RiskLevel::Safe,
        "'redis-cli GET FLUSHALL' must stay Safe (FLUSHALL is a key name, not a command): got {:?}",
        assessment.risk
    );
}

// ── Redirect-regex narrowness (FS-013 / FS-014) ────────────────────────────
//
// FS-014 is single-`>` only: an append (`>>`) to a shell-rc file is recoverable
// and must NOT fire. FS-013 requires the redirect to precede `authorized_keys`,
// so a *read* that redirects a backup elsewhere (filename before the `>`) stays
// clear.
#[test]
fn h3_rc_append_does_not_fire_fs014() {
    let s = scanner();
    let assessment = s.assess("echo export PATH=$PATH:/x >> ~/.bashrc");
    assert!(
        !assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "FS-014"),
        "FS-014 must not fire for an append (>>) to a shell-rc file: {:?}",
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn h3_authorized_keys_read_redirect_does_not_fire_fs013() {
    let s = scanner();
    // The filename precedes the `>`, so this is a read/backup, not a write to
    // authorized_keys.
    let assessment = s.assess("cat ~/.ssh/authorized_keys > /tmp/backup");
    assert!(
        !assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "FS-013"),
        "FS-013 must not fire when authorized_keys precedes the redirect: {:?}",
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref())
            .collect::<Vec<_>>()
    );
    assert!(
        assessment.risk < RiskLevel::Danger,
        "'cat ~/.ssh/authorized_keys > /tmp/backup' must not reach Danger (got {:?})",
        assessment.risk
    );
}

#[test]
fn h3_tee_to_unrelated_path_does_not_fire_fs013() {
    let s = scanner();
    // The tee branch must require the authorized_keys target; piping into tee
    // for an ordinary log file is benign.
    let assessment = s.assess("echo line | tee -a /var/log/app.log");
    assert!(
        !assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "FS-013"),
        "FS-013 must not fire for 'tee -a /var/log/app.log': {:?}",
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        assessment.risk,
        RiskLevel::Safe,
        "'echo line | tee -a /var/log/app.log' must be Safe (got {:?})",
        assessment.risk
    );
}
