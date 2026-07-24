use std::fs;

use super::ratchet_helpers::{
    assert_has_warning_for, assert_no_warning_for, load_global_base, project_ratchet_warnings,
};
use super::*;

#[test]
fn language_analysis_defaults_to_256_kib_limit_and_no_trusted_aliases() {
    let config = AegisConfig::defaults();

    assert_eq!(
        config.language_analysis.inline_source_limit_bytes,
        16 * 1024
    );
    assert_eq!(config.language_analysis.script_file_limit_bytes, 256 * 1024);
    assert_eq!(config.language_analysis.max_script_files, 8);
    assert_eq!(config.language_analysis.max_depth, 8);
    assert_eq!(config.language_analysis.max_targets, 16);
    assert_eq!(config.language_analysis.max_aggregate_bytes, 1024 * 1024);
    assert_eq!(config.language_analysis.timeout_ms, 100);
    assert_eq!(config.language_analysis.trusted_aliases, Vec::new());
}

#[test]
fn language_analysis_budgets_are_globally_bounded_and_project_tightenable() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[language_analysis]\ninline_source_limit_bytes = 999999\n\
         max_script_files = 99\nmax_depth = 99\n\
         max_targets = 99\nmax_aggregate_bytes = 9999999\ntimeout_ms = 9999\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[language_analysis]\ninline_source_limit_bytes = 1024\n\
         max_script_files = 2\nmax_depth = 3\n\
         max_targets = 4\nmax_aggregate_bytes = 4096\ntimeout_ms = 25\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(config.language_analysis.inline_source_limit_bytes, 1024);
    assert_eq!(config.language_analysis.max_script_files, 2);
    assert_eq!(config.language_analysis.max_depth, 3);
    assert_eq!(config.language_analysis.max_targets, 4);
    assert_eq!(config.language_analysis.max_aggregate_bytes, 4096);
    assert_eq!(config.language_analysis.timeout_ms, 25);
}

#[test]
fn project_layer_can_lower_the_script_file_limit() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[language_analysis]\nscript_file_limit_bytes = 65536\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(config.language_analysis.script_file_limit_bytes, 65536);
}

#[test]
fn project_layer_cannot_raise_the_script_file_limit_above_the_global_value() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[language_analysis]\nscript_file_limit_bytes = 65536\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[language_analysis]\nscript_file_limit_bytes = 999999\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(
        config.language_analysis.script_file_limit_bytes, 65536,
        "project must not be able to raise the script-file limit above the trusted global value"
    );
}

#[test]
fn global_layer_cannot_raise_the_script_file_limit_above_the_hard_ceiling() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[language_analysis]\nscript_file_limit_bytes = 5000000\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(
        config.language_analysis.script_file_limit_bytes,
        LANGUAGE_ANALYSIS_SCRIPT_FILE_HARD_CEILING_BYTES,
        "the 1 MiB hard ceiling is non-configurable, even at the trusted global layer"
    );
}

#[test]
fn global_layer_can_set_trusted_aliases() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[[language_analysis.trusted_aliases]]\nalias = \"py\"\ncanonical = \"python3\"\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(
        config.language_analysis.trusted_aliases,
        vec![TrustedAlias {
            alias: "py".to_string(),
            canonical: "python3".to_string(),
        }]
    );
}

#[test]
fn project_layer_trusted_alias_entries_are_dropped_entirely() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[[language_analysis.trusted_aliases]]\nalias = \"py\"\ncanonical = \"python3\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[[language_analysis.trusted_aliases]]\nalias = \"node-wrapper\"\ncanonical = \"node\"\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(
        config.language_analysis.trusted_aliases,
        vec![TrustedAlias {
            alias: "py".to_string(),
            canonical: "python3".to_string(),
        }],
        "a project-layer trusted alias must never be added — only the trusted \
         global alias survives"
    );
}

#[test]
fn project_layer_raising_the_script_file_limit_surfaces_a_ratchet_warning() {
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[language_analysis]\nscript_file_limit_bytes = 65536\n",
    )
    .unwrap();
    let base = load_global_base(home.path());

    let project = TempDir::new().unwrap();
    let project_path = project.path().join(PROJECT_CONFIG_FILE);
    fs::write(
        &project_path,
        "[language_analysis]\nscript_file_limit_bytes = 999999\n",
    )
    .unwrap();

    let warnings = project_ratchet_warnings(&base, &project_path);
    assert_has_warning_for(
        &warnings,
        "language_analysis.script_file_limit_bytes",
        "raising the script-file limit above the trusted global value",
    );
}

#[test]
fn project_layer_trusted_alias_attempt_surfaces_a_ratchet_warning() {
    let home = TempDir::new().unwrap();
    let base = load_global_base(home.path());

    let project = TempDir::new().unwrap();
    let project_path = project.path().join(PROJECT_CONFIG_FILE);
    fs::write(
        &project_path,
        "[[language_analysis.trusted_aliases]]\nalias = \"py\"\ncanonical = \"python3\"\n",
    )
    .unwrap();

    let warnings = project_ratchet_warnings(&base, &project_path);
    assert_has_warning_for(
        &warnings,
        "language_analysis.trusted_aliases",
        "a project-layer trusted alias attempt",
    );
}

#[test]
fn trusted_alias_with_an_empty_alias_field_is_rejected() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[[language_analysis.trusted_aliases]]\nalias = \"\"\ncanonical = \"python3\"\n",
    )
    .unwrap();

    let result = AegisConfig::load_for(workspace.path(), Some(home.path()));

    assert!(
        result.is_err(),
        "an empty trusted-alias alias field must be rejected"
    );
}

#[test]
fn trusted_alias_that_maps_a_program_to_itself_is_rejected() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[[language_analysis.trusted_aliases]]\nalias = \"python3\"\ncanonical = \"python3\"\n",
    )
    .unwrap();

    let result = AegisConfig::load_for(workspace.path(), Some(home.path()));

    assert!(
        result.is_err(),
        "a trusted alias mapping a program to itself must be rejected"
    );
}

#[test]
fn duplicate_trusted_alias_entries_are_rejected() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[[language_analysis.trusted_aliases]]\nalias = \"py\"\ncanonical = \"python3\"\n\
         [[language_analysis.trusted_aliases]]\nalias = \"py\"\ncanonical = \"bash\"\n",
    )
    .unwrap();

    let result = AegisConfig::load_for(workspace.path(), Some(home.path()));

    assert!(
        result.is_err(),
        "two trusted-alias entries with the same alias must be rejected"
    );
}

#[test]
fn project_layer_repeating_the_identical_trusted_global_alias_set_does_not_warn() {
    // The project's request is dropped regardless (kept = base, always) — but
    // when it happens to match the trusted global set exactly, `kept ==
    // requested` and the shared `push_ratchet_warning` guard correctly stays
    // quiet, same as every other ratchet field.
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[[language_analysis.trusted_aliases]]\nalias = \"py\"\ncanonical = \"python3\"\n",
    )
    .unwrap();
    let base = load_global_base(home.path());

    let project = TempDir::new().unwrap();
    let project_path = project.path().join(PROJECT_CONFIG_FILE);
    fs::write(
        &project_path,
        "[[language_analysis.trusted_aliases]]\nalias = \"py\"\ncanonical = \"python3\"\n",
    )
    .unwrap();

    let warnings = project_ratchet_warnings(&base, &project_path);
    assert_no_warning_for(
        &warnings,
        "language_analysis.trusted_aliases",
        "repeating the identical trusted global alias set",
    );
}

#[test]
fn language_analysis_round_trips_through_toml_serialization() {
    let mut config = AegisConfig::defaults();
    config.language_analysis.script_file_limit_bytes = 131072;
    config.language_analysis.trusted_aliases.push(TrustedAlias {
        alias: "py".to_string(),
        canonical: "python3".to_string(),
    });

    let toml = config.to_toml_string().unwrap();

    assert!(
        toml.contains("[language_analysis]"),
        "serialized config must include a [language_analysis] section: {toml}"
    );
    assert!(
        toml.contains("script_file_limit_bytes = 131072"),
        "serialized script-file limit must round-trip: {toml}"
    );
    assert!(
        toml.contains("alias = \"py\""),
        "serialized trusted alias must round-trip: {toml}"
    );
}

#[test]
fn project_layer_raising_analysis_budgets_surfaces_ratchet_warnings() {
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[language_analysis]\ninline_source_limit_bytes = 1024\n\
         max_script_files = 2\nmax_depth = 3\nmax_targets = 4\n\
         max_aggregate_bytes = 4096\ntimeout_ms = 25\n",
    )
    .unwrap();
    let base = load_global_base(home.path());
    let project = TempDir::new().unwrap();
    let project_path = project.path().join(PROJECT_CONFIG_FILE);
    fs::write(
        &project_path,
        "[language_analysis]\ninline_source_limit_bytes = 16384\n\
         max_script_files = 8\nmax_depth = 8\nmax_targets = 16\n\
         max_aggregate_bytes = 1048576\ntimeout_ms = 100\n",
    )
    .unwrap();

    let warnings = project_ratchet_warnings(&base, &project_path);
    for field in [
        "language_analysis.inline_source_limit_bytes",
        "language_analysis.max_script_files",
        "language_analysis.max_depth",
        "language_analysis.max_targets",
        "language_analysis.max_aggregate_bytes",
        "language_analysis.timeout_ms",
    ] {
        assert_has_warning_for(&warnings, field, "raising an analysis budget");
    }
}

#[test]
fn language_analysis_enabled_switch_is_rejected() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[language_analysis]\nenabled = false\n",
    )
    .unwrap();

    let result = AegisConfig::load_for(workspace.path(), Some(home.path()));

    assert!(
        result.is_err(),
        "there is deliberately no project or global language_analysis.enabled switch"
    );
}
