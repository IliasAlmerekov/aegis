// ── C3-01: provider target config must ratchet, not last-wins ──────────
// When a provider is ENABLED in the trusted base (either via its
// `auto_snapshot_<provider>` flag OR via `snapshot_policy = "Full"`, which
// materializes every built-in provider regardless of the flags), the project
// layer must NOT replace that provider's target config with a disabled/empty/
// narrower one. Weakening is ignored (keep base) and a
// `project_security_ratchet` warning is emitted. Repointing to another ENABLED
// (non-empty) target is permitted and is NOT a bypass.

#[test]
fn project_sqlite_snapshot_path_cannot_empty_global_enabled() {
    // C3-01: sqlite provider is enabled in base (auto_snapshot_sqlite = true
    // with a non-empty path); project empties the path → keep base + warn.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_sqlite = true\nsqlite_snapshot_path = \"/opt/db.sqlite\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "sqlite_snapshot_path = \"\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.sqlite_snapshot_path, "/opt/db.sqlite",
        "project must not empty a globally-enabled sqlite snapshot path; got {:?}",
        config.sqlite_snapshot_path
    );
    assert_has_warning_for(&warnings, "sqlite_snapshot_path", "C3-01 sqlite empty");
}

#[test]
fn project_sqlite_snapshot_path_can_repoint_to_nonempty() {
    // C3-01: repointing an enabled provider to another non-empty target is
    // permitted (not a bypass) — keep project value, NO warning.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_sqlite = true\nsqlite_snapshot_path = \"/opt/db.sqlite\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "sqlite_snapshot_path = \"/other/db.sqlite\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.sqlite_snapshot_path, "/other/db.sqlite",
        "project must be able to repoint sqlite to another non-empty path; got {:?}",
        config.sqlite_snapshot_path
    );
    assert_no_warning_for(&warnings, "sqlite_snapshot_path", "C3-01 sqlite repoint");
}

#[test]
fn project_postgres_snapshot_cannot_empty_database_when_enabled() {
    // C3-01: postgres provider enabled in base; project empties database →
    // keep base database + warn.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_postgres = true\n[postgres_snapshot]\ndatabase = \"mydb\"\n",
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
        config.postgres_snapshot.database, "mydb",
        "project must not empty a globally-enabled postgres database; got {:?}",
        config.postgres_snapshot.database
    );
    assert_has_warning_for(&warnings, "postgres_snapshot", "C3-01 postgres empty");
}

#[test]
fn project_postgres_snapshot_can_repoint_database_when_enabled() {
    // C3-01: repointing an enabled postgres provider to another non-empty
    // database is permitted — keep project value, NO warning.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_postgres = true\n[postgres_snapshot]\ndatabase = \"mydb\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[postgres_snapshot]\ndatabase = \"otherdb\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.postgres_snapshot.database, "otherdb",
        "project must be able to repoint postgres to another non-empty database; got {:?}",
        config.postgres_snapshot.database
    );
    assert_no_warning_for(&warnings, "postgres_snapshot", "C3-01 postgres repoint");
}

#[test]
fn project_mysql_snapshot_cannot_empty_database_when_enabled() {
    // C3-01: mysql provider enabled in base; project empties database →
    // keep base database + warn.
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
        "[mysql_snapshot]\ndatabase = \"\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.mysql_snapshot.database, "mydb",
        "project must not empty a globally-enabled mysql database; got {:?}",
        config.mysql_snapshot.database
    );
    assert_has_warning_for(&warnings, "mysql_snapshot", "C3-01 mysql empty");
}

#[test]
fn project_supabase_snapshot_cannot_empty_db_when_enabled() {
    // C3-01: supabase provider enabled in base; project empties
    // supabase_snapshot.db.database → keep base + warn.
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
        "[supabase_snapshot.db]\ndatabase = \"\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.supabase_snapshot.db.database, "supadb",
        "project must not empty a globally-enabled supabase db.database; got {:?}",
        config.supabase_snapshot.db.database
    );
    assert_has_warning_for(&warnings, "supabase_snapshot", "C3-01 supabase empty");
}

#[test]
fn project_provider_target_not_ratcheted_when_base_disabled() {
    // C3-01 guard: the target ratchet is conditional on the base provider being
    // ENABLED. When the base flag is false (provider off), the project may
    // enable the provider AND configure its own target — no ratchet, no warn.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_postgres = false\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "auto_snapshot_postgres = true\n[postgres_snapshot]\ndatabase = \"projdb\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.postgres_snapshot.database, "projdb",
        "project must be free to configure its own provider when the base leaves it off; got {:?}",
        config.postgres_snapshot.database
    );
    assert_no_warning_for(&warnings, "postgres_snapshot", "C3-01 base-disabled guard");
}

#[test]
fn project_provider_target_ratcheted_under_snapshot_policy_full() {
    // C3-01: under `snapshot_policy = "Full"` every built-in provider is
    // materialized regardless of its `auto_snapshot_*` flag, so a project that
    // empties an enabled provider's target must be ratcheted even though the
    // per-provider flag is at its default `false`.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "snapshot_policy = \"Full\"\nsqlite_snapshot_path = \"/opt/db.sqlite\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "sqlite_snapshot_path = \"\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.sqlite_snapshot_path, "/opt/db.sqlite",
        "project must not empty a sqlite target enabled via snapshot_policy=Full; got {:?}",
        config.sqlite_snapshot_path
    );
    assert_has_warning_for(
        &warnings,
        "sqlite_snapshot_path",
        "C3-01 Full-policy ratchet",
    );
}

#[test]
fn project_docker_scope_cannot_narrow_all_to_labeled_when_enabled() {
    // C3-01 docker: breadth rank All > Labeled. Provider enabled in base with
    // mode = "All"; project narrows to "Labeled" → keep base (All) + warn.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_docker = true\n[docker_scope]\nmode = \"All\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[docker_scope]\nmode = \"Labeled\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.docker_scope.mode,
        crate::snapshot::DockerScopeMode::All,
        "project must not narrow docker_scope from All to Labeled; got {:?}",
        config.docker_scope.mode
    );
    assert_has_warning_for(&warnings, "docker_scope", "C3-01 docker All→Labeled");
}

#[test]
fn project_docker_scope_cannot_introduce_noop_names_empty_when_enabled() {
    // C3-01 docker: Names with empty name_patterns is a no-op (breadth 0),
    // narrower than Labeled (breadth 1). Provider enabled in base with
    // mode = "Labeled"; project switches to "Names" with no patterns → keep
    // base (Labeled) + warn.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_docker = true\n[docker_scope]\nmode = \"Labeled\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[docker_scope]\nmode = \"Names\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.docker_scope.mode,
        crate::snapshot::DockerScopeMode::Labeled,
        "project must not narrow docker_scope from Labeled to a no-op Names; got {:?}",
        config.docker_scope.mode
    );
    assert_has_warning_for(
        &warnings,
        "docker_scope",
        "C3-01 docker Labeled→Names(empty)",
    );
}

#[test]
fn project_docker_scope_can_broaden_labeled_to_all() {
    // C3-01 docker guard: broadening the scope is permitted (not a bypass) —
    // keep project value (All), NO warning.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_docker = true\n[docker_scope]\nmode = \"Labeled\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[docker_scope]\nmode = \"All\"\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.docker_scope.mode,
        crate::snapshot::DockerScopeMode::All,
        "project must be able to broaden docker_scope from Labeled to All; got {:?}",
        config.docker_scope.mode
    );
    assert_no_warning_for(&warnings, "docker_scope", "C3-01 docker broaden");
}

// ── C3-02: sandbox.allow_write must support project tightening (intersection)
// Current merge always returns `base.allow_write` under the Project layer, so
// a project cannot tighten to a subset. Desired: keep `base ∩ requested`.
// Tightening (subset) honored, no warning. Expansion (paths outside base)
// dropped, warn.

#[test]
fn project_sandbox_allow_write_can_tighten_to_subset() {
    // C3-02: project may tighten the writable surface to a subset of the base.
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
        "[sandbox]\nallow_write = [\"/opt\"]\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.sandbox.allow_write,
        vec![std::path::PathBuf::from("/opt")],
        "project allow_write tightening to a subset must be honored; got {:?}",
        config.sandbox.allow_write
    );
    assert_no_warning_for(&warnings, "sandbox.allow_write", "C3-02 tighten-subset");
}

#[test]
fn project_sandbox_allow_write_expansion_dropped_and_warned() {
    // C3-02: paths outside the base are dropped (keep intersection) + warn.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[sandbox]\nenabled = true\nallow_write = [\"/opt\"]\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nallow_write = [\"/opt\", \"/etc\"]\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert_eq!(
        config.sandbox.allow_write,
        vec![std::path::PathBuf::from("/opt")],
        "project allow_write expansion must be dropped to the intersection; got {:?}",
        config.sandbox.allow_write
    );
    assert_has_warning_for(&warnings, "sandbox.allow_write", "C3-02 expansion-dropped");
}

#[test]
fn project_sandbox_allow_write_disjoint_tightens_to_empty() {
    // C3-02: a disjoint project set tightens the writable surface to nothing;
    // the outside path is dropped + warn.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[sandbox]\nenabled = true\nallow_write = [\"/opt\"]\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nallow_write = [\"/var\"]\n",
    )
    .unwrap();

    let base = load_global_base(home.path());
    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let warnings = project_ratchet_warnings(&base, &workspace.path().join(PROJECT_CONFIG_FILE));

    assert!(
        config.sandbox.allow_write.is_empty(),
        "project allow_write disjoint from base must tighten to empty; got {:?}",
        config.sandbox.allow_write
    );
    assert_has_warning_for(&warnings, "sandbox.allow_write", "C3-02 disjoint-empty");
}

// ── C3-03: misspelled [sandbox] fields must be rejected ─────────────────
// `PartialSandboxSettings` lacks `#[serde(deny_unknown_fields)]`, so typos like
// `require` (instead of `required`) or `allow_netork` (instead of
// `allow_network`) are silently ignored — a config that looks stricter than it
// is. Both the partial (layered) path and the direct `AegisConfig`/`SandboxSettings`
// path must reject unknown fields.

#[test]
fn partial_sandbox_rejects_unknown_field_require() {
    // C3-03: `require` is a misspelling of `required`; the project file must
    // fail to load rather than silently ignore the stricter setting.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nrequire = true\n",
    )
    .unwrap();

    let result = AegisConfig::load_for(workspace.path(), Some(home.path()));
    assert!(
        result.is_err(),
        "misspelled `[sandbox] require = true` must be rejected, not silently ignored; got Ok({:?})",
        result.ok()
    );
}

#[test]
fn partial_sandbox_rejects_unknown_field_allow_netork() {
    // C3-03: `allow_netork` is a misspelling of `allow_network`.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nallow_netork = false\n",
    )
    .unwrap();

    let result = AegisConfig::load_for(workspace.path(), Some(home.path()));
    assert!(
        result.is_err(),
        "misspelled `[sandbox] allow_netork = false` must be rejected, not silently ignored; got Ok({:?})",
        result.ok()
    );
}

#[test]
fn direct_sandbox_rejects_unknown_field_require() {
    // C3-03: the direct `AegisConfig`/`SandboxSettings` deserialization path
    // (used by `amend`) must also reject unknown `[sandbox]` fields.
    let toml_src = "[sandbox]\nrequire = true\n";
    let result = toml::from_str::<AegisConfig>(toml_src);
    assert!(
        result.is_err(),
        "misspelled `[sandbox] require = true` must be rejected on the direct AegisConfig path; got Ok({:?})",
        result.ok()
    );
}