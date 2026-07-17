// Shared no-source command corpus (Iteration 0 RED #3).
//
// Included verbatim by both `tests/no_source.rs` (as a module) and
// `benches/no_source_bench.rs` (via `include!`) so the contract test and the
// performance-regression bench exercise the *exact same* commands. Living in a
// `tests/common/` subdirectory keeps Cargo from compiling it as its own
// integration-test target while still letting the sibling `benches/` file
// reach it by absolute path. Regular comments (not `//!`) so it stays valid
// both as a module and when `include!`d mid-file.
//
// A safe command never exposes inline interpreter source, so language-aware
// analysis must not start for any of them: `source_targets` returns an empty
// vector and `worker::analyze` returns `Outcome::NotStarted`.

/// Commands that expose no analyzable source.
pub const NO_SOURCE: &[&str] = &[
    "ls -la /home/user",
    "echo hello world",
    "cat /etc/hostname",
    "grep -r TODO src/",
    "git status",
    "git log --oneline -20",
    "cargo build --release",
    "npm run test",
    "docker ps -a",
    "kubectl get pods -n production",
];
