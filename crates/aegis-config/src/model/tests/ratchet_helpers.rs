//! Test helpers shared by `ratchet.rs` for the C3 provider-target ratchet
//! coverage. Extracted into a sibling module so `ratchet.rs` stays under the
//! 800-line file-size budget; no `#[test]` bodies live here.

use std::path::Path;

use tempfile::TempDir;

use super::AegisConfig;
use super::ConfigLayerPath;
use super::ConfigSourceLayer;

/// Load only the global config layer (no project file) — used as the trusted
/// `base` for `project_security_ratchet_warnings`.
pub(super) fn load_global_base(home: &Path) -> AegisConfig {
    let scratch = TempDir::new().unwrap();
    AegisConfig::load_for_inspection(scratch.path(), Some(home)).unwrap()
}

/// Compute the ratchet warnings the project layer would trigger against `base`.
pub(super) fn project_ratchet_warnings(base: &AegisConfig, project_path: &Path) -> Vec<String> {
    let layer = ConfigLayerPath {
        source_layer: ConfigSourceLayer::Project,
        path: project_path.to_path_buf(),
    };
    AegisConfig::project_security_ratchet_warnings(base, &layer)
        .map(|warnings| warnings.into_iter().map(|w| w.field.to_string()).collect())
        .unwrap_or_default()
}

pub(super) fn assert_no_warning_for(fields: &[String], expected: &str, msg: &str) {
    assert!(
        !fields.iter().any(|f| f == expected),
        "{msg}: expected NO `{expected}` warning but got {fields:?}"
    );
}

pub(super) fn assert_has_warning_for(fields: &[String], expected: &str, msg: &str) {
    assert!(
        fields.iter().any(|f| f == expected),
        "{msg}: expected a `{expected}` warning but got {fields:?}"
    );
}
