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
