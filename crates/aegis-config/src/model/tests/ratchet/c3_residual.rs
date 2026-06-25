// ── C3-residual: two security-critical paths left out of ADR-013 ratchet ──
//
// Fix 1: a project-layer `[[rules]]` entry with `decision = "Allow"` must be
// DROPPED at merge (project may not auto-approve via rules — only tighten via
// Prompt/Block) and surfaced as a `project_security_ratchet` warning. Global
// `[[rules]] Allow` stays honored (global is trusted, last-wins). Project
// Prompt/Block rules are still honored (project may tighten).
//
// Fix 2: `audit.integrity_mode` must be ratcheted (stricter of base/requested
// wins under the Project layer). `ChainSha256` is stricter than `Off`. Global
// stays last-wins. The warning fires iff `kept != requested` under the Project
// layer, mirroring `push_ratchet_warning`.
//
// These tests assert on the OBSERVABLE merged config + the warnings Vec so they
// pass under EITHER correct implementation approach (provenance-based or
// direct-filter). They must FAIL against the current (un-ratcheted) code.

// ── Fix 1: project `[[rules]] decision = "Allow"` dropped + warned ────────

/// Returns the full ratchet warning list (with requested/kept) for a project
/// layer. Inlined here so the fragment can inspect `requested`/`kept` without
/// touching `ratchet_helpers.rs` (whose return type elides them).
fn full_project_warnings(
    base: &AegisConfig,
    project_path: &std::path::Path,
) -> Vec<crate::model::ratchet::SecurityRatchetWarning> {
    let layer = ConfigLayerPath {
        source_layer: ConfigSourceLayer::Project,
        path: project_path.to_path_buf(),
    };
    AegisConfig::project_security_ratchet_warnings(base, &layer).unwrap_or_default()
}

#[test]
fn project_rules_allow_dropped_and_warned() {
    // C3-residual Fix-1 case 1: base has NO rules; project adds a `[[rules]]
    // decision = "allow"`. The Allow entry must be DROPPED (not honored) and a
    // `rules` ratchet warning must fire.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    // No [[rules]] in base — base is fully trusted-default.
    fs::write(global_dir.join(GLOBAL_CONFIG_FILE), "mode = \"Protect\"\n").unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[[rules]]\npattern = [\"terraform\"]\ndecision = \"allow\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert!(
        config
            .rules
            .iter()
            .all(|r| r.decision != PolicyRuleDecision::Allow),
        "project-layer [[rules]] Allow must be dropped at merge; got rules = {:?}",
        config.rules
    );
    assert!(
        config.rules.is_empty(),
        "with no base rules, the merged rules Vec must be empty after dropping the project Allow; got {:?}",
        config.rules
    );
    assert_has_warning_for(&warnings, "rules", "C3-residual Fix-1 project Allow dropped");
}

#[test]
fn project_rules_allow_dropped_preserves_base_allow() {
    // C3-residual Fix-1 case 1 (parity guard): when the base (global) already
    // has a `[[rules]] Allow`, a project-layer Allow must still be dropped while
    // the base Allow survives — only the PROJECT Allow is dropped + warned.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[[rules]]\npattern = [\"git\"]\ndecision = \"allow\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[[rules]]\npattern = [\"terraform\"]\ndecision = \"allow\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    // Base (global) Allow for "git" survives.
    assert!(
        config.rules.iter().any(|r| {
            r.decision == PolicyRuleDecision::Allow
                && matches!(
                    r.pattern.first(),
                    Some(PolicyPatternToken::Single(s)) if s == "git"
                )
        }),
        "global-layer [[rules]] Allow must survive the project merge; got rules = {:?}",
        config.rules
    );
    // Project Allow for "terraform" is dropped.
    assert!(
        !config.rules.iter().any(|r| {
            matches!(
                r.pattern.first(),
                Some(PolicyPatternToken::Single(s)) if s == "terraform"
            )
        }),
        "project-layer [[rules]] Allow must be dropped; got rules = {:?}",
        config.rules
    );
    assert_has_warning_for(&warnings, "rules", "C3-residual Fix-1 project Allow dropped (base kept)");
}

#[test]
fn project_rules_prompt_honored_no_warning() {
    // C3-residual Fix-1 case 2: project-layer `[[rules]] decision = "prompt"`
    // (tightening) is STILL honored and produces NO ratchet warning.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(global_dir.join(GLOBAL_CONFIG_FILE), "mode = \"Protect\"\n").unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[[rules]]\npattern = [\"terraform\"]\ndecision = \"prompt\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.rules.len(),
        1,
        "project Prompt rule must be honored (present in merged rules); got {:?}",
        config.rules
    );
    assert_eq!(config.rules[0].decision, PolicyRuleDecision::Prompt);
    assert_no_warning_for(&warnings, "rules", "C3-residual Fix-1 project Prompt honored");
}

#[test]
fn project_rules_block_honored_no_warning() {
    // C3-residual Fix-1 case 3: project-layer `[[rules]] decision = "block"`
    // (tightening) is STILL honored and produces NO ratchet warning.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(global_dir.join(GLOBAL_CONFIG_FILE), "mode = \"Protect\"\n").unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[[rules]]\npattern = [\"terraform\"]\ndecision = \"block\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.rules.len(),
        1,
        "project Block rule must be honored (present in merged rules); got {:?}",
        config.rules
    );
    assert_eq!(config.rules[0].decision, PolicyRuleDecision::Block);
    assert_no_warning_for(&warnings, "rules", "C3-residual Fix-1 project Block honored");
}

#[test]
fn global_rules_allow_honored_no_warning() {
    // C3-residual Fix-1 case 4: a GLOBAL-layer `[[rules]] decision = "allow"`
    // is STILL honored (present in merged rules) and triggers NO ratchet
    // warning (global is trusted, last-wins). The project file is empty so no
    // project-layer rules are requested.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[[rules]]\npattern = [\"git\"]\ndecision = \"allow\"\n",
    )
    .unwrap();
    // Empty project file — ensures a project layer exists but requests nothing.
    fs::write(workspace.path().join(PROJECT_CONFIG_FILE), "").unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.rules.len(),
        1,
        "global [[rules]] Allow must be honored; got {:?}",
        config.rules
    );
    assert_eq!(config.rules[0].decision, PolicyRuleDecision::Allow);
    assert_no_warning_for(&warnings, "rules", "C3-residual Fix-1 global Allow honored");
}

// ── C3-residual Fix-1 bypass (iteration 2): a project-layer `[[rules]]`
// entry whose top-level `decision = "prompt"` (or `"block"`) but whose
// `when.then = "allow"` is a same-class auto-approve bypass. At runtime
// `effective_decision` returns `when.then = Allow` when the env condition
// matches, silently auto-approving a Danger command — exactly what Fix-1
// exists to prevent. Such entries must be DROPPED at the project merge
// (exactly like a plain `decision = "allow"` rule) and surface a `"rules"`
// ratchet warning. The test asserts on OBSERVABLE behavior (merged `rules`
// Vec contents + warnings Vec) so it passes under any correct implementation.

#[test]
fn project_rules_prompt_with_when_then_allow_dropped_and_warned() {
    // C3-residual Fix-1 bypass: project `[[rules]] decision = "prompt"` with
    // `when.then = "allow"` must be DROPPED at merge and warned, exactly like a
    // plain `decision = "allow"` rule. Currently `is_untrusted_allow` only
    // checks `rule.decision == Allow`, so this rule SURVIVES the project-layer
    // drop — RED.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    // No [[rules]] in base — base is fully trusted-default.
    fs::write(global_dir.join(GLOBAL_CONFIG_FILE), "mode = \"Protect\"\n").unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[[rules]]\n\
         pattern = [\"terraform\"]\n\
         decision = \"prompt\"\n\
         when = { env = \"HOME\", value = \"/root\", then = \"allow\" }\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    // The rule must NOT survive into the merged config — assert no rule whose
    // `when.then == Allow` is present (covers both `decision = "prompt"` and
    // `decision = "block"` shapes).
    assert!(
        config.rules.iter().all(|r| {
            r.when
                .as_ref()
                .map(|w| w.then != PolicyRuleDecision::Allow)
                .unwrap_or(true)
        }),
        "project-layer [[rules]] with when.then = \"allow\" must be dropped at merge; \
         got rules = {:?}",
        config.rules,
    );
    assert!(
        config.rules.is_empty(),
        "with no base rules, the merged rules Vec must be empty after dropping the \
         project `when.then = allow` rule; got {:?}",
        config.rules,
    );
    assert_has_warning_for(
        &warnings,
        "rules",
        "C3-residual Fix-1 bypass: project prompt+when.then=allow must warn",
    );
}

#[test]
fn project_rules_block_with_when_then_allow_dropped_and_warned() {
    // C3-residual Fix-1 bypass (block shape): project `[[rules]] decision =
    // "block"` with `when.then = "allow"` must also be DROPPED at merge and
    // warned. Same root cause as the prompt shape — `is_untrusted_allow` must
    // flag any rule whose `when.then == Allow`, regardless of the top-level
    // `decision`. RED until fixed.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(global_dir.join(GLOBAL_CONFIG_FILE), "mode = \"Protect\"\n").unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[[rules]]\n\
         pattern = [\"terraform\"]\n\
         decision = \"block\"\n\
         when = { env = \"CI\", value = \"true\", then = \"allow\" }\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert!(
        config.rules.iter().all(|r| {
            r.when
                .as_ref()
                .map(|w| w.then != PolicyRuleDecision::Allow)
                .unwrap_or(true)
        }),
        "project-layer [[rules]] block with when.then = \"allow\" must be dropped at merge; \
         got rules = {:?}",
        config.rules,
    );
    assert!(
        config.rules.is_empty(),
        "with no base rules, the merged rules Vec must be empty after dropping the \
         project block+when.then=allow rule; got {:?}",
        config.rules,
    );
    assert_has_warning_for(
        &warnings,
        "rules",
        "C3-residual Fix-1 bypass: project block+when.then=allow must warn",
    );
}

// ── Fix 2: `audit.integrity_mode` ratcheted ──────────────────────────────

#[test]
fn project_audit_integrity_mode_off_weakened_from_chainsha256_kept_and_warned() {
    // C3-residual Fix-2 case 6: base `ChainSha256` (default), project requests
    // `Off`. Stricter of base/requested is kept (`ChainSha256`); a warning fires
    // with field `"audit.integrity_mode"`, requested `"Off"`, kept `"ChainSha256"`.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    // No [audit] in global — base defaults to ChainSha256.
    fs::write(global_dir.join(GLOBAL_CONFIG_FILE), "mode = \"Protect\"\n").unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[audit]\nintegrity_mode = \"Off\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = full_project_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.audit.integrity_mode,
        AuditIntegrityMode::ChainSha256,
        "project must NOT weaken integrity_mode from ChainSha256 to Off; got {:?}",
        config.audit.integrity_mode
    );

    let warning = warnings
        .iter()
        .find(|w| w.field == "audit.integrity_mode");
    let warning = warning.expect("expected an `audit.integrity_mode` ratchet warning");
    assert_eq!(
        warning.requested, "Off",
        "warning.requested must be the project-requested value `Off`"
    );
    assert_eq!(
        warning.kept, "ChainSha256",
        "warning.kept must be the effective merged value `ChainSha256`"
    );
}

#[test]
fn project_audit_integrity_mode_chainsha256_tightens_from_off_honored_no_warning() {
    // C3-residual Fix-2 case 7: base `Off`, project requests `ChainSha256`
    // (tightening). Merged is `ChainSha256`, NO warning (kept == requested).
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[audit]\nintegrity_mode = \"Off\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[audit]\nintegrity_mode = \"ChainSha256\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = full_project_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.audit.integrity_mode,
        AuditIntegrityMode::ChainSha256,
        "project tightening Off→ChainSha256 must be honored; got {:?}",
        config.audit.integrity_mode
    );
    assert!(
        !warnings
            .iter()
            .any(|w| w.field == "audit.integrity_mode"),
        "tightening must NOT warn; got warnings = {:?}",
        warnings
    );
}

#[test]
fn project_audit_integrity_mode_chainsha256_equal_no_warning() {
    // C3-residual Fix-2 case 8: base `ChainSha256`, project `ChainSha256`
    // (equal). Merged `ChainSha256`, NO warning.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(global_dir.join(GLOBAL_CONFIG_FILE), "mode = \"Protect\"\n").unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[audit]\nintegrity_mode = \"ChainSha256\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = full_project_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.audit.integrity_mode,
        AuditIntegrityMode::ChainSha256,
        "equal ChainSha256 must stay ChainSha256; got {:?}",
        config.audit.integrity_mode
    );
    assert!(
        !warnings
            .iter()
            .any(|w| w.field == "audit.integrity_mode"),
        "equal value must NOT warn; got warnings = {:?}",
        warnings
    );
}

#[test]
fn global_audit_integrity_mode_off_last_wins_no_warning() {
    // C3-residual Fix-2 case 9: a GLOBAL-layer `integrity_mode = "Off"` overlay
    // on base `ChainSha256` is honored (global last-wins, trusted) and triggers
    // NO ratchet warning (the ratchet only gates the Project layer).
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[audit]\nintegrity_mode = \"Off\"\n",
    )
    .unwrap();
    // Empty project file so a project layer exists but requests nothing.
    fs::write(workspace.path().join(PROJECT_CONFIG_FILE), "").unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = full_project_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.audit.integrity_mode,
        AuditIntegrityMode::Off,
        "global-layer integrity_mode = Off must be honored (last-wins); got {:?}",
        config.audit.integrity_mode
    );
    assert!(
        !warnings
            .iter()
            .any(|w| w.field == "audit.integrity_mode"),
        "global layer must NOT trigger a project ratchet warning; got warnings = {:?}",
        warnings
    );
}

#[test]
fn project_audit_integrity_mode_off_equal_to_off_base_no_warning() {
    // C3-residual Fix-2 case 10: base `Off`, project `Off` (equal). Merged `Off`,
    // NO warning (kept == requested).
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[audit]\nintegrity_mode = \"Off\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[audit]\nintegrity_mode = \"Off\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = full_project_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.audit.integrity_mode,
        AuditIntegrityMode::Off,
        "equal Off must stay Off; got {:?}",
        config.audit.integrity_mode
    );
    assert!(
        !warnings
            .iter()
            .any(|w| w.field == "audit.integrity_mode"),
        "equal Off must NOT warn; got warnings = {:?}",
        warnings
    );
}