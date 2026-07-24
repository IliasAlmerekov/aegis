//! Slice B — parent-side language analysis orchestration (ADR-022 §2/§6, L1
//! Iteration 6). Real-subprocess seam: these tests spawn the actual
//! `aegis --internal-language-worker` binary and exercise the full
//! route → spawn → Analyze → map → merge composition through the public
//! `aegis::analysis::run` boundary.

use std::time::Duration;

use aegis::analysis::{Outcome, run};
use aegis_types::{AnalysisStatus, Assessment, DegradationReason, ParsedCommand, RiskLevel};

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
    // A `bash -c` inline body routes to Bash, which has no adapter yet (L1
    // Shell/Bash is Iteration 8), so the worker returns
    // Response::UnsupportedLanguage. The orchestration must record
    // GrammarUnavailable degradation (ADR-022 §4) — never claim the target was
    // analyzed safely. (JavaScript and TypeScript gained adapters in Iteration 7,
    // so `node -e` and a future TS inline runner no longer exercise this path.)
    let baseline = safe_baseline();
    let outcome = run(
        "bash -c \"x\"",
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        Duration::from_secs(5),
    )
    .await;
    let assessment = match outcome {
        Outcome::Analyzed { assessment, .. } => assessment,
        other => panic!("a bash inline body must spawn the worker: {other:?}"),
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
async fn run_analyzes_inline_javascript_and_merges_a_filesystem_delete_match() {
    // `node -e "fs.unlinkSync('data.txt')"` routes to JavaScript. The adapter
    // emits FilesystemDelete (non-recursive, non-forced), which classifies as
    // LANG-FS-DEL at Warn. The real worker subprocess analyzes the inline body
    // and the merged Assessment must carry that match and lift to at least Warn.
    let baseline = safe_baseline();
    let outcome = run(
        "node -e \"fs.unlinkSync('data.txt')\"",
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
        other => panic!("inline javascript must spawn the worker: {other:?}"),
    };
    assert!(
        assessment.risk >= RiskLevel::Warn,
        "risk must lift to at least Warn: {:?}",
        assessment.risk
    );
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "LANG-FS-DEL"),
        "must carry a LANG-FS-DEL match: {:?}",
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
        "a clean non-recursive delete must complete without degradation"
    );
}

#[tokio::test]
async fn run_analyzes_a_javascript_exec_payload_and_degrades_the_recursive_bash_target() {
    // `node -e "child_process.exec('rm -rf /tmp/x')"` — the inline body is a
    // JavaScript `child_process.exec` of a literal shell string. The top-level
    // sink is CodeExecution (LANG-EXEC at Danger); its literal payload is Bash
    // shell source, enqueued as a recursive target. Bash has no adapter yet (L1
    // Shell/Bash is Iteration 8), so the recursive target degrades as
    // GrammarUnavailable (ADR-022 §9 honest degradation). Slice C drains the
    // recursive queue, so the LANG-EXEC match must surface, target_count must
    // cover the top-level AND the recursive target (>= 2), and the overall
    // status must be Degraded — never claiming the recursive target was clean.
    let baseline = safe_baseline();
    let outcome = run(
        "node -e \"child_process.exec('rm -rf /tmp/x')\"",
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
        other => panic!("a javascript exec inline body must spawn the worker: {other:?}"),
    };
    let ids: Vec<&str> = assessment
        .matched
        .iter()
        .map(|m| m.pattern.id.as_ref())
        .collect();
    assert!(
        ids.contains(&"LANG-EXEC"),
        "top-level child_process.exec must match LANG-EXEC: {ids:?}"
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
        "the recursive Bash target must degrade the overall status: {:?}",
        summary.status
    );
    assert!(
        summary
            .degradation_reasons
            .contains(&DegradationReason::GrammarUnavailable),
        "must record GrammarUnavailable for the unsupported recursive Bash target: {:?}",
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

#[tokio::test]
async fn run_analyzes_inline_javascript_chmod_and_lifts_to_danger() {
    // `node -e "fs.chmodSync('x', 0o777)"` routes to JavaScript. The adapter
    // emits PermissionOrOwnershipChange, which classifies as LANG-FS-CHMOD at
    // Danger. No execution sink → no recursive target. (The Python analog
    // covers `os.chmod`; this pins the same contract at the JS orchestration
    // seam, which Slice 2 did not cover.)
    let baseline = safe_baseline();
    let outcome = run(
        "node -e \"fs.chmodSync('x', 0o777)\"",
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
        other => panic!("inline javascript chmod must spawn the worker: {other:?}"),
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
async fn run_analyzes_inline_javascript_writefilesync_and_lifts_to_warn() {
    // `node -e "fs.writeFileSync('x', 'y')"` routes to JavaScript. The adapter
    // emits FilesystemOverwrite{destructive_mode} (writeFileSync truncates),
    // which classifies as LANG-FS-OVR-W at Warn. No execution sink → no
    // recursive target. (The Python analog covers `open('x','w')`; this pins
    // the same contract at the JS orchestration seam, which Slice 2 did not
    // cover.)
    let baseline = safe_baseline();
    let outcome = run(
        "node -e \"fs.writeFileSync('x', 'y')\"",
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
        other => panic!("inline javascript writeFileSync must spawn the worker: {other:?}"),
    };
    assert!(
        assessment.risk >= RiskLevel::Warn,
        "writeFileSync must lift Safe → Warn: {:?}",
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
        "a clean writeFileSync must complete without degradation"
    );
}

#[tokio::test]
async fn run_analyzes_javascript_exec_and_surfaces_the_recursive_javascript_target() {
    // `node -e "eval('fs.unlinkSync(x)')"` — the inline body is a JavaScript
    // `eval` of a literal JavaScript string. The top-level `eval` is a
    // CodeExecution sink (LANG-EXEC at Danger); its literal payload
    // `fs.unlinkSync(x)` is itself analyzable JavaScript, enqueued as a
    // recursive target. Unlike the JS exec→Bash test (Slice 2), the recursive
    // target is JavaScript — which IS supported — so BOTH matches must surface,
    // `target_count` must cover the top-level AND the recursive target (>= 2),
    // and the overall status must be Complete with NO degradation. This pins
    // the JS→JS recursion contract, which no prior test covered (the existing
    // JS exec test recurses into Bash, which degrades as GrammarUnavailable).
    //
    // The inner operand `x` is a bare identifier (variable), so the recursive
    // target is a FilesystemDelete with `Dynamic` certainty — a non-execution
    // dynamic operand emits its match without degradation (Python C3 fix), so
    // the recursive LANG-FS-DEL surfaces cleanly and the status stays Complete.
    let baseline = safe_baseline();
    let outcome = run(
        "node -e \"eval('fs.unlinkSync(x)')\"",
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
                "top-level + recursive JS target must be analyzed: {target_count}"
            );
            assessment
        }
        other => panic!("a javascript eval inline body must spawn the worker: {other:?}"),
    };
    let ids: Vec<&str> = assessment
        .matched
        .iter()
        .map(|m| m.pattern.id.as_ref())
        .collect();
    assert!(
        ids.contains(&"LANG-EXEC"),
        "top-level eval must match LANG-EXEC: {ids:?}"
    );
    assert!(
        ids.contains(&"LANG-FS-DEL"),
        "recursive fs.unlinkSync must match LANG-FS-DEL: {ids:?}"
    );
    assert!(
        assessment.risk >= RiskLevel::Danger,
        "risk must lift to Danger: {:?}",
        assessment.risk
    );
    let summary = assessment.analysis.as_ref().expect("analysis must be set");
    assert_eq!(
        summary.status,
        AnalysisStatus::Complete,
        "both JS targets are supported; the recursive target must NOT degrade: {:?}",
        summary.status
    );
    assert!(
        !summary
            .degradation_reasons
            .contains(&DegradationReason::GrammarUnavailable),
        "a JS→JS recursive target must not record GrammarUnavailable: {:?}",
        summary.degradation_reasons
    );
}
