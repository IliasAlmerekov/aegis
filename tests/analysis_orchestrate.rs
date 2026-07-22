//! Slice B — parent-side language analysis orchestration (ADR-022 §2/§6, L1
//! Iteration 6). Real-subprocess seam: these tests spawn the actual
//! `aegis --internal-language-worker` binary and exercise the full
//! route → spawn → Analyze → map → merge composition through the public
//! `aegis::analysis::run` boundary.

use std::time::Duration;

use aegis::analysis::{Outcome, run};
use aegis_types::{AnalysisStatus, Assessment, DegradationReason, ParsedCommand, RiskLevel};

/// A throwaway `Assessment` baseline (Safe, no matches, no language analysis)
/// that mirrors the shape the scanner produces before language analysis runs.
fn safe_baseline() -> Assessment {
    Assessment {
        risk: RiskLevel::Safe,
        effect_opaque: false,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: ParsedCommand {
            program: None,
            argv: Vec::new(),
            normalized: String::new(),
            inline_scripts: Vec::new(),
            raw: String::new(),
        },
        analysis: None,
    }
}

#[tokio::test]
async fn run_returns_baseline_unchanged_when_route_yields_no_inline_target() {
    // `ls -la` routes to no interpreter, so there is no analyzable inline
    // source — ADR-022 §0: no worker is spawned and the baseline is returned
    // untouched (analysis still None).
    let baseline = safe_baseline();
    let outcome = run(
        "ls -la",
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        Duration::from_secs(2),
    )
    .await;
    match outcome {
        Outcome::NotStarted { baseline: b } => assert!(
            b.analysis.is_none(),
            "no-source baseline must be returned with no language analysis"
        ),
        other => panic!("no inline target must not start a worker: {other:?}"),
    }
}
#[tokio::test]
async fn run_analyzes_inline_python_and_merges_a_recursive_delete_match() {
    // `shutil.rmtree('x')` is a recursive filesystem delete: the Python adapter
    // emits FilesystemDelete{recursive:true}, which classifies as LANG-FS-DEL-R
    // at Danger. The real worker subprocess analyzes the inline body and the
    // merged Assessment must lift to Danger and carry that match.
    let baseline = safe_baseline();
    let outcome = run(
        "python3 -c \"shutil.rmtree('x')\"",
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        Duration::from_secs(5),
    )
    .await;
    let assessment = match outcome {
        Outcome::Analyzed {
            assessment,
            target_count,
        } => {
            assert_eq!(target_count, 1, "one inline target must be analyzed");
            assessment
        }
        other => panic!("inline python must spawn the worker: {other:?}"),
    };
    assert!(
        assessment.risk >= RiskLevel::Danger,
        "risk must lift to Danger: {:?}",
        assessment.risk
    );
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "LANG-FS-DEL-R"),
        "must carry a LANG-FS-DEL-R match: {:?}",
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref().to_string())
            .collect::<Vec<_>>()
    );
    let summary = assessment.analysis.as_ref().expect("analysis must be set");
    assert_eq!(
        summary.status,
        AnalysisStatus::Complete,
        "a clean recursive delete must complete without degradation"
    );
}

#[tokio::test]
async fn run_records_grammar_unavailable_for_an_unsupported_language() {
    // A `node -e` inline body routes to JavaScript, which has no adapter in L1,
    // so the worker returns Response::UnsupportedLanguage. The orchestration
    // must record GrammarUnavailable degradation (ADR-022 §4) — never claim the
    // target was analyzed safely.
    let baseline = safe_baseline();
    let outcome = run(
        "node -e \"x\"",
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        Duration::from_secs(5),
    )
    .await;
    let assessment = match outcome {
        Outcome::Analyzed { assessment, .. } => assessment,
        other => panic!("a node inline body must spawn the worker: {other:?}"),
    };
    let summary = assessment
        .analysis
        .as_ref()
        .expect("analysis must be set even when the language is unsupported");
    assert_eq!(
        summary.status,
        AnalysisStatus::Degraded,
        "an unsupported language must degrade, not complete"
    );
    assert!(
        summary
            .degradation_reasons
            .contains(&DegradationReason::GrammarUnavailable),
        "must record GrammarUnavailable: {:?}",
        summary.degradation_reasons
    );
}

#[tokio::test]
async fn run_analyzes_a_recursive_exec_payload_and_surfaces_the_nested_match() {
    // `python3 -c "exec('shutil.rmtree(x)')"` — the inline body is a Python
    // `exec` of a literal `shutil.rmtree(x)`. The top-level `exec` is a
    // CodeExecution sink (LANG-EXEC); its literal payload is itself analyzable
    // Python that recursively deletes a directory tree (LANG-FS-DEL-R). Slice C
    // drains the recursive queue, so BOTH matches must surface and `target_count`
    // must cover the top-level AND the recursive target (>= 2). Under Slice B the
    // recursive target was discarded, so only LANG-EXEC surfaced with count 1.
    let baseline = safe_baseline();
    let outcome = run(
        "python3 -c \"exec('shutil.rmtree(x)')\"",
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        Duration::from_secs(5),
    )
    .await;
    let assessment = match outcome {
        Outcome::Analyzed {
            assessment,
            target_count,
        } => {
            assert!(
                target_count >= 2,
                "top-level + recursive target must be analyzed: {target_count}"
            );
            assessment
        }
        other => panic!("an exec inline body must spawn the worker: {other:?}"),
    };
    let ids: Vec<&str> = assessment
        .matched
        .iter()
        .map(|m| m.pattern.id.as_ref())
        .collect();
    assert!(
        ids.contains(&"LANG-EXEC"),
        "top-level exec must match LANG-EXEC: {ids:?}"
    );
    assert!(
        ids.contains(&"LANG-FS-DEL-R"),
        "recursive shutil.rmtree must match LANG-FS-DEL-R: {ids:?}"
    );
    assert!(
        assessment.risk >= RiskLevel::Danger,
        "risk must lift to Danger: {:?}",
        assessment.risk
    );
}

#[tokio::test]
async fn run_analyzes_inline_chmod_and_lifts_to_danger() {
    // `python3 -c "os.chmod('x', 0o777)"` — a permission change. The Python
    // adapter emits PermissionOrOwnershipChange, which classifies as
    // LANG-FS-CHMOD at Danger. No execution sink → no recursive target.
    let baseline = safe_baseline();
    let outcome = run(
        "python3 -c \"os.chmod('x', 0o777)\"",
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        Duration::from_secs(5),
    )
    .await;
    let assessment = match outcome {
        Outcome::Analyzed {
            assessment,
            target_count,
        } => {
            assert_eq!(target_count, 1, "one inline target, no recursion");
            assessment
        }
        other => panic!("inline python chmod must spawn the worker: {other:?}"),
    };
    assert!(
        assessment.risk >= RiskLevel::Danger,
        "chmod must lift to Danger: {:?}",
        assessment.risk
    );
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "LANG-FS-CHMOD"),
        "must carry LANG-FS-CHMOD: {:?}",
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref().to_string())
            .collect::<Vec<_>>()
    );
    let summary = assessment.analysis.as_ref().expect("analysis must be set");
    assert_eq!(
        summary.status,
        AnalysisStatus::Complete,
        "a clean chmod must complete without degradation"
    );
}

#[tokio::test]
async fn run_analyzes_inline_open_write_and_lifts_to_warn() {
    // `python3 -c "open('x','w')"` — a truncating write. The Python adapter
    // emits FilesystemOverwrite{destructive_mode}, which classifies as
    // LANG-FS-OVR-W at Warn (the highest risk an overwrite carries). No
    // execution sink → no recursive target.
    let baseline = safe_baseline();
    let outcome = run(
        "python3 -c \"open('x','w')\"",
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        Duration::from_secs(5),
    )
    .await;
    let assessment = match outcome {
        Outcome::Analyzed {
            assessment,
            target_count,
        } => {
            assert_eq!(target_count, 1, "one inline target, no recursion");
            assessment
        }
        other => panic!("inline python open-w must spawn the worker: {other:?}"),
    };
    assert!(
        assessment.risk >= RiskLevel::Warn,
        "open-w must lift Safe → Warn: {:?}",
        assessment.risk
    );
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "LANG-FS-OVR-W"),
        "must carry LANG-FS-OVR-W: {:?}",
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref().to_string())
            .collect::<Vec<_>>()
    );
    let summary = assessment.analysis.as_ref().expect("analysis must be set");
    assert_eq!(
        summary.status,
        AnalysisStatus::Complete,
        "a clean open-w must complete without degradation"
    );
}

#[tokio::test]
async fn run_cross_language_shell_payload_degrades_as_grammar_unavailable() {
    // `python3 -c "os.system('rm -rf /tmp/x')"` — a Python execution sink whose
    // literal payload is shell source. The top-level emits LANG-EXEC (Danger)
    // and enqueues a recursive Bash target (ADR-022 §7 cross-language). The
    // Bash adapter is not qualified in L1 (Iteration 8), so the worker returns
    // UnsupportedLanguage for the recursive target and the orchestration
    // records GrammarUnavailable degradation — never claiming the shell
    // payload was analyzed safely (ADR-022 §9). target_count covers the
    // top-level Python target AND the recursive Bash target (>= 2).
    let baseline = safe_baseline();
    let outcome = run(
        "python3 -c \"os.system('rm -rf /tmp/x')\"",
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        Duration::from_secs(5),
    )
    .await;
    let assessment = match outcome {
        Outcome::Analyzed {
            assessment,
            target_count,
        } => {
            assert!(
                target_count >= 2,
                "top-level + recursive Bash target: {target_count}"
            );
            assessment
        }
        other => panic!("inline python os.system must spawn the worker: {other:?}"),
    };
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "LANG-EXEC"),
        "top-level os.system must match LANG-EXEC: {:?}",
        assessment
            .matched
            .iter()
            .map(|m| m.pattern.id.as_ref().to_string())
            .collect::<Vec<_>>()
    );
    assert!(
        assessment.risk >= RiskLevel::Danger,
        "risk must lift to Danger: {:?}",
        assessment.risk
    );
    let summary = assessment.analysis.as_ref().expect("analysis must be set");
    assert_eq!(
        summary.status,
        AnalysisStatus::Degraded,
        "the unsupported Bash payload must degrade"
    );
    assert!(
        summary
            .degradation_reasons
            .contains(&DegradationReason::GrammarUnavailable),
        "must record GrammarUnavailable for the recursive Bash target: {:?}",
        summary.degradation_reasons
    );
}
