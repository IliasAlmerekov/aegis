//! Live routed-source and effective-budget orchestration regressions.

use std::time::Duration;

use aegis::analysis::{
    AnalysisCwd, OrchestrationBudget, Outcome, run, run_with_budget, run_with_budget_in_cwd,
};
use aegis_types::{
    AnalysisStatus, Assessment, DegradationReason, MatchEvidence, ParsedCommand, RiskLevel,
};
use sha2::{Digest, Sha256};

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
async fn run_resolves_relative_script_file_against_command_cwd() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("run.py"), "print('ok')\n").unwrap();
    let baseline = safe_baseline();

    let outcome = run_with_budget_in_cwd(
        "python3 ./run.py",
        AnalysisCwd::Resolved(workspace.path()),
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        OrchestrationBudget {
            total_timeout: Duration::from_secs(2),
            ..OrchestrationBudget::L1_DEFAULT
        },
    )
    .await;
    let assessment = match outcome {
        Outcome::Analyzed { assessment, .. } => assessment,
        other => panic!("relative script file must be analyzed: {other:?}"),
    };

    assert_eq!(
        assessment.analysis.as_ref().map(|analysis| analysis.status),
        Some(AnalysisStatus::NotApplicable),
        "{assessment:?}"
    );
}

#[tokio::test]
async fn run_resolves_relative_direct_exec_against_command_cwd() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(
        workspace.path().join("direct-script"),
        "#!/usr/bin/env python3\nimport os\nos.remove('victim')\n",
    )
    .unwrap();
    let baseline = safe_baseline();

    let outcome = run_with_budget_in_cwd(
        "./direct-script",
        AnalysisCwd::Resolved(workspace.path()),
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        OrchestrationBudget {
            total_timeout: Duration::from_secs(2),
            ..OrchestrationBudget::L1_DEFAULT
        },
    )
    .await;
    let assessment = match outcome {
        Outcome::Analyzed { assessment, .. } => assessment,
        other => panic!("relative direct executable must be analyzed: {other:?}"),
    };

    assert!(
        assessment
            .matched
            .iter()
            .any(|matched| matched.pattern.id.as_ref() == "LANG-FS-DEL")
    );
}

#[tokio::test]
async fn run_degrades_relative_script_when_command_cwd_is_unavailable() {
    let baseline = safe_baseline();

    let outcome = run_with_budget_in_cwd(
        "python3 ./aegis-unavailable-cwd-source.py",
        AnalysisCwd::Unavailable,
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        OrchestrationBudget {
            total_timeout: Duration::from_secs(2),
            ..OrchestrationBudget::L1_DEFAULT
        },
    )
    .await;
    let assessment = match outcome {
        Outcome::Analyzed { assessment, .. } => assessment,
        other => panic!("unavailable cwd must produce degradation: {other:?}"),
    };

    assert!(assessment.analysis.as_ref().is_some_and(|analysis| {
        analysis
            .degradation_reasons
            .contains(&DegradationReason::DynamicSource)
    }));
}

#[tokio::test]
async fn run_preserves_dynamic_source_as_typed_degradation() {
    let baseline = safe_baseline();
    let outcome = run(
        "unknown-producer | python3",
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        Duration::from_secs(2),
    )
    .await;
    let assessment = match outcome {
        Outcome::Analyzed { assessment, .. } => assessment,
        other => panic!("dynamic source must produce a degraded assessment: {other:?}"),
    };

    assert_eq!(
        assessment.analysis.as_ref().map(|a| a.status),
        Some(AnalysisStatus::Degraded)
    );
    assert!(assessment.analysis.as_ref().is_some_and(|a| {
        a.degradation_reasons
            .contains(&DegradationReason::DynamicSource)
    }));
}

#[tokio::test]
async fn run_resolves_script_file_and_honors_file_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("danger.py");
    std::fs::write(&path, "import os\nos.remove('victim')\n").unwrap();
    let command = format!("python3 {}", path.display());
    let baseline = safe_baseline();

    let analyzed = run(
        &command,
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        Duration::from_secs(2),
    )
    .await;
    let assessment = match analyzed {
        Outcome::Analyzed { assessment, .. } => assessment,
        other => panic!("script file must be analyzed: {other:?}"),
    };
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "LANG-FS-DEL")
    );

    let degraded = run_with_budget(
        &command,
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        OrchestrationBudget {
            script_file_limit_bytes: 1,
            total_timeout: Duration::from_secs(2),
            ..OrchestrationBudget::L1_DEFAULT
        },
    )
    .await;
    let assessment = match degraded {
        Outcome::Analyzed { assessment, .. } => assessment,
        other => panic!("oversized script must degrade: {other:?}"),
    };
    assert!(assessment.analysis.as_ref().is_some_and(|a| {
        a.degradation_reasons
            .contains(&DegradationReason::LimitExceeded)
    }));
}

#[tokio::test]
async fn run_degrades_missing_script_file_instead_of_auto_safe() {
    let baseline = safe_baseline();
    let outcome = run(
        "python3 /definitely/missing/aegis-language-test.py",
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        Duration::from_secs(2),
    )
    .await;
    let assessment = match outcome {
        Outcome::Analyzed { assessment, .. } => assessment,
        other => panic!("missing script must produce degradation: {other:?}"),
    };
    assert!(assessment.analysis.as_ref().is_some_and(|a| {
        a.degradation_reasons
            .contains(&DegradationReason::UnsafeSource)
    }));
}

#[tokio::test]
async fn run_analyzes_direct_exec_with_verified_shebang() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("direct-script");
    std::fs::write(
        &path,
        "#!/usr/bin/env python3\nimport os\nos.remove('victim')\n",
    )
    .unwrap();
    let baseline = safe_baseline();
    let outcome = run(
        &path.display().to_string(),
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        Duration::from_secs(2),
    )
    .await;
    let assessment = match outcome {
        Outcome::Analyzed { assessment, .. } => assessment,
        other => panic!("verified direct script must be analyzed: {other:?}"),
    };
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "LANG-FS-DEL")
    );
}

#[tokio::test]
async fn run_records_target_aggregate_and_total_time_budget_exhaustion() {
    let baseline = safe_baseline();
    for budget in [
        OrchestrationBudget {
            inline_source_limit_bytes: 1,
            total_timeout: Duration::from_secs(2),
            ..OrchestrationBudget::L1_DEFAULT
        },
        OrchestrationBudget {
            max_targets: 0,
            total_timeout: Duration::from_secs(2),
            ..OrchestrationBudget::L1_DEFAULT
        },
        OrchestrationBudget {
            max_aggregate_bytes: 1,
            total_timeout: Duration::from_secs(2),
            ..OrchestrationBudget::L1_DEFAULT
        },
        OrchestrationBudget {
            total_timeout: Duration::ZERO,
            ..OrchestrationBudget::L1_DEFAULT
        },
    ] {
        let outcome = run_with_budget(
            "python3 -c 'print(1)'",
            &baseline,
            Some(env!("CARGO_BIN_EXE_aegis")),
            &[],
            budget,
        )
        .await;
        let assessment = match outcome {
            Outcome::Analyzed { assessment, .. } => assessment,
            other => panic!("budget exhaustion must degrade: {other:?}"),
        };
        assert!(
            assessment.analysis.as_ref().is_some_and(|a| a
                .degradation_reasons
                .contains(&DegradationReason::LimitExceeded)),
            "{assessment:?}"
        );
    }
}

#[tokio::test]
async fn script_resolution_is_inside_total_deadline_and_preserves_original_byte_hash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bom.py");
    let bytes = b"\xEF\xBB\xBFimport os\nos.remove('victim')\n";
    std::fs::write(&path, bytes).unwrap();
    let command = format!("python3 {}", path.display());
    let baseline = safe_baseline();

    let timed_out = run_with_budget(
        &command,
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        OrchestrationBudget {
            total_timeout: Duration::ZERO,
            ..OrchestrationBudget::L1_DEFAULT
        },
    )
    .await;
    let timed_out = match timed_out {
        Outcome::Analyzed { assessment, .. } => assessment,
        other => panic!("zero deadline must degrade before file resolution: {other:?}"),
    };
    assert!(timed_out.analysis.as_ref().is_some_and(|a| {
        a.degradation_reasons
            .contains(&DegradationReason::LimitExceeded)
    }));

    let analyzed = run(
        &command,
        &baseline,
        Some(env!("CARGO_BIN_EXE_aegis")),
        &[],
        Duration::from_secs(2),
    )
    .await;
    let analyzed = match analyzed {
        Outcome::Analyzed { assessment, .. } => assessment,
        other => panic!("BOM script must be analyzed: {other:?}"),
    };
    let expected_hash = format!("{:x}", Sha256::digest(bytes));
    let provenance = analyzed.matched.iter().find_map(|matched| {
        if let MatchEvidence::LanguageRule { provenance, .. } = &matched.evidence {
            Some(provenance)
        } else {
            None
        }
    });
    let provenance = provenance.expect("language match provenance");
    assert_eq!(
        provenance.source_hash.as_deref(),
        Some(expected_hash.as_str())
    );
    assert_eq!(
        provenance.span.map(|span| span.byte_start),
        Some(13),
        "BOM-prefixed spans must map back to original bytes"
    );
}
