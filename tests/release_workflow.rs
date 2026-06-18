//! Red tests for M3.2 — static musl release targets.
//!
//! These tests encode the release-workflow target matrix contract. The current
//! `.github/workflows/release.yml` uses GNU targets and has no static-binary
//! verification step, so the migration tests are expected to FAIL until the
//! workflow is migrated to musl targets. The asset-name test is a preservation
//! invariant (already green) and must stay green across the migration.

use std::path::Path;

fn release_workflow() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml");
    std::fs::read_to_string(&path).expect("release workflow should be readable")
}

/// Extracts the single matrix `include:` entry for `target` from the workflow
/// text. The entry spans from its `- target: <triple>` marker up to the next
/// `- target: ` marker (or end of file), so callers can assert on per-target
/// fields like `use_cross` without a YAML dependency.
///
/// Panics if the target is absent — this is a test-fixture failure, not a
/// runtime failure, so `panic!`/`expect` is acceptable here.
fn matrix_entry(workflow: &str, target: &str) -> String {
    for segment in workflow.split("- target: ").skip(1) {
        if let Some(rest) = segment.strip_prefix(target) {
            // `rest` ends where the next `- target: ` marker began, so it is
            // exactly this entry's body (plus a trailing newline).
            return format!("- target: {target}{rest}");
        }
    }
    panic!("release workflow matrix should define target {target}");
}

#[test]
fn release_workflow_should_build_linux_musl_targets() {
    let wf = release_workflow();
    assert!(
        wf.contains("x86_64-unknown-linux-musl"),
        "release workflow must build x86_64-unknown-linux-musl"
    );
    assert!(
        wf.contains("aarch64-unknown-linux-musl"),
        "release workflow must build aarch64-unknown-linux-musl"
    );
}

#[test]
fn release_workflow_should_not_build_linux_gnu_targets() {
    let wf = release_workflow();
    assert!(
        !wf.contains("x86_64-unknown-linux-gnu"),
        "release workflow must not build x86_64-unknown-linux-gnu"
    );
    assert!(
        !wf.contains("aarch64-unknown-linux-gnu"),
        "release workflow must not build aarch64-unknown-linux-gnu"
    );
}

#[test]
fn release_workflow_should_keep_installer_asset_names() {
    let wf = release_workflow();
    assert!(
        wf.contains("aegis-linux-x86_64"),
        "release workflow must keep aegis-linux-x86_64 asset name"
    );
    assert!(
        wf.contains("aegis-linux-aarch64"),
        "release workflow must keep aegis-linux-aarch64 asset name"
    );
}

#[test]
fn release_workflow_should_verify_static_linux_binaries() {
    let wf = release_workflow();
    assert!(
        wf.contains("Verify static Linux binary"),
        "release workflow must include a 'Verify static Linux binary' step"
    );
    assert!(
        wf.contains("unknown-linux-musl"),
        "release workflow must reference unknown-linux-musl in verification"
    );
    assert!(
        wf.contains("ldd"),
        "release workflow must invoke ldd to verify static linkage"
    );
    assert!(
        wf.contains("not a dynamic executable"),
        "release workflow must assert 'not a dynamic executable' output from ldd"
    );
}

#[test]
fn release_workflow_should_build_linux_musl_targets_via_cross() {
    let wf = release_workflow();

    for target in ["x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"] {
        let entry = matrix_entry(&wf, target);
        assert!(
            entry.contains("use_cross: true"),
            "release workflow must build {target} via cross (use_cross: true); matrix entry:\n{entry}"
        );
    }
}
