use super::*;

// ── C3 ratchet expansion: neighboring un-ratcheted fields ───────────────
// The existing ratchet covers mode, allowlist_override_level, snapshot_policy,
// ci_policy, and sandbox.required. Sibling fields (sandbox.enabled,
// auto_snapshot_*, sandbox.allow_network, sandbox.allow_write) are currently
// last-wins, which lets a project config silently defeat a stricter global
// base. These tests pin the expanded ratchet behavior.

#[test]
fn project_sandbox_enabled_cannot_weaken_global_enabled_true() {
    // F1: a project setting `enabled = false` must NOT disable a globally
    // enabled sandbox — that would bypass the inherited `required` ratchet
    // entirely (runtime builds the sandbox as `enabled.then(...)`).
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[sandbox]\nenabled = true\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nenabled = false\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert!(
        config.sandbox.enabled,
        "project sandbox.enabled must not weaken a globally enabled sandbox; got {:?}",
        config.sandbox.enabled
    );
}

#[test]
fn project_sandbox_enabled_can_tighten_default_false_to_true() {
    // Guard: tightening (default false → project true) must still work after
    // the ratchet expansion is implemented.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nenabled = true\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert!(
        config.sandbox.enabled,
        "project must be able to enable the sandbox when the base leaves it disabled"
    );
}

#[test]
fn project_auto_snapshot_git_cannot_weaken_global_true() {
    // F2: under SnapshotPolicy::Selective each plugin is gated by its
    // auto_snapshot_* flag, so disabling one is equivalent to setting
    // snapshot_policy = "None" (which IS ratcheted). The flag must ratchet too.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_git = true\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "auto_snapshot_git = false\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert!(
        config.auto_snapshot_git,
        "project auto_snapshot_git must not weaken a globally enabled snapshot flag; got {}",
        config.auto_snapshot_git
    );
}

#[test]
fn project_auto_snapshot_docker_cannot_weaken_global_true() {
    // F2: same bypass via the docker snapshot flag.
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
        "auto_snapshot_docker = false\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert!(
        config.auto_snapshot_docker,
        "project auto_snapshot_docker must not weaken a globally enabled snapshot flag; got {}",
        config.auto_snapshot_docker
    );
}

#[test]
fn project_auto_snapshot_git_can_tighten_default_false_to_true() {
    // Guard: tightening (default false → project true) must still work after
    // the ratchet expansion is implemented.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "auto_snapshot_git = true\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert!(
        config.auto_snapshot_git,
        "project must be able to enable git snapshots when the base leaves them disabled"
    );
}

#[test]
fn project_sandbox_allow_network_cannot_weaken_global_false() {
    // F3: `allow_network` is directional — `true` is WEAKER (grants network
    // access). A project enabling network over a global deny must be ratcheted
    // back to the stricter `false`.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[sandbox]\nenabled = true\nallow_network = false\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nallow_network = true\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert!(
        !config.sandbox.allow_network,
        "project allow_network must not weaken a globally denied network access; got {}",
        config.sandbox.allow_network
    );
}

#[test]
fn project_sandbox_allow_network_can_tighten_global_true_to_false() {
    // Guard: tightening (global true → project false) must still work after the
    // ratchet expansion is implemented.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[sandbox]\nenabled = true\nallow_network = true\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nallow_network = false\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert!(
        !config.sandbox.allow_network,
        "project must be able to tighten allow_network from global true to false"
    );
}

#[test]
fn project_sandbox_allow_write_cannot_expand_global_base() {
    // F3: `allow_write` is a Vec<PathBuf> — more entries = weaker. Under the
    // Project layer the ratchet must keep the base set and ignore the project
    // expansion (which could add writable paths the global base did not allow).
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[sandbox]\nenabled = true\nallow_write = [\"/opt/data\"]\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nallow_write = [\"/opt/data\", \"/etc\"]\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(
        config.sandbox.allow_write,
        vec![std::path::PathBuf::from("/opt/data")],
        "project allow_write must not expand the writable set beyond the global base; got {:?}",
        config.sandbox.allow_write
    );
}
