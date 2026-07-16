use std::fs;
use std::path::PathBuf;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn config_schema_contract_covers_exit_code_compatibility() {
    let path = repo_path("docs/config-schema.md");
    let contents = fs::read_to_string(&path).expect("docs/config-schema.md must exist");

    assert!(
        contents.contains("## Exit-code compatibility contract"),
        "config schema doc must document exit-code compatibility contract"
    );

    for needle in [
        "`0` — command approved/executed successfully",
        "`2` — user denied in a prompt path (`prompt` decision)",
        "`3` — hard block (`block` decision)",
        "`4` — internal/config error",
        "`exit_code` in `--output json` always matches",
        "1..=255",
    ] {
        assert!(
            contents.contains(needle),
            "config schema doc must mention `{needle}`; missing compatibility contract detail"
        );
    }
}

#[test]
fn threat_model_is_current_and_documents_non_goals_honestly() {
    let path = repo_path("docs/threat-model.md");
    let contents = fs::read_to_string(&path).expect("docs/threat-model.md must exist");

    for needle in [
        "Aegis is a **heuristic command guardrail**",
        "Aegis is **not**:",
        "Aegis does not aim to provide:",
        "Residual risk",
        "Known examples:",
        "Security invariants",
        "Explicit non-goals",
        "Verification maturity note",
        "Current fuzzing coverage includes parser and scanner harnesses",
    ] {
        assert!(
            contents.contains(needle),
            "threat-model doc must keep current scope-and-limit language: {needle}"
        );
    }
}

#[test]
fn h9_public_docs_distinguish_required_recovery_from_best_effort_snapshots() {
    let threat_model = fs::read_to_string(repo_path("docs/threat-model.md")).unwrap();
    let config_schema = fs::read_to_string(repo_path("docs/config-schema.md")).unwrap();
    let readme = fs::read_to_string(repo_path("README.md")).unwrap();

    for needle in [
        "Effect-opaque execution",
        "Required recovery",
        "one-time Recovery override",
        "Mode::Audit",
        "SnapshotPolicy::None",
    ] {
        assert!(
            threat_model.contains(needle),
            "threat model must document H9 term `{needle}`"
        );
    }
    for needle in [
        "Effect-opaque execution",
        "Required recovery",
        "no applicable Snapshot plugin",
        "Run once without recovery",
    ] {
        assert!(
            config_schema.contains(needle),
            "config schema must document H9 term `{needle}`"
        );
    }
    for needle in [
        "Effect-opaque execution",
        "Run once without recovery",
        "does not inspect the referenced script",
    ] {
        assert!(readme.contains(needle), "README must mention `{needle}`");
    }
    for stale in [
        "when snapshots are requested, that matters only for `Danger`",
        "Snapshot requests matter only for `Danger` flows.",
        "if there are no applicable snapshot plugins, no snapshots are requested even for `Danger`",
    ] {
        assert!(
            !config_schema.contains(stale),
            "config schema must remove stale snapshot claim `{stale}`"
        );
    }
}

#[test]
fn readme_links_to_contract_docs() {
    let readme = fs::read_to_string(repo_path("README.md")).expect("README.md must exist");
    assert!(
        readme.contains("[Architecture decisions](docs/adr/README.md)"),
        "README must link to ADR index document"
    );
    assert!(
        readme.contains("[Config schema](docs/config-schema.md)"),
        "README must link to config schema contract document"
    );
    assert!(
        readme.contains("[Threat model](docs/threat-model.md)"),
        "README must link to threat model contract document"
    );
    assert!(
        readme.contains("[Release readiness](docs/release-readiness.md)"),
        "README must link to release-readiness contract document"
    );
    for needle in [
        "command -v aegis",
        "aegis --version",
        "SHELL",
        "AEGIS_REAL_SHELL",
        "find the `shell` field",
        "curl -fsSL",
        "install.sh",
        "Global",
        "Local",
        "Binary",
        "Claude Code",
        "Aegis is working",
        "Uninstall",
        "uninstall.sh",
    ] {
        assert!(readme.contains(needle), "README must mention `{needle}`");
    }
}

#[test]
fn adr_index_split_is_present_and_active_docs_reference_it() {
    let adr_index =
        fs::read_to_string(repo_path("docs/adr/README.md")).expect("docs/adr/README.md must exist");

    for needle in [
        "## Current architecture snapshot",
        "## ADR index",
        "## Verification guidance",
        "ADR-001",
        "ADR-010",
        "adr-010-full-shell-evaluation-and-deferred-execution-remain-non-goals.md",
    ] {
        assert!(
            adr_index.contains(needle),
            "ADR index must include `{needle}`"
        );
    }

    let architecture =
        fs::read_to_string(repo_path("ARCHITECTURE.md")).expect("ARCHITECTURE.md must exist");
    assert!(
        architecture.contains("docs/adr/README.md"),
        "ARCHITECTURE.md must point readers at the ADR index"
    );

    let contributing =
        fs::read_to_string(repo_path("CONTRIBUTING.md")).expect("CONTRIBUTING.md must exist");
    assert!(
        contributing.contains("docs/adr/README.md"),
        "CONTRIBUTING.md must point contributors at the ADR index"
    );

    let threat_model = fs::read_to_string(repo_path("docs/threat-model.md"))
        .expect("docs/threat-model.md must exist");
    assert!(
        threat_model.contains(
            "docs/adr/adr-010-full-shell-evaluation-and-deferred-execution-remain-non-goals.md"
        ),
        "threat-model doc must link to the ADR-010 non-goals record"
    );
}

#[test]
fn release_readiness_doc_separates_launch_and_security_checklists() {
    let path = repo_path("docs/release-readiness.md");
    let contents = fs::read_to_string(&path).expect("docs/release-readiness.md must exist");

    for needle in [
        "## Minimum Launch Checklist",
        "## Security-Grade Checklist",
        "## Verification-first manual install path",
        "sha256sum -c <asset-name>.sha256",
        "shasum -a 256 -c <asset-name>.sha256",
        "This verifies the downloaded binary against the checksum sidecar published",
        "It proves integrity of the file you downloaded",
        "does **not** authenticate the publisher",
        "signature /",
        "make the binary available on your `PATH`",
        "asset=aegis-linux-x86_64",
        "chmod +x \"./$asset\"",
        "mv \"./$asset\" \"$HOME/.local/bin/aegis\"",
        "Replace `aegis-linux-x86_64` with your platform asset name",
        "export PATH=\"$HOME/.local/bin:$PATH\"",
        "Claude Code: run `command -v aegis`, then paste the absolute path it",
        "shell-based launchers that honor `$SHELL`",
        "SHELL=/absolute/path/to/aegis",
        "AEGIS_REAL_SHELL=/absolute/path/to/your-real-shell",
        "integrity_mode = \"ChainSha256\"",
        "aegis audit --verify-integrity",
    ] {
        assert!(
            contents.contains(needle),
            "release-readiness doc must include `{needle}`"
        );
    }
}

#[test]
fn config_schema_recommends_chain_sha256_for_security_conscious_deployments() {
    let path = repo_path("docs/config-schema.md");
    let contents = fs::read_to_string(&path).expect("docs/config-schema.md must exist");

    for needle in [
        "## Audit integrity mode",
        "integrity_mode = \"Off\"",
        "integrity_mode = \"ChainSha256\"",
        "aegis audit --verify-integrity",
    ] {
        assert!(
            contents.contains(needle),
            "config schema doc must include `{needle}`"
        );
    }
}

#[test]
fn audit_integrity_docs_match_the_chain_sha256_runtime_default() {
    for path in ["docs/config-schema.md", "docs/release-readiness.md"] {
        let contents = fs::read_to_string(repo_path(path)).expect("audit integrity doc must exist");
        assert!(
            contents.contains("The runtime default is `ChainSha256`"),
            "{path} must state the ChainSha256 runtime default"
        );
    }
}

#[test]
fn troubleshooting_covers_manual_checksum_and_integrity_verification() {
    let path = repo_path("docs/troubleshooting.md");
    let contents = fs::read_to_string(&path).expect("docs/troubleshooting.md must exist");

    for needle in [
        "Manual checksum verification fails",
        "sha256sum -c <asset-name>.sha256",
        "shasum -a 256 -c <asset-name>.sha256",
        "Audit integrity verification",
        "aegis audit --verify-integrity",
    ] {
        assert!(
            contents.contains(needle),
            "troubleshooting doc must include `{needle}`"
        );
    }
}

#[test]
fn docs_should_document_explicit_shell_proxy_setup() {
    let readme = fs::read_to_string(repo_path("README.md")).expect("README.md must exist");

    assert!(
        readme.contains("aegis setup-shell"),
        "README must document the explicit `aegis setup-shell` opt-in command"
    );
    assert!(
        readme.contains("aegis setup-shell --remove"),
        "README must document how to undo shell-proxy setup with `aegis setup-shell --remove`"
    );
}
