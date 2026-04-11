use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

fn benchcheck_bin() -> String {
    std::env::var("CARGO_BIN_EXE_aegis_benchcheck")
        .unwrap_or_else(|_| panic!("expected Cargo to expose aegis_benchcheck test binary"))
}

fn write_policy(root: &Path, contents: &str) -> String {
    let path = root.join("scanner_bench_baseline.toml");
    fs::write(&path, contents).expect("baseline policy should write");
    path.display().to_string()
}

fn write_estimate(root: &Path, bench_name: &str, point_estimate_ns: f64) {
    let dir = root.join(bench_name).join("new");
    fs::create_dir_all(&dir).expect("criterion benchmark dir should be created");
    let contents = format!(
        r#"{{
  "mean": {{
    "confidence_interval": {{
      "confidence_level": 0.95,
      "lower_bound": {point_estimate_ns},
      "upper_bound": {point_estimate_ns}
    }},
    "point_estimate": {point_estimate_ns},
    "standard_error": 0.0
  }}
}}"#
    );
    fs::write(dir.join("estimates.json"), contents).expect("estimates.json should write");
}

#[test]
fn benchcheck_accepts_results_within_threshold() {
    let temp = TempDir::new().expect("tempdir should exist");
    let criterion_root = temp.path().join("criterion");
    fs::create_dir_all(&criterion_root).expect("criterion root should exist");

    let policy_path = write_policy(
        temp.path(),
        r#"
default_regression_pct = 15.0

[[benchmarks]]
name = "1000_safe_commands"
baseline_ns = 2_000_000
"#,
    );
    write_estimate(&criterion_root, "1000_safe_commands", 2_150_000.0);

    let output = Command::new(benchcheck_bin())
        .args([
            "--baseline",
            &policy_path,
            "--criterion-root",
            &criterion_root.display().to_string(),
        ])
        .output()
        .expect("benchcheck should run");

    assert!(
        output.status.success(),
        "benchmark result within threshold should pass; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn benchcheck_rejects_regression_over_threshold_with_interpretable_output() {
    let temp = TempDir::new().expect("tempdir should exist");
    let criterion_root = temp.path().join("criterion");
    fs::create_dir_all(&criterion_root).expect("criterion root should exist");

    let policy_path = write_policy(
        temp.path(),
        r#"
default_regression_pct = 10.0

[[benchmarks]]
name = "1000_safe_commands"
baseline_ns = 2_000_000
description = "Safe-path benchmark"
"#,
    );
    write_estimate(&criterion_root, "1000_safe_commands", 2_500_000.0);

    let output = Command::new(benchcheck_bin())
        .args([
            "--baseline",
            &policy_path,
            "--criterion-root",
            &criterion_root.display().to_string(),
        ])
        .output()
        .expect("benchcheck should run");

    assert!(
        !output.status.success(),
        "benchmark result above threshold must fail"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1000_safe_commands"),
        "report must mention the benchmark name; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("+25.0%"),
        "report must show the regression percentage; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("threshold +10.0%"),
        "report must show the configured threshold; stdout:\n{stdout}"
    );
}

#[test]
fn benchcheck_fails_when_expected_benchmark_output_is_missing() {
    let temp = TempDir::new().expect("tempdir should exist");
    let criterion_root = temp.path().join("criterion");
    fs::create_dir_all(&criterion_root).expect("criterion root should exist");

    let policy_path = write_policy(
        temp.path(),
        r#"
default_regression_pct = 15.0

[[benchmarks]]
name = "heredoc_worst_case"
baseline_ns = 8_000_000
"#,
    );

    let output = Command::new(benchcheck_bin())
        .args([
            "--baseline",
            &policy_path,
            "--criterion-root",
            &criterion_root.display().to_string(),
        ])
        .output()
        .expect("benchcheck should run");

    assert!(
        !output.status.success(),
        "missing benchmark output must fail the check"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("missing criterion result"),
        "failure must explain the missing benchmark result; stderr:\n{stderr}"
    );
}
