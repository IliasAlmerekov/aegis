use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(
    name = "aegis_benchcheck",
    about = "Validate Criterion benchmark output against the checked-in Aegis baseline policy"
)]
struct Args {
    /// Path to the checked-in benchmark baseline policy.
    #[arg(long, default_value = "perf/scanner_bench_baseline.toml")]
    baseline: PathBuf,

    /// Criterion output root directory containing benchmark subdirectories.
    #[arg(long, default_value = "target/criterion")]
    criterion_root: PathBuf,
}

#[derive(Debug, Deserialize)]
struct BenchmarkPolicyFile {
    default_regression_pct: f64,
    benchmarks: Vec<BenchmarkPolicy>,
}

#[derive(Debug, Deserialize)]
struct BenchmarkPolicy {
    name: String,
    baseline_ns: f64,
    regression_pct: Option<f64>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CriterionEstimateFile {
    mean: CriterionPointEstimate,
}

#[derive(Debug, Deserialize)]
struct CriterionPointEstimate {
    point_estimate: f64,
}

#[derive(Debug)]
struct BenchmarkReport {
    name: String,
    baseline_ns: f64,
    observed_ns: Option<f64>,
    delta_pct: Option<f64>,
    threshold_pct: f64,
    description: Option<String>,
    failure_reason: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let policy = load_policy(&args.baseline)?;
    let reports = evaluate_policy(&policy, &args.criterion_root)?;

    for report in &reports {
        println!("{}", report.render_line());
    }

    let failures: Vec<&BenchmarkReport> = reports
        .iter()
        .filter(|report| report.failure_reason.is_some())
        .collect();

    if failures.is_empty() {
        println!(
            "benchmark regression check passed for {} benchmark(s)",
            reports.len()
        );
        return Ok(());
    }

    eprintln!(
        "benchmark regression check failed for {} benchmark(s)",
        failures.len()
    );
    for report in failures {
        if let Some(reason) = &report.failure_reason {
            eprintln!("{reason}");
        }
    }

    bail!("benchmark regression policy failed")
}

fn load_policy(path: &Path) -> Result<BenchmarkPolicyFile> {
    let contents = fs::read_to_string(path).with_context(|| {
        format!(
            "failed to read benchmark baseline policy {}",
            path.display()
        )
    })?;
    toml::from_str(&contents).with_context(|| {
        format!(
            "failed to parse benchmark baseline policy {}",
            path.display()
        )
    })
}

fn evaluate_policy(
    policy: &BenchmarkPolicyFile,
    criterion_root: &Path,
) -> Result<Vec<BenchmarkReport>> {
    let mut reports = Vec::with_capacity(policy.benchmarks.len());

    for benchmark in &policy.benchmarks {
        let threshold_pct = benchmark
            .regression_pct
            .unwrap_or(policy.default_regression_pct);
        let estimate_path = criterion_root
            .join(&benchmark.name)
            .join("new")
            .join("estimates.json");

        match load_point_estimate(&estimate_path) {
            Ok(observed_ns) => {
                let delta_pct = percent_delta(benchmark.baseline_ns, observed_ns);
                let failure_reason = (delta_pct > threshold_pct).then(|| {
                    format!(
                        "benchmark {} regressed by +{delta_pct:.1}% (threshold +{threshold_pct:.1}%)",
                        benchmark.name
                    )
                });

                reports.push(BenchmarkReport {
                    name: benchmark.name.clone(),
                    baseline_ns: benchmark.baseline_ns,
                    observed_ns: Some(observed_ns),
                    delta_pct: Some(delta_pct),
                    threshold_pct,
                    description: benchmark.description.clone(),
                    failure_reason,
                });
            }
            Err(_) => {
                reports.push(BenchmarkReport {
                    name: benchmark.name.clone(),
                    baseline_ns: benchmark.baseline_ns,
                    observed_ns: None,
                    delta_pct: None,
                    threshold_pct,
                    description: benchmark.description.clone(),
                    failure_reason: Some(format!(
                        "missing criterion result for {} at {}",
                        benchmark.name,
                        estimate_path.display()
                    )),
                });
            }
        }
    }

    Ok(reports)
}

fn load_point_estimate(path: &Path) -> Result<f64> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read criterion estimate {}", path.display()))?;
    let estimate: CriterionEstimateFile = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse criterion estimate {}", path.display()))?;
    Ok(estimate.mean.point_estimate)
}

fn percent_delta(baseline_ns: f64, observed_ns: f64) -> f64 {
    ((observed_ns - baseline_ns) / baseline_ns) * 100.0
}

fn format_ns(ns: f64) -> String {
    if ns >= 1_000_000.0 {
        format!("{:.3} ms", ns / 1_000_000.0)
    } else if ns >= 1_000.0 {
        format!("{:.3} µs", ns / 1_000.0)
    } else {
        format!("{ns:.0} ns")
    }
}

impl BenchmarkReport {
    fn render_line(&self) -> String {
        let status = if self.failure_reason.is_some() {
            "FAIL"
        } else {
            "PASS"
        };

        let mut line = match (self.observed_ns, self.delta_pct) {
            (Some(observed_ns), Some(delta_pct)) => format!(
                "{status} {} observed {} baseline {} delta {delta_pct:+.1}% threshold +{:.1}%",
                self.name,
                format_ns(observed_ns),
                format_ns(self.baseline_ns),
                self.threshold_pct,
            ),
            _ => format!(
                "{status} {} missing criterion result baseline {} threshold +{:.1}%",
                self.name,
                format_ns(self.baseline_ns),
                self.threshold_pct,
            ),
        };

        if let Some(description) = &self.description {
            line.push_str(" — ");
            line.push_str(description);
        }

        line
    }
}
