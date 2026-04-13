# Registry-Based Extension Points Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Clarify Aegis's existing pattern, snapshot, and allowlist extension boundaries so contributors can identify source-of-truth, provenance, effective view, and runtime/materialized view without introducing a broad new registry abstraction.

**Architecture:** Keep this rollout thin and local. First align docs and facade vocabulary across `patterns`, `snapshot`, and `allowlist`, then do targeted cleanup in order: patterns, snapshot, allowlist. Preserve current semantics throughout: merged pattern sets still feed scanner construction, snapshot availability stays distinct from applicability, and allowlist advisory analysis remains non-authoritative relative to compiled/runtime matching.

**Tech Stack:** Rust 2024, existing `PatternSet`, `SnapshotRegistry`, layered config/allowlist model, `rtk cargo test`, `rtk cargo bench --bench scanner_bench`

---

## File Structure

- Modify: `src/interceptor/patterns.rs`
  - Clarify `PatternSet` as the effective merged pattern boundary.
  - Add thin facade accessors so scanner-facing code consumes the merged set through one explicit API.
- Modify: `src/interceptor/mod.rs`
  - Keep `scanner_for` / `assess_with_custom_patterns` as the canonical scanner-facing consumers of the effective merged pattern set.
- Modify: `src/interceptor/scanner/mod.rs`
  - Switch any direct `PatternSet` field access to the clarified facade.
- Modify: `src/snapshot/mod.rs`
  - Clarify “available providers” vs config-filtered runtime set vs applicability.
  - Add thin helper methods that expose those boundaries without changing policy behavior.
- Modify: `src/config/model.rs`
  - Document the provenance/effective-view boundary for layered allowlist rules.
- Modify: `src/config/allowlist.rs`
  - Clarify authoritative compile/match APIs versus advisory analysis APIs.
  - Add a thin compile facade if needed to make the effective rule boundary explicit.
- Create: `docs/superpowers/plans/2026-04-13-registry-based-extension-points.md`

## Task 1: Thin docs/API-first alignment pass

**Files:**
- Modify: `src/interceptor/patterns.rs`
- Modify: `src/snapshot/mod.rs`
- Modify: `src/config/model.rs`
- Modify: `src/config/allowlist.rs`

- [ ] **Step 1: Add thin descriptive docs to `PatternSet` and its main constructors**

```rust
// src/interceptor/patterns.rs
/// Effective merged pattern set consumed by scanner construction.
///
/// This type is the scanner-facing source of truth after built-in patterns and
/// config-supplied custom patterns have been merged and validated.
#[derive(Debug)]
pub struct PatternSet {
    pub patterns: Vec<Arc<Pattern>>,
}

impl PatternSet {
    /// Load the canonical built-in pattern source only.
    pub fn load() -> Result<PatternSet, AegisError> {
        Self::from_sources(&[])
    }

    /// Build the effective merged pattern set for scanner construction.
    ///
    /// Merge order and duplicate-id rules are authoritative here, so scanner
    /// consumers should depend on this merged view rather than branching on
    /// built-in vs custom sources themselves.
    pub fn from_sources(custom_patterns: &[UserPattern]) -> Result<PatternSet, AegisError> {
        let file: PatternsFile = toml::from_str(BUILTIN_PATTERNS_TOML)
            .map_err(|e| AegisError::Config(format!("failed to parse patterns.toml: {e}")))?;

        let builtin_patterns: Vec<Pattern> = file.patterns.into_iter().map(Pattern::from).collect();
        let custom_patterns: Vec<Pattern> =
            custom_patterns.iter().cloned().map(Pattern::from).collect();

        let mut ids: HashSet<String> =
            HashSet::with_capacity(builtin_patterns.len() + custom_patterns.len());
        let mut patterns: Vec<Arc<Pattern>> =
            Vec::with_capacity(builtin_patterns.len() + custom_patterns.len());

        for pattern in builtin_patterns
            .into_iter()
            .chain(custom_patterns.into_iter())
        {
            Self::validate_pattern(&pattern, &mut ids)?;
            patterns.push(Arc::new(pattern));
        }

        Ok(PatternSet { patterns })
    }
}
```

- [ ] **Step 2: Add thin descriptive docs to snapshot registry boundaries**

```rust
// src/snapshot/mod.rs
/// Holds the config-filtered runtime snapshot provider set.
///
/// "Available providers" means providers known to this binary/runtime.
/// This registry stores the subset materialized for the current runtime config.
/// Applicability to a specific command/cwd is evaluated later.
pub struct SnapshotRegistry {
    plugins: Vec<Box<dyn SnapshotPlugin>>,
}

/// Eager runtime config used to materialize a config-filtered snapshot provider set.
#[derive(Debug, Clone)]
pub struct SnapshotRegistryConfig {
    pub snapshot_policy: crate::config::SnapshotPolicy,
    pub auto_snapshot_git: bool,
    pub auto_snapshot_docker: bool,
    pub docker_scope: crate::config::DockerScope,
}
```

- [ ] **Step 3: Document layered allowlist provenance and the effective view boundary**

```rust
// src/config/model.rs
/// Return the precedence-resolved allowlist input annotated with its source layer.
///
/// This is the canonical provenance-preserving input to allowlist compilation.
/// Matching semantics are defined later by `src/config/allowlist.rs`.
pub(crate) fn layered_allowlist_rules(&self) -> Vec<LayeredAllowlistRule> {
    self.allowlist
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, rule)| {
            let source_layer = self
                .allowlist_layers
                .get(index)
                .copied()
                .unwrap_or(AllowlistSourceLayer::Project);

            LayeredAllowlistRule { rule, source_layer }
        })
        .collect()
}
```

- [ ] **Step 4: Clarify authoritative vs advisory allowlist APIs**

```rust
// src/config/allowlist.rs
/// Compiled effective allowlist view used for authoritative runtime matching.
#[derive(Debug, Clone, Default)]
pub struct Allowlist {
    project_entries: Vec<CompiledAllowlistRule>,
    global_entries: Vec<CompiledAllowlistRule>,
}

/// Produce non-authoritative quality warnings for one structured allowlist rule.
///
/// This advisory analysis does not participate in runtime allow/deny matching
/// and must not be treated as a replacement for compiled rule evaluation.
pub fn analyze_allowlist_rule(rule: &AllowlistRule) -> Vec<AllowlistWarning> {
    let mut warnings = Vec::new();
    let location = warning_location(rule);

    if !has_scope(rule.cwd.as_deref()) && !has_scope(rule.user.as_deref()) {
        warnings.push(AllowlistWarning {
            code: "missing_scope",
            message: "allowlist rule has no cwd or user scope".to_string(),
            location: location.clone(),
        });
    }

    if is_broad_pattern(rule.pattern.trim()) {
        warnings.push(AllowlistWarning {
            code: "broad_pattern",
            message: "allowlist rule uses wildcard matching that may be broader than intended"
                .to_string(),
            location,
        });
    }

    warnings
}
```

- [ ] **Step 5: Run thin-pass regression checks**

Run:

```bash
rtk cargo test load_builtin_patterns_parses_without_error -- --nocapture
rtk cargo test from_config_enables_only_requested_plugins -- --nocapture
rtk cargo test project_layer_beats_global_layer_when_both_match -- --nocapture
```

Expected: PASS, confirming the documentation/alignment pass did not change behavior.

- [ ] **Step 6: Commit the thin alignment pass**

```bash
rtk git add src/interceptor/patterns.rs src/snapshot/mod.rs src/config/model.rs src/config/allowlist.rs
rtk git commit -m "docs: align extension point boundaries"
```

## Task 2: Make the patterns boundary explicit for scanner consumers

**Files:**
- Modify: `src/interceptor/patterns.rs`
- Modify: `src/interceptor/scanner/mod.rs`
- Modify: `src/interceptor/mod.rs`

- [ ] **Step 1: Add a thin accessor for the effective merged pattern set**

```rust
// src/interceptor/patterns.rs
impl PatternSet {
    /// Return the effective merged pattern set consumed by scanner construction.
    pub fn patterns(&self) -> &[Arc<Pattern>] {
        self.patterns.as_slice()
    }
}
```

- [ ] **Step 2: Make `PatternSet` storage private and switch scanner construction to the accessor**

```rust
// src/interceptor/patterns.rs
pub struct PatternSet {
    patterns: Vec<Arc<Pattern>>,
}
```

```rust
// src/interceptor/scanner/mod.rs
let compiled: Vec<(Arc<Pattern>, Regex)> = patterns
    .patterns()
    .iter()
    .map(|p| {
        let rx = Regex::new(&p.pattern)
            .unwrap_or_else(|e| panic!("invalid regex in pattern {}: {e}", p.id));
        (Arc::clone(p), rx)
    })
    .collect();

for pattern in patterns.patterns() {
    let kws = extract_keywords(&pattern.pattern);
    if kws.is_empty() {
        has_uncovered = true;
    } else {
        keywords.extend(kws);
    }
}
```

- [ ] **Step 3: Add a regression test that pins the canonical scanner-facing merged-pattern boundary**

```rust
// src/interceptor/mod.rs
#[test]
fn assess_with_custom_patterns_uses_the_effective_merged_pattern_set() {
    let custom = crate::config::UserPattern {
        id: "USR-REG-001".to_string(),
        category: crate::interceptor::patterns::Category::Cloud,
        risk: RiskLevel::Warn,
        pattern: "internal-teardown".to_string(),
        description: "Internal teardown guard".to_string(),
        safe_alt: Some("internal-teardown --dry-run".to_string()),
    };

    let assessment = assess_with_custom_patterns("internal-teardown", &[custom]).unwrap();

    assert_eq!(assessment.risk, RiskLevel::Warn);
    assert!(assessment
        .matched
        .iter()
        .any(|m| m.pattern.source == crate::interceptor::patterns::PatternSource::Custom));
}
```

- [ ] **Step 4: Run focused pattern/scanner tests**

Run:

```bash
rtk cargo test from_sources_merges_builtin_and_custom_and_marks_custom_source -- --nocapture
rtk cargo test from_sources_rejects_duplicate_ids_between_builtin_and_custom -- --nocapture
rtk cargo test assess_with_custom_patterns_uses_the_effective_merged_pattern_set -- --nocapture
```

Expected: PASS, showing scanner-facing behavior still depends on the merged effective set and not on source-specific branching outside `patterns.rs`.

- [ ] **Step 5: Commit the patterns cleanup**

```bash
rtk git add src/interceptor/patterns.rs src/interceptor/scanner/mod.rs src/interceptor/mod.rs
rtk git commit -m "refactor: clarify pattern set boundary"
```

## Task 3: Clarify snapshot availability vs materialization vs applicability

**Files:**
- Modify: `src/snapshot/mod.rs`

- [ ] **Step 1: Add a thin helper for providers known to the binary/runtime**

```rust
// src/snapshot/mod.rs
const BUILTIN_SNAPSHOT_PROVIDER_NAMES: &[&str] = &["git", "docker"];

impl SnapshotRegistry {
    /// Return built-in providers known to this binary/runtime.
    pub fn available_provider_names() -> &'static [&'static str] {
        BUILTIN_SNAPSHOT_PROVIDER_NAMES
    }
}
```

- [ ] **Step 2: Add a thin helper for the config-filtered materialized provider set**

```rust
// src/snapshot/mod.rs
impl SnapshotRegistry {
    /// Return provider names materialized for the current runtime config.
    pub fn configured_provider_names(&self) -> Vec<&'static str> {
        self.plugins.iter().map(|plugin| plugin.name()).collect()
    }
}
```

- [ ] **Step 3: Clarify `applicable_plugins` as a later-stage runtime-use check**

```rust
// src/snapshot/mod.rs
/// Return the subset of configured providers applicable to `cwd`.
///
/// This is a later-stage runtime-use question than provider availability or
/// config-driven materialization.
pub fn applicable_plugins(&self, cwd: &Path) -> Vec<&'static str> {
    self.plugins
        .iter()
        .filter(|plugin| plugin.is_applicable(cwd))
        .map(|plugin| plugin.name())
        .collect()
}
```

- [ ] **Step 4: Add regression tests that keep availability distinct from config materialization**

```rust
// src/snapshot/mod.rs
#[test]
fn available_provider_names_report_builtins_independent_of_runtime_config() {
    assert_eq!(SnapshotRegistry::available_provider_names(), &["git", "docker"]);
}

#[test]
fn configured_provider_names_report_only_materialized_runtime_plugins() {
    let mut config = Config::default();
    config.auto_snapshot_git = false;
    config.auto_snapshot_docker = true;

    let registry = SnapshotRegistry::from_config(&config);

    assert_eq!(registry.configured_provider_names(), vec!["docker"]);
}
```

- [ ] **Step 5: Run focused snapshot tests**

Run:

```bash
rtk cargo test available_provider_names_report_builtins_independent_of_runtime_config -- --nocapture
rtk cargo test configured_provider_names_report_only_materialized_runtime_plugins -- --nocapture
rtk cargo test from_config_enables_only_requested_plugins -- --nocapture
rtk cargo test policy_full_enables_all_plugins -- --nocapture
```

Expected: PASS, proving the code now documents and exposes availability, config materialization, and applicability as distinct boundaries without changing policy behavior.

- [ ] **Step 6: Commit the snapshot cleanup**

```bash
rtk git add src/snapshot/mod.rs
rtk git commit -m "refactor: clarify snapshot registry boundaries"
```

## Task 4: Clarify authoritative allowlist matching vs advisory analysis

**Files:**
- Modify: `src/config/allowlist.rs`
- Modify: `src/config/model.rs`
- Modify: `src/runtime.rs`

- [ ] **Step 1: Add a thin explicit compile facade for layered provenance-preserving input**

```rust
// src/config/allowlist.rs
impl Allowlist {
    /// Compile the effective layered allowlist view used for authoritative runtime matching.
    pub fn from_layered_rules<T>(rules: &[T]) -> Result<Self>
    where
        T: Clone + Into<LayeredAllowlistRule>,
    {
        let mut project_entries = Vec::new();
        let mut global_entries = Vec::new();

        for rule in rules.iter().cloned().map(Into::into) {
            let compiled = compile_rule(rule)?;
            match compiled.source_layer {
                AllowlistSourceLayer::Project => project_entries.push(compiled),
                AllowlistSourceLayer::Global => global_entries.push(compiled),
            }
        }

        Ok(Self {
            project_entries,
            global_entries,
        })
    }

    pub fn new<T>(rules: &[T]) -> Result<Self>
    where
        T: Clone + Into<LayeredAllowlistRule>,
    {
        Self::from_layered_rules(rules)
    }
}
```

- [ ] **Step 2: Switch runtime/config call sites to the explicit compile facade**

```rust
// src/runtime.rs
allowlist: Allowlist::from_layered_rules(&config.layered_allowlist_rules())?,
```

```rust
// src/config/model.rs
Allowlist::from_layered_rules(&self.layered_allowlist_rules()).map(|_| ())?;
```

- [ ] **Step 3: Add a regression test that advisory analysis stays non-authoritative**

```rust
// src/config/allowlist.rs
#[test]
fn advisory_warnings_do_not_override_authoritative_runtime_matching() {
    let rule = AllowlistRule {
        pattern: "terraform destroy *".to_string(),
        cwd: Some("/srv/infra".to_string()),
        user: None,
        expires_at: None,
        reason: "scoped teardown".to_string(),
    };

    let warnings = analyze_allowlist_rule(&rule);
    let allowlist = Allowlist::from_layered_rules(&[rule]).unwrap();

    assert!(warnings.iter().any(|w| w.code == "broad_pattern"));
    assert_eq!(
        allowlist
            .match_reason(&ctx("terraform destroy -target=module.test.api"))
            .map(|m| m.reason),
        Some("scoped teardown".to_string())
    );
}
```

- [ ] **Step 4: Run focused allowlist tests**

Run:

```bash
rtk cargo test project_layer_beats_global_layer_when_both_match -- --nocapture
rtk cargo test warning_flags_broad_rule_without_scope -- --nocapture
rtk cargo test advisory_warnings_do_not_override_authoritative_runtime_matching -- --nocapture
```

Expected: PASS, preserving layered precedence and runtime match behavior while clearly keeping advisory analysis secondary.

- [ ] **Step 5: Commit the allowlist cleanup**

```bash
rtk git add src/config/allowlist.rs src/config/model.rs src/runtime.rs
rtk git commit -m "refactor: clarify allowlist rule boundaries"
```

## Task 5: Final verification and performance note

- [ ] **Step 1: Run formatter**

Run:

```bash
rtk cargo fmt
```

Expected: exits 0 and rewrites files if needed.

- [ ] **Step 2: Run focused regression tests for each zone**

Run:

```bash
rtk cargo test assess_with_custom_patterns_uses_the_effective_merged_pattern_set -- --nocapture
rtk cargo test configured_provider_names_report_only_materialized_runtime_plugins -- --nocapture
rtk cargo test advisory_warnings_do_not_override_authoritative_runtime_matching -- --nocapture
```

Expected: PASS

- [ ] **Step 3: Run full verification for touched areas**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
```

Expected: all commands exit 0

- [ ] **Step 4: Run the scanner benchmark because the patterns cleanup touches a scanner-facing boundary**

Run:

```bash
rtk cargo bench --bench scanner_bench
```

Expected: benchmark completes; record that scanner behavior still consumes the effective merged pattern set and that the change clarified the boundary rather than altering hot-path semantics.

- [ ] **Step 5: Confirm the tree is clean after the staged commits**

Run:

```bash
rtk git status --short
```

Expected: no output, because each task above committed its own changes and the tree is clean after verification.
