//! Shared CI-detection helper used by the CLI runtime entrypoints.

use std::env;

fn truthy_env(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
}

fn falsy_env(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "no")
}

/// Returns `true` when Aegis is running inside a CI environment.
///
/// Detection order:
/// 1. `AEGIS_CI` — explicit override
/// 2. Well-known CI env vars set by major CI providers
pub fn is_ci_environment() -> bool {
    if let Ok(value) = env::var("AEGIS_CI") {
        if falsy_env(&value) {
            return false;
        }
        if truthy_env(&value) {
            return true;
        }
    }

    for key in [
        "CI",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "CIRCLECI",
        "BUILDKITE",
        "TRAVIS",
        "TF_BUILD",
    ] {
        if let Ok(value) = env::var(key)
            && truthy_env(&value)
        {
            return true;
        }
    }

    env::var("JENKINS_URL")
        .ok()
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}
