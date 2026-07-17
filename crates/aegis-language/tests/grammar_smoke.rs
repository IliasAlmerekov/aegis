//! Grammar manifest contract tests — Iteration 0 RED #1 (ADR-022 §8/§9).
//!
//! These assert the qualification contract for a release grammar: it must be
//! version-pinned, carry license evidence, declare a Tree-sitter ABI supported
//! by the pinned runtime, and belong to the L1 release-grammar set. The
//! expected rejections and the qualified set come from ADR-022 — an
//! independent source of truth — not from the validator's own logic, so the
//! tests cannot pass by construction.

use aegis_language::manifest::{
    GrammarEntry, ManifestError, RELEASE_GRAMMARS, SUPPORTED_LANGUAGE_ABI, validate_entry,
};

/// A grammar entry that satisfies every contract clause except the single
/// field a given test mutates.
fn well_formed(language: &'static str) -> GrammarEntry {
    GrammarEntry {
        language,
        crate_name: "tree-sitter-fixture",
        version: "0.1.0",
        upstream: "https://example.invalid/tree-sitter-fixture",
        license_spdx: "MIT",
        abi: SUPPORTED_LANGUAGE_ABI,
    }
}

#[test]
fn rejects_an_unpinned_grammar_with_empty_version() {
    let mut entry = well_formed("python");
    entry.version = "";
    assert_eq!(
        validate_entry(&entry, RELEASE_GRAMMARS),
        Err(ManifestError::Unpinned { language: "python" })
    );
}

#[test]
fn rejects_a_wildcard_version_as_unpinned() {
    let mut entry = well_formed("python");
    entry.version = "*";
    assert_eq!(
        validate_entry(&entry, RELEASE_GRAMMARS),
        Err(ManifestError::Unpinned { language: "python" })
    );
}

#[test]
fn rejects_a_grammar_missing_license_evidence() {
    let mut entry = well_formed("python");
    entry.license_spdx = "";
    assert_eq!(
        validate_entry(&entry, RELEASE_GRAMMARS),
        Err(ManifestError::MissingLicense { language: "python" })
    );
}

#[test]
fn rejects_a_grammar_with_unsupported_tree_sitter_abi() {
    let mut entry = well_formed("python");
    entry.abi = 99;
    assert_eq!(
        validate_entry(&entry, RELEASE_GRAMMARS),
        Err(ManifestError::UnsupportedAbi {
            language: "python",
            abi: 99
        })
    );
}

#[test]
fn accepts_a_grammar_at_a_backwards_compatible_abi() {
    // Tree-sitter runtime 0.26 speaks LANGUAGE_VERSION 15 but remains
    // backwards-compatible down to MIN_COMPATIBLE_LANGUAGE_VERSION 13. A
    // grammar generated for ABI 14 (e.g. the pinned tree-sitter-typescript
    // 0.23.2) is therefore qualified, not rejected.
    use aegis_language::manifest::MIN_SUPPORTED_LANGUAGE_ABI;
    let mut entry = well_formed("python");
    entry.abi = MIN_SUPPORTED_LANGUAGE_ABI + 1;
    assert_eq!(
        validate_entry(&entry, RELEASE_GRAMMARS),
        Ok(()),
        "a grammar at a backwards-compatible ABI must pass the contract"
    );
}

#[test]
fn rejects_a_grammar_below_the_minimum_compatible_abi() {
    use aegis_language::manifest::MIN_SUPPORTED_LANGUAGE_ABI;
    let mut entry = well_formed("python");
    entry.abi = MIN_SUPPORTED_LANGUAGE_ABI - 1;
    assert_eq!(
        validate_entry(&entry, RELEASE_GRAMMARS),
        Err(ManifestError::UnsupportedAbi {
            language: "python",
            abi: MIN_SUPPORTED_LANGUAGE_ABI - 1
        })
    );
}

#[test]
fn rejects_a_grammar_absent_from_the_release_manifest() {
    // Ruby is a staged 1.x adapter (ADR-022 §9), not part of the L1 release
    // set, so a compiled-in Ruby grammar must be rejected.
    let entry = well_formed("ruby");
    assert_eq!(
        validate_entry(&entry, RELEASE_GRAMMARS),
        Err(ManifestError::AbsentFromRelease { language: "ruby" })
    );
}

#[test]
fn accepts_every_well_formed_release_grammar() {
    for &language in RELEASE_GRAMMARS {
        let entry = well_formed(language);
        assert_eq!(
            validate_entry(&entry, RELEASE_GRAMMARS),
            Ok(()),
            "release grammar `{language}` should pass the qualification contract"
        );
    }
}

#[test]
fn release_grammar_set_matches_adr022_l1_foundation() {
    // ADR-022 §9: the L1 foundation ships Python, JavaScript, TypeScript, and
    // Shell/Bash. Go, PHP, Ruby, PowerShell, Perl, and Lua are 1.x adapters.
    assert_eq!(
        RELEASE_GRAMMARS,
        &["python", "javascript", "typescript", "bash"]
    );
}

#[test]
fn release_target_set_matches_adr022_release_matrix() {
    // ADR-022 §8: the qualified grammar set must be identical across these
    // four official release targets. The CI cross-compile matrix (Iteration 0
    // RED #2b) builds `aegis-language` for each and asserts the four grammars
    // link; this test locks the declared target set against silent drift.
    use aegis_language::manifest::RELEASE_TARGETS;
    assert_eq!(
        RELEASE_TARGETS,
        &[
            "x86_64-unknown-linux-musl",
            "aarch64-unknown-linux-musl",
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
        ]
    );
}

// ── Iteration 0 RED #2a: host build + grammar smoke ─────────────────────────
//
// ADR-022 §8 requires the four foundation adapters to be statically present in
// the release binary. These tests prove the wired grammars actually parse on
// the host build and that the built-in manifest's metadata matches the live
// Tree-sitter runtime rather than a hand-asserted constant.

use aegis_language::{SourceLanguage, parse};

/// A trivial well-formed snippet per release grammar, with the root node kind
// the corresponding grammar assigns to a complete top-level statement. These
// expected kinds come from each grammar's published `node-types.json`, an
// independent source of truth — not from the parser's own output.
#[test]
fn each_release_grammar_parses_a_trivial_snippet_on_host() {
    let cases = [
        // The expected root kinds are each grammar's published document node
        // (from node-types.json), an independent source of truth: asserting
        // them proves the right grammar is wired to the right language id.
        (SourceLanguage::Python, "x = 1\n", "module"),
        (SourceLanguage::JavaScript, "let x = 1;\n", "program"),
        (
            SourceLanguage::TypeScript,
            "let x: number = 1;\n",
            "program",
        ),
        (SourceLanguage::Bash, "echo hi\n", "program"),
    ];
    for (lang, source, expected_root) in cases {
        let tree = parse(lang, source)
            .unwrap_or_else(|err| panic!("parse({lang:?}, {source:?}) failed: {err}"));
        let root = tree.root_node();
        assert!(
            !root.has_error(),
            "release grammar `{lang:?}` produced an error tree for {source:?}"
        );
        assert_eq!(
            root.kind(),
            expected_root,
            "release grammar `{lang:?}` root kind for {source:?}"
        );
    }
}

#[test]
fn builtin_manifest_satisfies_the_release_contract() {
    use aegis_language::manifest::{BUILTIN_MANIFEST, validate_manifest};
    validate_manifest(BUILTIN_MANIFEST, RELEASE_GRAMMARS)
        .expect("the built-in release manifest must satisfy the qualification contract");
}

#[test]
fn builtin_manifest_abi_matches_the_live_tree_sitter_runtime() {
    // The manifest's recorded ABI must equal each wired grammar's actual
    // runtime ABI (tree_sitter::Language::abi_version), proving the manifest
    // is not a stale hand-asserted constant.
    use aegis_language::manifest::BUILTIN_MANIFEST;

    for entry in BUILTIN_MANIFEST {
        let lang = SourceLanguage::from_id(entry.language).unwrap_or_else(|| {
            panic!(
                "manifest language `{}` is not a wired SourceLanguage",
                entry.language
            )
        });
        let runtime_abi = lang.tree_sitter_language().abi_version();
        assert_eq!(
            entry.abi, runtime_abi as u32,
            "manifest ABI for `{}` ({}) must match the live grammar ABI ({})",
            entry.language, entry.abi, runtime_abi,
        );
    }
}

// ── Iteration 0 re-review: manifest pin + provenance completeness ─────────────
//
// ADR-022 §8 requires each grammar to be a *pinned* version. `validate_entry`
// only rejects empty/`*` versions — it cannot tell a caret requirement from an
// exact pin. The authoritative pin is `Cargo.lock`, which `cargo-deny` keeps
// duplicate-free. These two tests close that gap against the real lockfile and
// enforce that the plan-mandated provenance inventory (ADR-022 §8 inventory
// list) is fully populated, so the `crate_name`/`upstream` fields are not inert.

use std::fs;

/// Read the workspace `Cargo.lock`. The test crate's `CARGO_MANIFEST_DIR` is
/// `crates/aegis-language`, so the lockfile is two directories up.
fn cargo_lock() -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

/// Return every `version` Cargo.lock records for a package `name`. A
/// well-formed lockfile has exactly one (cargo-deny bans duplicates); returning
/// all of them lets the pin test assert the manifest version is the locked one
/// and the uniqueness assertion catch a future duplicate.
fn locked_versions(name: &str) -> Vec<String> {
    let mut versions = Vec::new();
    for block in cargo_lock().split("\n[[package]]") {
        // The package name is the first `name = "..."` line in the block.
        let Some(block_name) = block
            .lines()
            .find_map(|l| l.strip_prefix("name = ").map(str::trim))
            .map(|n| n.trim_matches('"'))
        else {
            continue;
        };
        if block_name != name {
            continue;
        }
        if let Some(version_line) = block.lines().find(|l| l.starts_with("version = ")) {
            let v = version_line
                .trim_start_matches("version = ")
                .trim_matches('"')
                .to_owned();
            versions.push(v);
        }
    }
    versions
}

#[test]
fn builtin_manifest_versions_match_cargo_lock_pins() {
    use aegis_language::manifest::BUILTIN_MANIFEST;

    for entry in BUILTIN_MANIFEST {
        let locked = locked_versions(entry.crate_name);
        assert!(
            !locked.is_empty(),
            "Cargo.lock has no `[[package]]` entry for `{}` — the grammar crate is \
             not actually pinned in the lockfile (ADR-022 §8)",
            entry.crate_name,
        );
        assert_eq!(
            locked.len(),
            1,
            "Cargo.lock pins `{}` to {} versions ({:?}); cargo-deny should ban the \
             duplicate — the manifest records a single pin (ADR-022 §8)",
            entry.crate_name,
            locked.len(),
            locked,
        );
        assert_eq!(
            locked[0], entry.version,
            "manifest version `{}` for `{}` does not match the Cargo.lock pin `{}` — \
             the manifest must reflect the real lockfile pin, not a caret range",
            entry.version, entry.crate_name, locked[0],
        );
    }
}

#[test]
fn builtin_manifest_provenance_is_complete() {
    use aegis_language::manifest::BUILTIN_MANIFEST;

    for entry in BUILTIN_MANIFEST {
        assert!(
            !entry.crate_name.is_empty(),
            "grammar `{}` has an empty crate_name",
            entry.language,
        );
        assert!(
            entry.crate_name.starts_with("tree-sitter-"),
            "grammar `{}` crate_name `{}` must be a `tree-sitter-*` grammar crate",
            entry.language,
            entry.crate_name,
        );
        assert!(
            !entry.version.is_empty(),
            "grammar `{}` has an empty version",
            entry.language,
        );
        assert!(
            entry.upstream.starts_with("https://"),
            "grammar `{}` upstream `{}` must be an https URL (ADR-022 §8 inventory)",
            entry.language,
            entry.upstream,
        );
        assert!(
            !entry.license_spdx.is_empty(),
            "grammar `{}` has no SPDX license evidence",
            entry.language,
        );
        assert!(
            RELEASE_GRAMMARS.contains(&entry.language),
            "grammar `{}` is not in the release set {:?}",
            entry.language,
            RELEASE_GRAMMARS,
        );
    }
}
