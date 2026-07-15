use std::path::PathBuf;
use std::process::Command;

const BANNED_PHRASES: [&str; 4] = [
    concat!("tamper", "-evident"),
    concat!("tamper", "-proof"),
    concat!("tamper", " detection"),
    concat!("tamper", " evidence"),
];

const ALLOWED_CONTEXTS: [(&str, &str); 11] = [
    (
        "CHANGELOG.md",
        concat!(
            "- Aligned landing page copy with the current install flow while keeping the existing design (3D shield and section layout unchanged): installer/Homebrew/npm/Cargo, `aegis setup-shell` opt-in, `v0.5.8`, and honest audit wording (append-only; tamper",
            "-evident when hash-chain integrity is enabled) replacing the prior overclaim (M6)."
        ),
    ),
    (
        "CONTEXT.md",
        concat!(
            "but has no keyed or external anchor and therefore does not prove adversarial tamper",
            "-evidence against an actor who can rewrite the whole local log."
        ),
    ),
    (
        "CONTEXT.md",
        concat!("_Avoid_: tamper", "-evident log, tamper", "-proof audit"),
    ),
    (
        "PROJECT_STATE.md",
        concat!(
            "  `Snapshot` state, and `Rollback` from adversarial tamper",
            " proof, backup, or"
        ),
    ),
    (
        "TASKS.md",
        concat!(
            "  corruption and some edits, but cannot prove adversarial tamper",
            "-evidence against"
        ),
    ),
    (
        "TASKS.md",
        concat!(
            "  claim adversarial tamper",
            "-evidence. Cryptographic anchoring is out of the 1.0"
        ),
    ),
    (
        "docs/adr/adr-013-project-config-security-ratchet.md",
        concat!(
            "  tamper",
            "-evidence — see TASKS.md H5 — but silently disabling even that from an"
        ),
    ),
    (
        "docs/plans/2026-07-14-h5-audit-integrity-contract.md",
        concat!(
            "\"tamper",
            "-evident\" exceeds Aegis' local heuristic-guardrail contract."
        ),
    ),
    (
        "docs/plans/2026-07-14-h5-audit-integrity-contract.md",
        concat!(
            "1. **Terminology.** Ban `tamper",
            "-evident`, `tamper",
            "-proof`, `tamper",
            " detection`,"
        ),
    ),
    (
        "docs/plans/2026-07-14-h5-audit-integrity-contract.md",
        concat!(
            "   and `tamper",
            " evidence` as *capability claims* for the local chain. Canonical"
        ),
    ),
    (
        "docs/plans/2026-07-14-h5-audit-integrity-contract.md",
        concat!(
            "- `rtk git grep -n \"tamper",
            "-evident\\|tamper",
            " evidence\\|tamper",
            " detection\"` — only"
        ),
    ),
];

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn tracked_files() -> Vec<String> {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("audit-integrity wording guard requires git to be installed");

    assert!(
        output.status.success(),
        "audit-integrity wording guard requires `git ls-files` to succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    output
        .stdout
        .split(|byte| *byte == b'\0')
        .filter_map(|path| (!path.is_empty()).then(|| String::from_utf8_lossy(path).into_owned()))
        .collect()
}

fn is_allowed_context(path: &str, line: &str) -> bool {
    ALLOWED_CONTEXTS
        .iter()
        .any(|(allowed_path, allowed_line)| path == *allowed_path && line == *allowed_line)
}

#[test]
fn wording_allowlist_rejects_unlisted_capability_claims() {
    assert!(
        !is_allowed_context(
            "CONTEXT.md",
            concat!(
                "The local audit log is tamper",
                "-evident only if you trust the local disk."
            )
        ),
        "a broad residual-risk marker must not allow an adjacent capability claim"
    );
    assert!(
        !is_allowed_context(
            "CHANGELOG.md",
            concat!(
                "The local audit log is tamper",
                "-evident for every deployment."
            )
        ),
        "historical wording must be allowlisted per line, not for the whole changelog"
    );
}

#[test]
fn tracked_contract_guard_is_independent_of_process_working_directory() {
    let temporary_directory = tempfile::tempdir().expect("temporary directory must be created");
    let output = Command::new(std::env::current_exe().expect("test executable path must resolve"))
        .args([
            "--exact",
            "tracked_public_contracts_do_not_overclaim_audit_integrity",
        ])
        .current_dir(temporary_directory.path())
        .output()
        .expect("wording guard subprocess must run");

    assert!(
        output.status.success(),
        "wording guard must not depend on its process working directory: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn tracked_public_contracts_do_not_overclaim_audit_integrity() {
    let mut violations = Vec::new();

    for path in tracked_files() {
        let bytes = std::fs::read(repo_path(&path))
            .unwrap_or_else(|err| panic!("failed to read tracked file `{path}`: {err}"));
        let Ok(contents) = std::str::from_utf8(&bytes) else {
            assert!(
                matches!(
                    path.as_str(),
                    "landing/public/favicon.ico"
                        | "landing/public/models/shield.glb"
                        | "landing/public/shield-icon.png"
                        | "src/assets/aegis.gif"
                        | "src/assets/howitwork.png"
                ),
                "tracked non-UTF-8 file `{path}` must be added to the explicit binary allowlist"
            );
            continue;
        };

        for (index, line) in contents.lines().enumerate() {
            let lowercase = line.to_ascii_lowercase();
            for phrase in BANNED_PHRASES {
                if lowercase.contains(phrase) && !is_allowed_context(&path, line) {
                    violations.push(format!(
                        "{path}:{}: `{phrase}` overclaims local audit integrity; use `audit integrity chain` or `integrity check` instead",
                        index + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "audit integrity wording guard found capability claims:\n{}",
        violations.join("\n")
    );
}
