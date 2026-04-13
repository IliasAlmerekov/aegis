# Lazy Init for Non-Critical Subsystems Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delay snapshot machinery until a command reaches a snapshot-eligible path, and codify that audit keeps an eager append contract without introducing broad lazy-runtime abstractions.

**Architecture:** Keep config, allowlist, scanner, and user detection eager. Store snapshot settings eagerly but materialize `SnapshotRegistry` on demand inside `RuntimeContext`, and avoid probing applicable snapshot plugins on non-danger paths. For audit, do not invent a helper framework; instead lock in the existing eager logger contract with regression tests and comments so future laziness stays bounded to internals only.

**Tech Stack:** Rust 2024, std `OnceLock`/`LazyLock`, existing `tokio::runtime::Handle`, `src/runtime.rs`, `src/snapshot/mod.rs`, `src/planning/core.rs`, `src/audit/logger.rs`

---

## File Structure

- Modify: `src/runtime.rs`
  - Replace eager `SnapshotRegistry` construction with eager snapshot settings plus lazy registry materialization.
  - Keep `Config::load`, `Allowlist::new`, `scanner_for`, and effective-user detection unchanged.
- Modify: `src/snapshot/mod.rs`
  - Add a small config carrier for lazy runtime registry creation.
  - Add test-only instrumentation helpers to prove when registry construction happens.
- Modify: `src/planning/core.rs`
  - Stop asking for applicable snapshot plugins on non-danger paths.
  - Preserve current policy semantics for `Danger` commands.
- Modify: `src/main.rs`
  - Keep the `#[cfg(test)]` policy-evaluation helper aligned with the new danger-only snapshot plugin lookup.
- Modify: `src/audit/logger.rs`
  - Add regression tests and comments that preserve the eager append contract and prohibit future hidden eager helper work.
- Create: `docs/superpowers/plans/2026-04-13-lazy-init-non-critical-subsystems.md`

## Task 1: Add snapshot lazy-init observability tests

**Files:**
- Modify: `src/snapshot/mod.rs`
- Modify: `src/runtime.rs`
- Modify: `src/planning/core.rs`

- [ ] **Step 1: Add test-only snapshot registry build counters**

```rust
// src/snapshot/mod.rs
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(test)]
static SNAPSHOT_REGISTRY_BUILD_COUNT: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
pub(crate) fn reset_snapshot_registry_build_count_for_tests() {
    SNAPSHOT_REGISTRY_BUILD_COUNT.store(0, Ordering::SeqCst);
}

#[cfg(test)]
pub(crate) fn snapshot_registry_build_count_for_tests() -> usize {
    SNAPSHOT_REGISTRY_BUILD_COUNT.load(Ordering::SeqCst)
}
```

- [ ] **Step 2: Increment the counter inside registry construction**

```rust
// src/snapshot/mod.rs
pub fn from_config(config: &Config) -> Self {
    #[cfg(test)]
    SNAPSHOT_REGISTRY_BUILD_COUNT.fetch_add(1, Ordering::SeqCst);

    use crate::config::SnapshotPolicy;

    let mut plugins: Vec<Box<dyn SnapshotPlugin>> = Vec::new();

    match config.snapshot_policy {
        SnapshotPolicy::None => {}
        SnapshotPolicy::Selective => {
            if config.auto_snapshot_git {
                plugins.push(Box::new(GitPlugin));
            }
            if config.auto_snapshot_docker {
                plugins.push(Box::new(
                    DockerPlugin::new().with_scope(config.docker_scope.clone()),
                ));
            }
        }
        SnapshotPolicy::Full => {
            plugins.push(Box::new(GitPlugin));
            plugins.push(Box::new(
                DockerPlugin::new().with_scope(config.docker_scope.clone()),
            ));
        }
    }

    Self { plugins }
}
```

- [ ] **Step 3: Write a failing runtime test proving `RuntimeContext::new` stays eager except for snapshots**

```rust
// src/runtime.rs
#[test]
fn runtime_context_new_does_not_build_snapshot_registry_eagerly() {
    crate::snapshot::reset_snapshot_registry_build_count_for_tests();

    let mut config = Config::default();
    config.snapshot_policy = SnapshotPolicy::Selective;
    config.auto_snapshot_git = true;
    config.auto_snapshot_docker = false;

    let _context = RuntimeContext::new(config, test_handle()).unwrap();

    assert_eq!(crate::snapshot::snapshot_registry_build_count_for_tests(), 0);
}
```

- [ ] **Step 4: Write a failing planning test proving safe commands do not trigger snapshot probing**

```rust
// src/planning/core.rs
#[test]
fn safe_command_plan_does_not_materialize_snapshot_registry() {
    crate::snapshot::reset_snapshot_registry_build_count_for_tests();

    let mut config = Config::default();
    config.mode = Mode::Protect;
    config.snapshot_policy = SnapshotPolicy::Selective;
    config.auto_snapshot_git = true;
    config.auto_snapshot_docker = false;
    let context = RuntimeContext::new(config, test_handle()).unwrap();

    let outcome = super::plan_with_context(
        &context,
        super::PlanningRequest {
            command: "echo hello",
            cwd_state: CwdState::Resolved(std::path::PathBuf::from(".")),
            transport: ExecutionTransport::Shell,
            ci_detected: false,
        },
    );

    let PlanningOutcome::Planned(plan) = outcome else {
        panic!("safe command must produce a normal plan");
    };
    assert_eq!(plan.snapshot_plan(), SnapshotPlan::NotRequired);
    assert_eq!(crate::snapshot::snapshot_registry_build_count_for_tests(), 0);
}
```

- [ ] **Step 5: Write a failing planning test proving danger commands still probe snapshots once needed**

```rust
// src/planning/core.rs
#[test]
fn danger_command_plan_materializes_snapshot_registry_once() {
    crate::snapshot::reset_snapshot_registry_build_count_for_tests();

    let original_cwd = std::env::current_dir().unwrap();
    let workspace = TempDir::new().unwrap();
    Command::new("git")
        .arg("init")
        .current_dir(workspace.path())
        .output()
        .unwrap();
    std::env::set_current_dir(workspace.path()).unwrap();

    let mut config = Config::default();
    config.mode = Mode::Protect;
    config.snapshot_policy = SnapshotPolicy::Selective;
    config.auto_snapshot_git = true;
    config.auto_snapshot_docker = false;
    let context = RuntimeContext::new(config, test_handle()).unwrap();

    let outcome = super::plan_with_context(
        &context,
        super::PlanningRequest {
            command: "terraform destroy -target=module.prod.api",
            cwd_state: CwdState::Unavailable,
            transport: ExecutionTransport::Shell,
            ci_detected: false,
        },
    );

    std::env::set_current_dir(original_cwd).unwrap();

    let PlanningOutcome::Planned(plan) = outcome else {
        panic!("danger command must produce a normal plan");
    };
    assert!(matches!(plan.snapshot_plan(), SnapshotPlan::Required { .. }));
    assert_eq!(crate::snapshot::snapshot_registry_build_count_for_tests(), 1);
}
```

- [ ] **Step 6: Run targeted tests to verify they fail**

Run:

```bash
rtk cargo test runtime_context_new_does_not_build_snapshot_registry_eagerly -- --nocapture
rtk cargo test safe_command_plan_does_not_materialize_snapshot_registry -- --nocapture
rtk cargo test danger_command_plan_materializes_snapshot_registry_once -- --nocapture
```

Expected: FAIL because `RuntimeContext` still constructs `SnapshotRegistry::from_config(...)` eagerly and `plan_with_context` still probes plugins on safe paths.

- [ ] **Step 7: Commit the failing tests**

```bash
rtk git add src/snapshot/mod.rs src/runtime.rs src/planning/core.rs
rtk git commit -m "test: add snapshot lazy init guards"
```

## Task 2: Implement lazy snapshot registry materialization in runtime

**Files:**
- Modify: `src/snapshot/mod.rs`
- Modify: `src/runtime.rs`

- [ ] **Step 1: Add a snapshot-specific runtime config carrier**

```rust
// src/snapshot/mod.rs
#[derive(Debug, Clone)]
pub struct SnapshotRegistryConfig {
    pub snapshot_policy: crate::config::SnapshotPolicy,
    pub auto_snapshot_git: bool,
    pub auto_snapshot_docker: bool,
    pub docker_scope: crate::config::DockerScope,
}

impl From<&Config> for SnapshotRegistryConfig {
    fn from(config: &Config) -> Self {
        Self {
            snapshot_policy: config.snapshot_policy,
            auto_snapshot_git: config.auto_snapshot_git,
            auto_snapshot_docker: config.auto_snapshot_docker,
            docker_scope: config.docker_scope.clone(),
        }
    }
}
```

- [ ] **Step 2: Rename eager registry construction to use the new config carrier**

```rust
// src/snapshot/mod.rs
impl SnapshotRegistry {
    pub fn from_config(config: &Config) -> Self {
        Self::from_runtime_config(&SnapshotRegistryConfig::from(config))
    }

    pub fn from_runtime_config(config: &SnapshotRegistryConfig) -> Self {
        #[cfg(test)]
        SNAPSHOT_REGISTRY_BUILD_COUNT.fetch_add(1, Ordering::SeqCst);

        use crate::config::SnapshotPolicy;

        let mut plugins: Vec<Box<dyn SnapshotPlugin>> = Vec::new();

        match config.snapshot_policy {
            SnapshotPolicy::None => {}
            SnapshotPolicy::Selective => {
                if config.auto_snapshot_git {
                    plugins.push(Box::new(GitPlugin));
                }
                if config.auto_snapshot_docker {
                    plugins.push(Box::new(
                        DockerPlugin::new().with_scope(config.docker_scope.clone()),
                    ));
                }
            }
            SnapshotPolicy::Full => {
                plugins.push(Box::new(GitPlugin));
                plugins.push(Box::new(
                    DockerPlugin::new().with_scope(config.docker_scope.clone()),
                ));
            }
        }

        Self { plugins }
    }
}
```

- [ ] **Step 3: Replace eager registry storage in `RuntimeContext` with eager config plus lazy `OnceLock`**

```rust
// src/runtime.rs
use std::sync::{Arc, OnceLock};

use crate::snapshot::{SnapshotRecord, SnapshotRegistry, SnapshotRegistryConfig};

pub struct RuntimeContext {
    runtime_config: RuntimeConfig,
    allowlist: Allowlist,
    current_user: Option<String>,
    scanner: Arc<Scanner>,
    snapshot_registry_config: SnapshotRegistryConfig,
    snapshot_registry: OnceLock<SnapshotRegistry>,
    async_handle: Handle,
    audit_logger: AuditLogger,
}
```

- [ ] **Step 4: Initialize the lazy snapshot fields without touching the rest of runtime setup**

```rust
// src/runtime.rs
Ok(Self {
    allowlist: Allowlist::new(&config.layered_allowlist_rules())?,
    snapshot_registry_config: SnapshotRegistryConfig::from(&config),
    snapshot_registry: OnceLock::new(),
    async_handle: handle,
    audit_logger: build_audit_logger(&config),
    current_user,
    runtime_config: RuntimeConfig::from(&config),
    scanner,
})
```

- [ ] **Step 5: Add a narrow helper that materializes the registry on demand**

```rust
// src/runtime.rs
impl RuntimeContext {
    fn snapshot_registry(&self) -> &SnapshotRegistry {
        self.snapshot_registry.get_or_init(|| {
            SnapshotRegistry::from_runtime_config(&self.snapshot_registry_config)
        })
    }
}
```

- [ ] **Step 6: Switch snapshot call sites to the lazy helper**

```rust
// src/runtime.rs
pub fn create_snapshots(&self, cwd: &Path, cmd: &str, _verbose: bool) -> Vec<SnapshotRecord> {
    self.async_handle
        .block_on(self.snapshot_registry().snapshot_all(cwd, cmd))
}

pub fn applicable_snapshot_plugins(&self, cwd: &Path) -> Vec<&'static str> {
    self.snapshot_registry().applicable_plugins(cwd)
}

pub async fn create_snapshots_async(
    &self,
    cwd: &std::path::Path,
    cmd: &str,
) -> Vec<crate::snapshot::SnapshotRecord> {
    self.snapshot_registry().snapshot_all(cwd, cmd).await
}
```

- [ ] **Step 7: Run the targeted snapshot/runtime tests**

Run:

```bash
rtk cargo test runtime_context_new_does_not_build_snapshot_registry_eagerly -- --nocapture
rtk cargo test runtime_context_uses_external_handle_for_snapshots -- --nocapture
rtk cargo test from_config_enables_only_requested_plugins -- --nocapture
rtk cargo test policy_selective_honours_per_plugin_flags -- --nocapture
```

Expected: PASS for runtime construction laziness and existing snapshot behavior.

- [ ] **Step 8: Commit the runtime snapshot laziness**

```bash
rtk git add src/snapshot/mod.rs src/runtime.rs
rtk git commit -m "refactor: lazily build snapshot registry"
```

## Task 3: Gate snapshot applicability lookup to danger-only planning paths

**Files:**
- Modify: `src/planning/core.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Change planning to avoid snapshot plugin lookup on non-danger paths**

```rust
// src/planning/core.rs
let applicable_snapshot_plugins = if assessment.risk == crate::interceptor::RiskLevel::Danger
    && context.config().snapshot_policy != crate::config::SnapshotPolicy::None
{
    match &request.cwd_state {
        CwdState::Resolved(path) => context.applicable_snapshot_plugins(path),
        CwdState::Unavailable => context.applicable_snapshot_plugins(Path::new(".")),
    }
} else {
    Vec::new()
};
```

- [ ] **Step 2: Keep the `main.rs` test helper aligned with the same gate**

```rust
// src/main.rs
let applicable_snapshot_plugins = if assessment.risk == RiskLevel::Danger
    && context.config().snapshot_policy != aegis::config::SnapshotPolicy::None
{
    context.applicable_snapshot_plugins(cwd)
} else {
    Vec::new()
};
```

- [ ] **Step 3: Add a regression test that warn/block paths stay empty without materializing snapshots**

```rust
// src/planning/core.rs
#[test]
fn warn_command_plan_keeps_snapshot_registry_unmaterialized() {
    crate::snapshot::reset_snapshot_registry_build_count_for_tests();

    let mut config = Config::default();
    config.mode = Mode::Protect;
    config.snapshot_policy = SnapshotPolicy::Selective;
    config.auto_snapshot_git = true;
    let context = RuntimeContext::new(config, test_handle()).unwrap();

    let outcome = super::plan_with_context(
        &context,
        super::PlanningRequest {
            command: "git stash clear",
            cwd_state: CwdState::Resolved(std::path::PathBuf::from(".")),
            transport: ExecutionTransport::Shell,
            ci_detected: false,
        },
    );

    let PlanningOutcome::Planned(plan) = outcome else {
        panic!("warn command must produce a normal plan");
    };
    assert_eq!(plan.snapshot_plan(), SnapshotPlan::NotRequired);
    assert_eq!(crate::snapshot::snapshot_registry_build_count_for_tests(), 0);
}
```

- [ ] **Step 4: Run the focused planning tests**

Run:

```bash
rtk cargo test safe_command_plan_does_not_materialize_snapshot_registry -- --nocapture
rtk cargo test warn_command_plan_keeps_snapshot_registry_unmaterialized -- --nocapture
rtk cargo test danger_command_plan_materializes_snapshot_registry_once -- --nocapture
rtk cargo test unavailable_cwd_uses_legacy_snapshot_plugin_fallback_in_plan -- --nocapture
```

Expected: PASS, with safe/warn paths leaving the counter at zero and danger paths still producing the same snapshot plan as before.

- [ ] **Step 5: Commit the planning gate**

```bash
rtk git add src/planning/core.rs src/main.rs
rtk git commit -m "refactor: defer snapshot probing until danger paths"
```

## Task 4: Codify the eager audit contract and prohibit hidden eager helper work

**Files:**
- Modify: `src/audit/logger.rs`

- [ ] **Step 1: Add a regression test that logger construction does not touch the filesystem**

```rust
// src/audit/logger.rs
#[test]
fn new_does_not_create_files_or_directories() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("nested/audit.jsonl"));

    assert!(!logger.path().exists());
    assert!(!logger.lock_path().exists());
}
```

- [ ] **Step 2: Add a regression test that append still follows the existing contract with no eager helper activation**

```rust
// src/audit/logger.rs
#[test]
fn append_creates_parent_and_writes_entry_without_prebuilt_helpers() {
    let temp = tempfile::TempDir::new().unwrap();
    let log_path = temp.path().join("nested/audit.jsonl");
    let logger = AuditLogger::new(&log_path);

    let entry = AuditEntry::new(
        "echo hello".to_string(),
        RiskLevel::Safe,
        Vec::new(),
        Decision::Approved,
        Vec::new(),
        None,
        None,
    );

    logger.append(entry).unwrap();

    assert!(log_path.exists());
    let contents = std::fs::read_to_string(log_path).unwrap();
    assert!(contents.contains("\"command\":\"echo hello\""));
}
```

- [ ] **Step 3: Add a contract comment to keep future audit laziness bounded to internals**

```rust
// src/audit/logger.rs
/// Build an audit logger from validated config without touching the filesystem.
///
/// This eager constructor establishes the append/query contract only.
/// Future lazy work must remain internal helper activation and must not move
/// the append-only write path itself behind a hidden first-use lifecycle.
pub fn from_audit_config(config: &AuditConfig) -> Self {
    let logger = Self::default().with_integrity_mode(config.integrity_mode);
    if let Some(policy) = AuditRotationPolicy::from_config(config) {
        logger.with_rotation(policy)
    } else {
        logger
    }
}
```

- [ ] **Step 4: Run the focused audit tests**

Run:

```bash
rtk cargo test new_does_not_create_files_or_directories -- --nocapture
rtk cargo test append_creates_parent_and_writes_entry_without_prebuilt_helpers -- --nocapture
rtk cargo test rotation_keeps_archives_and_queries_span_them -- --nocapture
```

Expected: PASS, showing logger creation stays side-effect free while append and rotation behavior remain unchanged.

- [ ] **Step 5: Commit the audit guardrails**

```bash
rtk git add src/audit/logger.rs
rtk git commit -m "test: codify eager audit logger contract"
```

## Task 5: Final verification and bench note

- [ ] **Step 1: Run formatter**

Run:

```bash
rtk cargo fmt
```

Expected: exits 0 and rewrites files if needed.

- [ ] **Step 2: Run targeted library tests for the touched modules**

Run:

```bash
rtk cargo test runtime_context_new_does_not_build_snapshot_registry_eagerly -- --nocapture
rtk cargo test runtime_context_uses_external_handle_for_snapshots -- --nocapture
rtk cargo test safe_command_plan_does_not_materialize_snapshot_registry -- --nocapture
rtk cargo test warn_command_plan_keeps_snapshot_registry_unmaterialized -- --nocapture
rtk cargo test danger_command_plan_materializes_snapshot_registry_once -- --nocapture
rtk cargo test new_does_not_create_files_or_directories -- --nocapture
rtk cargo test append_creates_parent_and_writes_entry_without_prebuilt_helpers -- --nocapture
```

Expected: PASS

- [ ] **Step 3: Run full verification required for touched areas**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
```

Expected: all commands exit 0

- [ ] **Step 4: Run the scanner benchmark to confirm this change did not accidentally pull new work into the hot path**

Run:

```bash
rtk cargo bench --bench scanner_bench
```

Expected: benchmark completes; record in the implementation summary that scanner hot-path behavior was unchanged and snapshot work moved behind lazy runtime access.

- [ ] **Step 5: Commit the finished rollout**

```bash
rtk git status --short
```

Expected: no output, because each task above committed its own changes and the tree is clean after verification.
