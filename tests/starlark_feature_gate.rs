use std::fs;
use std::path::Path;

fn repo_path(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[test]
fn default_build_does_not_enable_starlark_policy_loader() {
    let cargo_toml = fs::read_to_string(repo_path("Cargo.toml"))
        .expect("Cargo.toml should be readable from repository root");

    assert!(
        cargo_toml.contains("starlark-policy"),
        "Cargo.toml must declare an explicit starlark-policy feature"
    );
    assert!(
        !cargo_toml.contains("default = [\"starlark-policy\"]"),
        "starlark-policy must not be part of default features while its dependency chain has advisories"
    );
}

#[cfg(not(feature = "starlark-policy"))]
#[test]
fn runtime_context_fails_closed_when_policy_star_exists_without_feature() {
    use tempfile::TempDir;

    let home = TempDir::new().expect("temp dir");
    let aegis_dir = home.path().join(".aegis");
    fs::create_dir_all(&aegis_dir).expect("create .aegis dir");
    let policy_path = aegis_dir.join("policy.star");
    fs::write(&policy_path, "# test policy placeholder\n").expect("write policy.star");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let handle = rt.handle().clone();

    let config = aegis::config::AegisConfig::default();
    let result =
        aegis::runtime::RuntimeContext::new_with_policy_path(config, handle, Some(&policy_path));

    let err = match result {
        Ok(_) => panic!(
            "RuntimeContext must fail closed when policy.star exists without starlark-policy feature"
        ),
        Err(e) => e,
    };
    assert!(
        err.to_string()
            .contains("compiled without the starlark-policy feature"),
        "error must mention the missing feature, got: {err}"
    );
}
