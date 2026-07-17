//! Release grammar manifest and qualification contract (ADR-022 §8).
//!
//! Official release binaries statically link every production-qualified grammar
//! and ship a manifest of versions, provenance, and licenses. This module owns
//! that contract: it defines the qualified release-grammar set, the supported
//! Tree-sitter runtime ABI, and a validator that rejects a grammar that is
//! unpinned, unlicensed, ABI-incompatible, or absent from the release
//! manifest. The authoritative inputs are ADR-022 §8 (static linking +
//! manifest) and §9 (the L1 foundation language set).

/// The production-qualified grammars statically linked into official release
/// binaries for the L1 foundation milestone (ADR-022 §9): Python, JavaScript,
/// TypeScript, and Shell/Bash.
///
/// Go, PHP, Ruby, PowerShell, Perl, and Lua are staged 1.x adapters and must
/// not appear here until each passes its independent qualification gate.
pub const RELEASE_GRAMMARS: &[&str] = &["python", "javascript", "typescript", "bash"];

/// The Tree-sitter language ABI version of the pinned runtime.
///
/// This is the *newest* ABI the runtime speaks (`tree_sitter::LANGUAGE_VERSION`,
/// 15 for the pinned 0.26 runtime). A grammar generated for this ABI is always
/// accepted; a grammar generated for an older ABI down to
/// [`MIN_SUPPORTED_LANGUAGE_ABI`] is accepted as backwards-compatible.
pub const SUPPORTED_LANGUAGE_ABI: u32 = tree_sitter::LANGUAGE_VERSION as u32;

/// The oldest Tree-sitter ABI the pinned runtime still accepts
/// (`tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION`, 13 for the 0.26 runtime).
///
/// A grammar below this ABI is rejected by [`validate_entry`] as incompatible,
/// matching the runtime's own `set_language` contract rather than requiring
/// every grammar to be regenerated for the exact current ABI.
pub const MIN_SUPPORTED_LANGUAGE_ABI: u32 = tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION as u32;

/// The official release targets across which the qualified grammar set must be
/// identical (ADR-022 §8). Official release binaries statically link every
/// qualified grammar for each of these four targets; the CI cross-compile
/// matrix builds `aegis-language` for each and asserts the four foundation
/// grammars link.
pub const RELEASE_TARGETS: &[&str] = &[
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

/// One grammar provenance entry in the release grammar manifest (ADR-022 §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarEntry {
    /// Canonical language identifier matching an element of
    /// [`RELEASE_GRAMMARS`].
    pub language: &'static str,
    /// crates.io grammar crate, e.g. `tree-sitter-python`.
    pub crate_name: &'static str,
    /// Pinned SemVer recorded in `Cargo.lock`. Empty or `*` is rejected as
    /// unpinned.
    pub version: &'static str,
    /// Upstream grammar repository URL.
    pub upstream: &'static str,
    /// SPDX license expression covering the grammar's bundled native source.
    pub license_spdx: &'static str,
    /// Tree-sitter language ABI version the grammar was generated for.
    pub abi: u32,
}

/// A grammar-manifest qualification failure (ADR-022 §8).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    /// The grammar has no pinned version (empty or `*`).
    #[error("grammar `{language}` has no pinned version")]
    Unpinned { language: &'static str },

    /// The grammar carries no SPDX license evidence.
    #[error("grammar `{language}` is missing license evidence")]
    MissingLicense { language: &'static str },

    /// The grammar declares a Tree-sitter ABI this runtime does not support.
    #[error("grammar `{language}` declares unsupported Tree-sitter ABI {abi}")]
    UnsupportedAbi { language: &'static str, abi: u32 },

    /// The grammar is compiled in but is not in the qualified release set.
    #[error("grammar `{language}` is absent from the release manifest")]
    AbsentFromRelease { language: &'static str },
}

/// Validate a single grammar entry against the release contract.
///
/// `release_grammars` is the authoritative qualified set (normally
/// [`RELEASE_GRAMMARS`]). Returns the first qualification failure, or `Ok`
/// when the entry is pinned, licensed, ABI-compatible, and part of the
/// release set.
pub fn validate_entry(
    entry: &GrammarEntry,
    release_grammars: &[&str],
) -> Result<(), ManifestError> {
    // A grammar is only eligible once it carries a concrete pinned version,
    // license evidence, and a Tree-sitter ABI the pinned runtime speaks — and
    // only for a language in the qualified release set (ADR-022 §8/§9). The
    // checks are ordered cheapest-first and short-circuit on the first failure.
    if entry.version.is_empty() || entry.version == "*" {
        return Err(ManifestError::Unpinned {
            language: entry.language,
        });
    }
    if entry.license_spdx.is_empty() {
        return Err(ManifestError::MissingLicense {
            language: entry.language,
        });
    }
    if entry.abi < MIN_SUPPORTED_LANGUAGE_ABI || entry.abi > SUPPORTED_LANGUAGE_ABI {
        return Err(ManifestError::UnsupportedAbi {
            language: entry.language,
            abi: entry.abi,
        });
    }
    if !release_grammars.contains(&entry.language) {
        return Err(ManifestError::AbsentFromRelease {
            language: entry.language,
        });
    }
    Ok(())
}

/// Validate every entry in a manifest, collecting all failures.
pub fn validate_manifest(
    manifest: &[GrammarEntry],
    release_grammars: &[&str],
) -> Result<(), Vec<ManifestError>> {
    let errors: Vec<ManifestError> = manifest
        .iter()
        .filter_map(|e| validate_entry(e, release_grammars).err())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// The built-in release grammar manifest: one [`GrammarEntry`] per
/// production-qualified grammar statically linked into official release
/// binaries for the L1 foundation (ADR-022 §8/§9).
///
/// Provenance — every entry is a crates.io-pinned grammar whose `LanguageFn`
/// is wired in [`crate::language::SourceLanguage::tree_sitter_language`]:
///
/// | language    | crate                    | version  | upstream                                          | license |
/// |-------------|--------------------------|----------|---------------------------------------------------|---------|
/// | python      | `tree-sitter-python`     | `0.25.0` | `https://github.com/tree-sitter/tree-sitter-python`     | MIT     |
/// | javascript  | `tree-sitter-javascript` | `0.25.0` | `https://github.com/tree-sitter/tree-sitter-javascript` | MIT     |
/// | typescript  | `tree-sitter-typescript` | `0.23.2` | `https://github.com/tree-sitter/tree-sitter-typescript` | MIT     |
/// | bash        | `tree-sitter-bash`       | `0.25.1` | `https://github.com/tree-sitter/tree-sitter-bash`       | MIT     |
///
/// The recorded `abi` is the pinned runtime's current ABI
/// ([`SUPPORTED_LANGUAGE_ABI`]); [`crate::language`] tests assert each wired
/// grammar's live `abi_version` matches this value.
pub const BUILTIN_MANIFEST: &[GrammarEntry] = &[
    GrammarEntry {
        language: "python",
        crate_name: "tree-sitter-python",
        version: "0.25.0",
        upstream: "https://github.com/tree-sitter/tree-sitter-python",
        license_spdx: "MIT",
        abi: SUPPORTED_LANGUAGE_ABI,
    },
    GrammarEntry {
        language: "javascript",
        crate_name: "tree-sitter-javascript",
        version: "0.25.0",
        upstream: "https://github.com/tree-sitter/tree-sitter-javascript",
        license_spdx: "MIT",
        abi: SUPPORTED_LANGUAGE_ABI,
    },
    GrammarEntry {
        language: "typescript",
        crate_name: "tree-sitter-typescript",
        version: "0.23.2",
        upstream: "https://github.com/tree-sitter/tree-sitter-typescript",
        license_spdx: "MIT",
        // tree-sitter-typescript 0.23.2 was generated for ABI 14; the pinned
        // 0.26 runtime (ABI 15) accepts it as backwards-compatible
        // (MIN_SUPPORTED_LANGUAGE_ABI..=SUPPORTED_LANGUAGE_ABI). The
        // `builtin_manifest_abi_matches_the_live_tree_sitter_runtime` test
        // guards this value against drift.
        abi: 14,
    },
    GrammarEntry {
        language: "bash",
        crate_name: "tree-sitter-bash",
        version: "0.25.1",
        upstream: "https://github.com/tree-sitter/tree-sitter-bash",
        license_spdx: "MIT",
        abi: SUPPORTED_LANGUAGE_ABI,
    },
];
