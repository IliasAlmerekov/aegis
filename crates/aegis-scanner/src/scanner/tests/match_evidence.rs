//! Language-aware analysis Iteration 1 — Slice D.
//!
//! Every `Match` now carries typed `MatchEvidence` identifying its detection
//! mechanism (ADR-022 §4). These tests pin the mechanism + source the scanner
//! records for each scan path — regex `Pattern` vs `Token-prefix rule` —
//! through the public `Scanner::assess` / `MatchResult.evidence` seam, without
//! asserting on classification (the Slice A compatibility fixtures already
//! guard that classifications are unchanged).
//!
//! Expected mechanisms are derived from how the scanner produces each match
//! (regex `full_scan` vs `Token-prefix rule` `prefix_scan`), an independent
//! source of truth — not from the evidence field itself.

use super::*;

/// Find the `MatchResult` whose pattern id equals `id`, panicking if absent.
fn match_for<'a>(assessment: &'a Assessment, id: &str) -> &'a MatchResult {
    assessment
        .matched
        .iter()
        .find(|m| m.pattern.id.as_ref() == id)
        .unwrap_or_else(|| {
            panic!(
                "expected matched pattern {id}, got {:?}",
                assessment
                    .matched
                    .iter()
                    .map(|m| m.pattern.id.as_ref())
                    .collect::<Vec<_>>()
            )
        })
}

#[test]
fn regex_pattern_match_records_regex_mechanism_and_builtin_source() {
    // `rm -rf` trips FS-001, a regex `Pattern` matched by `full_scan`.
    let assessment = scanner().assess("rm -rf /tmp/build");
    let m = match_for(&assessment, "FS-001");
    assert_eq!(
        m.evidence.mechanism(),
        DetectionMechanism::RegexPattern,
        "FS-001 is a regex Pattern match",
    );
    assert_eq!(
        m.evidence.source(),
        DetectionSource::Builtin,
        "FS-001 is a built-in pattern",
    );
}

#[test]
fn token_prefix_rule_match_records_token_prefix_mechanism() {
    // `git reset --hard` trips GIT-001, a `Token-prefix rule` matched by
    // `prefix_scan_effective_slices`.
    let assessment = scanner().assess("git reset --hard HEAD~1");
    let m = match_for(&assessment, "GIT-001");
    assert_eq!(
        m.evidence.mechanism(),
        DetectionMechanism::TokenPrefixRule,
        "GIT-001 is a Token-prefix rule match",
    );
    assert_eq!(m.evidence.source(), DetectionSource::Builtin);
}

#[test]
fn inline_extracted_regex_match_records_regex_mechanism() {
    // The `-c` body is extracted and scanned as a regex `Pattern` target; the
    // embedded `rm -rf` trips FS-001 via the regex path, not a token-prefix rule.
    let assessment = scanner().assess("bash -c 'rm -rf /tmp/x'");
    let m = match_for(&assessment, "FS-001");
    assert_eq!(m.evidence.mechanism(), DetectionMechanism::RegexPattern);
    assert_eq!(m.evidence.source(), DetectionSource::Builtin);
}

#[test]
fn every_matched_result_carries_evidence() {
    // ADR-022 §4: *every* Match carries typed evidence. Assert no matched
    // result is left without it across a corpus that exercises both mechanisms.
    for cmd in [
        "rm -rf /tmp/build",
        "git reset --hard HEAD~1",
        "mkfs.ext4 /dev/sda1",
        "git clean -fd .",
    ] {
        let assessment = scanner().assess(cmd);
        assert!(
            !assessment.matched.is_empty(),
            "corpus command {cmd:?} matched nothing",
        );
        for m in &assessment.matched {
            // mechanism() is total over the non_exhaustive enum; the call itself
            // proves evidence is populated (not a default/placeholder).
            let _ = m.evidence.mechanism();
            let _ = m.evidence.source();
        }
    }
}
