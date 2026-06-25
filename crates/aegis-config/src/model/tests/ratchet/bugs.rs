// ── bugs-01: docker ratchet must reject intra-rank narrowing/incomparable ─
// The current `ratchet_docker_scope` is rank-only: same-rank moves (Names→
// different Names, Labeled→Names, Labeled→different label, Names→Labeled) are
// KEPT (project wins) with no warning. Desired: under the Project layer, when
// the docker provider is ENABLED in the trusted base AND the base scope is not
// a no-op, a project overlay that NARROWS or is INCOMPARABLE with the base
// eligible-container set is rejected (keep base + warn). Only keep-or-broaden
// is permitted.

#[test]
fn project_docker_scope_names_to_disjoint_names_rejected() {
    // bugs-01 RED: base Names `["prod-db"]`; project Names `["x"]` (disjoint,
    // same rank). Current code keeps project `["x"]` with no warning. Desired:
    // keep base `["prod-db"]` + warn — overlay patterns are NOT a superset of
    // base patterns (some base pattern absent from overlay).
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_docker = true\n[docker_scope]\nmode = \"Names\"\nname_patterns = [\"prod-db\"]\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[docker_scope]\nmode = \"Names\"\nname_patterns = [\"x\"]\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.docker_scope.name_patterns,
        vec!["prod-db".to_string()],
        "project must not narrow docker_scope Names patterns to a disjoint set; got {:?}",
        config.docker_scope.name_patterns
    );
    assert_has_warning_for(
        &warnings,
        "docker_scope",
        "bugs-01 Names→disjoint Names rejected",
    );
}

#[test]
fn project_docker_scope_names_subset_patterns_rejected() {
    // bugs-01 RED: base Names `["a", "b"]`; project Names `["a"]` (subset, same
    // rank). Current code keeps project `["a"]` with no warning. Desired: keep
    // base `["a", "b"]` + warn — overlay patterns are NOT a superset of base
    // patterns (base "b" absent from overlay).
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_docker = true\n[docker_scope]\nmode = \"Names\"\nname_patterns = [\"a\", \"b\"]\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[docker_scope]\nmode = \"Names\"\nname_patterns = [\"a\"]\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.docker_scope.name_patterns,
        vec!["a".to_string(), "b".to_string()],
        "project must not narrow docker_scope Names patterns to a subset; got {:?}",
        config.docker_scope.name_patterns
    );
    assert_has_warning_for(
        &warnings,
        "docker_scope",
        "bugs-01 Names→subset Names rejected",
    );
}

#[test]
fn project_docker_scope_labeled_to_names_rejected() {
    // bugs-01 RED: base default `Labeled` (label "aegis.snapshot"); project
    // `Names` with `["x"]` (same rank, incomparable mode switch). Current code
    // keeps project `Names` with no warning. Desired: keep base `Labeled` +
    // label "aegis.snapshot" + warn.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_docker = true\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[docker_scope]\nmode = \"Names\"\nname_patterns = [\"x\"]\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.docker_scope.mode,
        crate::snapshot::DockerScopeMode::Labeled,
        "project must not switch docker_scope from Labeled to Names (incomparable); got {:?}",
        config.docker_scope.mode
    );
    assert_eq!(
        config.docker_scope.label, "aegis.snapshot",
        "base Labeled label must be kept when the project attempts an incomparable Names switch; got {:?}",
        config.docker_scope.label
    );
    assert_has_warning_for(
        &warnings,
        "docker_scope",
        "bugs-01 Labeled→Names rejected",
    );
}

#[test]
fn project_docker_scope_labeled_to_labeled_different_label_rejected() {
    // bugs-01 RED: base default `Labeled` (label "aegis.snapshot"); project
    // `Labeled` with a different label "other" (same rank, incomparable).
    // Current code keeps project label "other" with no warning. Desired: keep
    // base label "aegis.snapshot" + warn.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_docker = true\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[docker_scope]\nmode = \"Labeled\"\nlabel = \"other\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.docker_scope.label, "aegis.snapshot",
        "project must not repoint the Labeled docker_scope to a different (incomparable) label; got {:?}",
        config.docker_scope.label
    );
    assert_has_warning_for(
        &warnings,
        "docker_scope",
        "bugs-01 Labeled→Labeled different label rejected",
    );
}

#[test]
fn project_docker_scope_names_to_labeled_rejected() {
    // bugs-01 RED: base `Names` with `["prod-db"]`; project `Labeled` (same
    // rank, incomparable mode switch). Current code keeps project `Labeled`
    // with no warning. Desired: keep base `Names` + `["prod-db"]` + warn.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_docker = true\n[docker_scope]\nmode = \"Names\"\nname_patterns = [\"prod-db\"]\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[docker_scope]\nmode = \"Labeled\"\nlabel = \"aegis.snapshot\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.docker_scope.mode,
        crate::snapshot::DockerScopeMode::Names,
        "project must not switch docker_scope from Names to Labeled (incomparable); got {:?}",
        config.docker_scope.mode
    );
    assert_eq!(
        config.docker_scope.name_patterns,
        vec!["prod-db".to_string()],
        "base Names patterns must be kept when the project attempts an incomparable Labeled switch; got {:?}",
        config.docker_scope.name_patterns
    );
    assert_has_warning_for(
        &warnings,
        "docker_scope",
        "bugs-01 Names→Labeled rejected",
    );
}

#[test]
fn project_docker_scope_names_superset_patterns_allowed() {
    // bugs-01 GREEN-BY-DESIGN guard: base Names `["a"]`; project Names
    // `["a", "b"]` (literal-string superset — every base pattern is in the
    // overlay). Broaden-or-equal is permitted: keep project value, NO warning.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_docker = true\n[docker_scope]\nmode = \"Names\"\nname_patterns = [\"a\"]\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[docker_scope]\nmode = \"Names\"\nname_patterns = [\"a\", \"b\"]\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.docker_scope.name_patterns,
        vec!["a".to_string(), "b".to_string()],
        "project must be able to broaden docker_scope Names patterns (superset); got {:?}",
        config.docker_scope.name_patterns
    );
    assert_no_warning_for(
        &warnings,
        "docker_scope",
        "bugs-01 Names superset allowed",
    );
}

#[test]
fn project_docker_scope_labeled_to_labeled_same_label_allowed() {
    // bugs-01 GREEN-BY-DESIGN guard: base default `Labeled` (label
    // "aegis.snapshot"); project `Labeled` with the SAME label. Identical
    // effective scope — keep project value, NO warning.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_docker = true\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[docker_scope]\nmode = \"Labeled\"\nlabel = \"aegis.snapshot\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.docker_scope.label, "aegis.snapshot",
        "project Labeled with the same label as base must keep the label; got {:?}",
        config.docker_scope.label
    );
    assert_no_warning_for(
        &warnings,
        "docker_scope",
        "bugs-01 Labeled same label allowed",
    );
}

// ── bugs-02: do not ratchet under SnapshotPolicy::None ────────────────────
// Under `SnapshotPolicy::None` the registry materializes NO providers, so the
// provider target ratchet must not fire (no spurious keep-base / no spurious
// warning). `provider_enabled_in_base` must be `base.snapshot_policy != None
// && (Full || flag)`.

#[test]
fn project_provider_target_not_ratcheted_under_snapshot_policy_none() {
    // bugs-02 RED: global `snapshot_policy = "None"` + `auto_snapshot_postgres
    // = true` + `[postgres_snapshot] database = "mydb"`; project empties the
    // database. Under None the postgres provider is NEVER materialized, so the
    // ratchet must NOT fire: keep project value "" + NO warning. Current code
    // treats `auto_snapshot_postgres = true` as enabling the provider
    // regardless of policy → keeps "mydb" + warns → test FAILS.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "snapshot_policy = \"None\"\nauto_snapshot_postgres = true\n[postgres_snapshot]\ndatabase = \"mydb\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[postgres_snapshot]\ndatabase = \"\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.postgres_snapshot.database, "",
        "under SnapshotPolicy::None the postgres target ratchet must not fire; got {:?}",
        config.postgres_snapshot.database
    );
    assert_no_warning_for(
        &warnings,
        "postgres_snapshot",
        "bugs-02 no ratchet under SnapshotPolicy::None",
    );
}

// ── regressions-01: reordered equal allow_write subset must NOT warn ───────
// The allow_write warning branch gates on `kept != requested` compared as
// DEBUG STRINGS. A reordered-but-equal project subset (base `["/opt","/tmp"]`,
// project `["/tmp","/opt"]`) yields `kept = ["/opt","/tmp"]` (base order) vs
// `requested = ["/tmp","/opt"]` → Debug strings differ → SPURIOUS warning
// though nothing was weakened. Desired: compare as sets; no warning when the
// project requested nothing outside the base.

#[test]
fn project_sandbox_allow_write_reordered_subset_no_warning() {
    // regressions-01 RED: global `allow_write = ["/opt", "/tmp"]`; project
    // `allow_write = ["/tmp", "/opt"]` (reordered equal set). Effective
    // `allow_write` must equal the set `{"/opt", "/tmp"}` (order-insensitive)
    // AND there must be NO `sandbox.allow_write` warning. Current code
    // spuriously warns because the Debug strings differ in order → FAILS.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[sandbox]\nenabled = true\nallow_write = [\"/opt\", \"/tmp\"]\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nallow_write = [\"/tmp\", \"/opt\"]\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    let mut effective = config.sandbox.allow_write.clone();
    effective.sort();
    let mut expected = vec![
        std::path::PathBuf::from("/opt"),
        std::path::PathBuf::from("/tmp"),
    ];
    expected.sort();
    assert_eq!(
        effective, expected,
        "reordered equal allow_write set must be preserved as a set; got {:?}",
        config.sandbox.allow_write
    );
    assert_no_warning_for(
        &warnings,
        "sandbox.allow_write",
        "regressions-01 reordered equal subset no warning",
    );
}

// ── tests-01: backfill missing GREEN-BY-DESIGN repoint guards ─────────────
// Pin already-correct repoint behavior for mysql and supabase so the bugs-01 /
// bugs-02 fixes cannot regress them.

#[test]
fn project_mysql_snapshot_can_repoint_database_when_enabled() {
    // tests-01 GREEN-BY-DESIGN: global `auto_snapshot_mysql = true` +
    // `[mysql_snapshot] database = "mydb"`; project repoints to "otherdb".
    // Repointing an enabled provider to another non-empty target is permitted
    // — keep project value, NO warning. Current code already allows this.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_mysql = true\n[mysql_snapshot]\ndatabase = \"mydb\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[mysql_snapshot]\ndatabase = \"otherdb\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.mysql_snapshot.database, "otherdb",
        "project must be able to repoint mysql to another non-empty database; got {:?}",
        config.mysql_snapshot.database
    );
    assert_no_warning_for(&warnings, "mysql_snapshot", "tests-01 mysql repoint");
}

#[test]
fn project_supabase_snapshot_can_repoint_db_when_enabled() {
    // tests-01 GREEN-BY-DESIGN: global `auto_snapshot_supabase = true` +
    // `[supabase_snapshot.db] database = "supadb"`; project repoints
    // `db.database` to "otherdb". Repointing an enabled provider to another
    // non-empty target is permitted — keep project value, NO warning. Current
    // code already allows this.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_supabase = true\n[supabase_snapshot.db]\ndatabase = \"supadb\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[supabase_snapshot.db]\ndatabase = \"otherdb\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.supabase_snapshot.db.database, "otherdb",
        "project must be able to repoint supabase db.database to another non-empty database; got {:?}",
        config.supabase_snapshot.db.database
    );
    assert_no_warning_for(
        &warnings,
        "supabase_snapshot",
        "tests-01 supabase repoint",
    );
}