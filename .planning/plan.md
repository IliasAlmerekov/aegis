# Plan: Snapshot prune (M1.2)

## Goal

Add `aegis snapshot prune` so operators can bound snapshot artifact growth across
all six providers while preserving Aegis’s append-only audit log contract.

Done when:

- `SnapshotPlugin` has an idempotent `delete(snapshot_id)` method implemented
  for git, docker, postgres, mysql, sqlite, and supabase.
- A retention policy (max count per provider + max age across all providers) can
  be configured in `aegis.toml`.
- `aegis snapshot prune` defaults to a dry-run preview, requires `--yes` to
  actually delete artifacts, and appends a `Pruned` audit entry for every
  removed snapshot.
- `aegis snapshot list` hides or marks snapshot ids that have been pruned.
- Unit + integration tests prove: outside the retention window/count is deleted,
  inside is preserved, idempotent delete succeeds when the artifact is already
  gone, and the audit log gains only append-only prune records.

## Key design decisions

1. **Deletion lives in `SnapshotPlugin`, not a separate trait.**
   All six providers already know how to destroy their own artifacts, and the
   registry already works with `Box<dyn SnapshotPlugin>`. A new method keeps the
   object-safe vtable simple and avoids an extra generic boundary.

2. **Audit log stays append-only.**
   Prune does not rewrite or delete log lines. Instead it appends a new audit
   entry whose `command` is `aegis prune <snapshot_id>` and whose `decision` is
   `Decision::Pruned` (new variant). `list`/`rollback` cross-reference these
   records to hide dangling ids.

3. **Retention semantics are conservative (union).**
   A snapshot is kept if it is within the per-provider newest `max_count` OR
   younger than the global `max_age`. Only snapshots that fail both rules are
   eligible for prune. This reduces the chance of removing something a user
   might still want to roll back.

4. **Clock is injected for deterministic tests.**
   `SystemTime::now()`/`OffsetDateTime::now_utc()` make age-based tests flaky.
   Introduce a small `Clock` trait with `SystemClock` and `FixedClock`
   implementations. The prune service accepts `&dyn Clock`.

5. **No auto-prune in M1.2.**
   The done-when is a manual CLI command. Auto-prune after each snapshot is
   intentionally out of scope; it changes the runtime flow and deserves its own
   milestone.

## Files to change

### `aegis-types`

- `src/lib.rs` (or existing `Decision` enum module)
  - Add `Decision::Pruned` variant.
  - Update serde helpers and any `Display`/match sites.

### `aegis-audit`

- `src/logger.rs`
  - Add `PruneEntry` helper / public API to build a prune record.
  - Keep using `AuditEntry::new(...)` with the new `Decision::Pruned`.
- `src/logger/writer.rs`
  - Add `AuditEntry::new_prune(command, snapshot)` constructor if useful.
  - No breaking change to `append` path.

### `aegis-config`

- `src/model.rs`
  - Add `prune: PruneConfig` section to `AegisConfig` and `PartialConfig`.
  - Add `PruneConfig` struct with `max_count_per_provider: Option<usize>`,
    `max_age_days: Option<u32>`, `enabled: bool`.
  - Update `AegisConfig::defaults`, `merge_layer`, `INIT_TEMPLATE`.
- `src/snapshot.rs`
  - (If needed) no changes; retention is a top-level concern, not per-provider.

### `aegis-snapshot`

- `src/error.rs`
  - Add `SnapshotError::DeleteFailed { plugin, snapshot_id, source }`.
- `src/lib.rs`
  - Add `async fn delete(&self, snapshot_id: &str) -> Result<()>` to
    `SnapshotPlugin`.
  - Add `SnapshotRegistry::delete(plugin_name, snapshot_id)` dispatcher.
  - Add `SnapshotRegistry::prunable_records(...)` helper used by CLI prune.
- `src/git.rs`
  - Implement `delete`: parse `<cwd>\t<hash>`, locate stash ref by hash, run
    `git stash drop <ref>`. Treat "stash entry not found" as `Ok(())`.
- `src/docker/mod.rs`
  - Implement `delete`: parse JSONL records, run `docker rmi <image>` for each
    record. Treat missing image as `Ok(())`.
- `src/sqlite.rs`
  - Implement `delete`: parse snapshot_id, remove dump file. Treat
    `NotFound` as `Ok(())`.
- `src/postgres/mod.rs`, `src/mysql/mod.rs`
  - Implement `delete`: parse snapshot_id, remove dump file. Treat `NotFound`
    as `Ok(())`.
- `src/supabase/mod.rs` + `src/supabase/runtime.rs`
  - Implement `delete`: parse manifest path, remove manifest + dump file, then
    remove the bundle directory if empty. Treat `NotFound` as `Ok(())`.

### `aegis` (root binary)

- `src/main.rs`
  - Add `SnapshotCommand::Prune(PruneArgs)`.
  - Add `PruneArgs { yes: bool, dry_run: bool }`.
- `src/cli_dispatch.rs`
  - Wire `Commands::Snapshot(args)` to a new `prune::execute` async fn.
- `src/prune.rs` (new file)
  - `execute(args, runtime) -> i32`: load config, build registry, compute
    candidates, preview, prompt/confirm, delete, append prune audit entries.
- `src/cli_commands.rs`
  - Update `handle_snapshot_command` to call the new prune handler.
  - Optionally update `format_snapshot_listing` to hide/mask pruned ids by
    scanning for `Decision::Pruned` records (can be a follow-up if out of scope).

## Detailed implementation steps

### Step 1 — Types and errors

1. Add `Decision::Pruned` to `aegis-types`.
2. Add `SnapshotError::DeleteFailed` to `aegis-snapshot`.
3. Run `rtk cargo check` after each crate change.

### Step 2 — Config

1. Define `PruneConfig` in `aegis-config/src/model.rs`:

   ```rust
   #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
   #[serde(default, deny_unknown_fields)]
   pub struct PruneConfig {
       pub enabled: bool,
       pub max_count_per_provider: Option<usize>,
       pub max_age_days: Option<u32>,
   }
   ```

2. Add `prune: PruneConfig` to `AegisConfig` and `PartialConfig`.
3. Update defaults, merge, and `INIT_TEMPLATE` comments.

### Step 3 — SnapshotPlugin `delete`

1. Extend the trait:

   ```rust
   #[async_trait]
   pub trait SnapshotPlugin: Send + Sync {
       fn name(&self) -> &'static str;
       async fn is_applicable(&self, cwd: &Path) -> bool;
       async fn snapshot(&self, cwd: &Path, cmd: &str) -> Result<String>;
       async fn rollback(&self, snapshot_id: &str) -> Result<()>;
       async fn delete(&self, snapshot_id: &str) -> Result<()>;
   }
   ```

2. Implement for each provider. Common rules:
   - Parse the same snapshot_id format used by `rollback`.
   - If the artifact is already missing, return `Ok(())`.
   - On backend command failure, return `SnapshotError::DeleteFailed`.
   - Never panic on malformed input; reuse existing parse errors.

### Step 4 — Registry helpers

1. `SnapshotRegistry::delete(&self, plugin_name, snapshot_id)` mirrors
   `rollback`.
2. Add a `PrunableRecord { plugin, snapshot_id, recorded_at }` type.
3. Add `SnapshotRegistry::resolve_prunable_records(...)` that:
   - Reads the audit log.
   - Collects every recorded `AuditSnapshot` keyed by `(plugin, snapshot_id)`,
     keeping the latest `recorded_at`.
   - Subtracts ids recorded in later `Decision::Pruned` entries.
   - Returns the remaining set grouped by plugin.

### Step 5 — Retention service

1. Add a `Clock` trait and implementations in `aegis-snapshot/src/lib.rs` or a
   new `clock.rs`:

   ```rust
   pub trait Clock: Send + Sync {
       fn now(&self) -> OffsetDateTime;
   }
   ```

2. Add `RetentionPolicy::apply(records, now) -> Vec<PrunableRecord>`:
   - Per-provider: sort by `recorded_at` desc, keep newest `max_count`.
   - Global age: keep records where `now - recorded_at < max_age_days`.
   - Union of the two keep sets; the rest are prune candidates.

### Step 6 — CLI prune command

1. `src/prune.rs`:

   ```rust
   pub(crate) async fn execute(args: PruneArgs) -> Result<Vec<PrunedTarget>> {
       let config = AegisConfig::load()?;
       let prune_config = &config.prune;
       if !prune_config.enabled && !args.yes {
           // Inform user that prune is disabled and --yes is required
       }
       let logger = AuditLogger::from_audit_config(&config.audit);
       let registry = SnapshotRegistry::for_rollback()?; // all providers
       let records = registry.resolve_prunable_records(&logger).await?;
       let candidates = RetentionPolicy::from_config(prune_config)
           .apply(&records, &SystemClock);

       if args.dry_run || (!args.yes && !args.dry_run) {
           preview_candidates(&candidates);
           return Ok(Vec::new());
       }

       let mut pruned = Vec::new();
       for record in candidates {
           match registry.delete(&record.plugin, &record.snapshot_id).await {
               Ok(()) => {
                   append_prune_audit_entry(&logger, &record)?;
                   pruned.push(record);
               }
               Err(e) => {
                   tracing::warn!(plugin=%record.plugin, id=%record.snapshot_id, error=%e,
                       "prune delete failed");
                   // Continue; prune is best-effort per artifact.
               }
           }
       }
       Ok(pruned)
   }
   ```

2. Default CLI behavior:
   - `aegis snapshot prune` → dry-run preview, exit 0.
   - `aegis snapshot prune --yes` → execute.
   - `aegis snapshot prune --dry-run` → explicit preview.
   - If `--yes` and `--dry-run` both supplied, error.

### Step 7 — Update `snapshot list`

1. In `format_snapshot_listing`, collect `Pruned` snapshot ids from the audit
   log.
2. Exclude pruned ids from the normal list.
3. Optionally print a one-line note if any ids were hidden.

### Step 8 — Tests

#### Unit tests in `aegis-snapshot`

- `delete` for each provider:
  - Removes existing artifact.
  - Returns `Ok(())` when artifact already missing.
  - Returns error on malformed snapshot_id.
- `RetentionPolicy`:
  - `age_only_keeps_recent`.
  - `count_only_keeps_newest_n_per_provider`.
  - `union_keeps_any_record_that_passes_either_rule`.
  - `empty_input_yields_empty_candidates`.

#### Integration tests in `aegis` root

- `tests/snapshot_prune_git.rs`:
  - Create temp git repo with several stashes.
  - Seed audit log entries at fixed timestamps using `FixedClock`.
  - Run prune with retention that should keep N and delete the rest.
  - Assert kept stashes still exist via `git stash list`.
  - Assert deleted stashes are gone.
  - Assert audit log ends with prune entries.

- CLI behavior tests in `src/prune.rs` tests or `tests/cli_prune.rs`:
  - Default invocation previews but does not delete.
  - `--yes` deletes.
  - `--dry-run` conflicts with `--yes`.

#### Existing tests must still pass

- `rtk cargo test --workspace`
- `rtk cargo clippy --all-targets --all-features --locked -- -D warnings`
- `rtk cargo criterion` (scanner benchmarks; prune path does not touch hot path)

## Open questions / risks

1. **Provider delete idempotency on partial failures.**
   Docker rollback currently ignores failed `docker rm`. Delete should be
   similarly best-effort, but we must log and surface unexpected failures.

2. **Supabase bundle cleanup.**
   Deleting only `manifest.json` + `artifacts/db.dump` leaves an empty bundle
   directory. Decide whether to remove the directory if it becomes empty.
   Proposal: remove it, as it is Aegis-owned storage.

3. **`Decision` enum is public API.**
   Adding `Pruned` is a breaking change for consumers that match exhaustively.
   `Decision` is not currently marked `#[non_exhaustive]`. If we want to avoid
   downstream breakage, consider adding that attribute now and document it.

4. **Integrity chain interaction.**
   Prune audit entries participate in the hash chain just like any other append.
   No special handling required, but we should verify `verify_integrity` still
   passes after pruning.

## Implementation order

1. Types + errors (`aegis-types`, `aegis-snapshot` error).
2. Config (`aegis-config`).
3. `SnapshotPlugin::delete` implementations (one provider at a time, start with
   git because it has the best existing test harness).
4. Registry helpers + Clock + RetentionPolicy.
5. CLI wiring + `src/prune.rs`.
6. Update `snapshot list`.
7. Tests and clippy cleanup.
