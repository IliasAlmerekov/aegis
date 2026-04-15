# Supabase Snapshot Provider Design (Phase 1 / v1)

Date: 2026-04-15
Status: Approved design for planning
Scope: Add a new `supabase` snapshot provider to Aegis as a project-level provider with a strict DB-only manifest capability in v1.

## 1. Objective

Introduce a new built-in snapshot provider named `supabase` alongside the existing `git`, `docker`, `postgres`, `mysql`, and `sqlite` providers.

Phase 1 intentionally does **not** implement full Supabase project backup. Instead, it establishes the public `supabase` provider contract and delivers:

- explicit-config-only applicability
- direct PostgreSQL transport for snapshot/rollback
- a bundle-based snapshot artifact consisting of:
  - a JSON manifest
  - a PostgreSQL dump artifact
- strict DB-only rollback
- fail-closed behavior throughout

This keeps the public provider/domain correct now while leaving room for later expansion to Storage, Edge Functions, and project-level config exports.

## 2. Domain Boundary

The provider split is intentional and must stay explicit:

- `postgres` = PostgreSQL database backup/restore provider
- `supabase` = project-level Supabase provider

The `supabase` provider may reuse internal PostgreSQL snapshot/restore logic, but it must not inherit the public meaning of `postgres`. The `supabase` provider is a separate snapshot domain with its own config namespace, manifest format, and rollback rules.

## 3. Phase 1 Scope

### Included

- new built-in provider: `supabase`
- explicit config gate only
- direct PostgreSQL connection settings under `supabase_snapshot.db`
- DB dump artifact captured into a Supabase-specific snapshot bundle
- JSON manifest named `manifest.json`
- opaque, versioned `snapshot_id` that resolves to the bundle manifest
- strict DB-only rollback
- checksum verification before rollback
- audit/test/doc updates required to support the provider honestly

### Excluded

- Storage bucket/object capture
- Edge Functions capture
- Auth/project config export
- Supabase CLI/API dependency
- cwd-based project auto-discovery
- partial restore orchestration
- any “best effort rollback anyway” behavior

Publicly, the provider name is `supabase`, but the effective captured scope in v1 is **db-only manifest snapshot**.

## 4. Fail-Closed Contract

The provider must preserve Aegis’ fail-closed model:

- no manifest = no snapshot
- partial or degraded snapshots are never eligible for strict rollback
- `rollback.allowed` is derived from invariant checks, never a manual trust flag
- rollback recomputes derived fields instead of trusting persisted summary fields
- if persisted derived fields disagree with recomputed values, rollback fails closed
- config is never the source of truth for rollback target selection
- rollback never attempts a “best effort” restore when strict eligibility fails

## 5. Applicability Model

`SupabasePlugin::is_applicable()` in v1 is gated by explicit config only.

It should evaluate:

- whether the provider is enabled by runtime policy/config
- whether `supabase_snapshot` config is present and valid
- whether required tooling assumptions are satisfied

It must **not** rely on local Supabase project markers such as `supabase/config.toml` to enable the provider.

It must also **not** require runtime reachability of the DB target at applicability time. For v1, applicability checks tooling presence plus config validity, but does not prove that the target is reachable right now.

## 6. Config Shape

Phase 1 adds a separate Supabase config namespace:

```toml
snapshot_policy = "Selective"
auto_snapshot_supabase = false

[supabase_snapshot]
project_ref = ""
require_config_target_match_on_rollback = true

[supabase_snapshot.db]
database = ""
host = "localhost"
port = 5432
user = ""
```

### Config Rules

- `auto_snapshot_supabase` enables the provider in `Selective` mode.
- In `Full` mode, the provider may still be selected by policy, but must fail closed if the Supabase config is incomplete or invalid.
- `project_ref` is advisory-only metadata for audit/UI/future phases. It is not an auth source and not an applicability trigger.
- `require_config_target_match_on_rollback` is a fail-closed safety switch. When true, current config must match the manifest target or rollback is denied.
- Credentials must not be stored in config. They are supplied externally through PostgreSQL tooling conventions such as `PGPASSWORD`, `~/.pgpass`, and related libpq environment variables.

## 7. Snapshot Bundle Layout

Each Supabase snapshot is a bundle rooted under the Aegis snapshot directory:

```text
~/.aegis/snapshots/supabase-<timestamp>-<id>/
  manifest.json
  artifacts/
    db.dump
```

Rules:

- `manifest.json` is the canonical manifest filename.
- artifact paths stored in the manifest are relative to the manifest directory.
- rollback resolves artifact paths relative to the manifest, canonicalizes them, and rejects paths that escape the bundle root.

## 8. Snapshot ID Contract

The `supabase` provider returns an opaque, versioned `snapshot_id`.

Rules:

- the ID must not expose a plain manifest path as the public contract
- the ID must version its encoding format
- the ID must resolve to the canonical `manifest.json` inside the snapshot bundle
- the manifest is the source of truth for rollback

Phase 1 should prefer the same style as the existing DB providers: a versioned opaque string that internally encodes the manifest reference without introducing a separate snapshot index.

## 9. Manifest Format (JSON)

The manifest is a runtime artifact, not hand-edited configuration, so JSON is the canonical format.

### Required high-level shape

```json
{
  "manifest_version": 1,
  "provider": "supabase",
  "created_at": "2026-04-15T12:34:56Z",
  "capabilities": {
    "db": true,
    "storage": false,
    "functions": false,
    "project_config": false
  },
  "target": {
    "project_ref": "abcxyz123",
    "db": {
      "database": "postgres",
      "host": "db.supabase.co",
      "port": 5432,
      "user": "postgres"
    }
  },
  "artifacts": {
    "db": {
      "present": true,
      "complete": true,
      "path": "artifacts/db.dump",
      "format": "postgres.custom",
      "checksum_sha256": "…",
      "size_bytes": 1234567
    },
    "storage": {
      "captured": false,
      "status": "not_captured"
    },
    "functions": {
      "captured": false,
      "status": "not_captured"
    },
    "project_config": {
      "captured": false,
      "status": "not_captured"
    }
  },
  "rollback": {
    "db_supported": true,
    "allowed": true,
    "config_target_match_required": true,
    "reasons": []
  },
  "partial": false,
  "degraded": false,
  "warnings": [],
  "errors": [],
  "overall_status": "complete"
}
```

### Manifest Rules

- `manifest_version` is required.
- `provider` must equal `"supabase"`.
- `target.db` is required in v1 because rollback is strict DB-only.
- `artifacts.db.path` is required only when `artifacts.db.present = true`.
- `artifacts.db.checksum_sha256` is required in v1 when the DB artifact is present.
- `overall_status` is derived and limited to:
  - `complete`
  - `partial`
  - `degraded`
  - `failed`
- machine logic must rely on invariant checks and recomputation, not on persisted summary fields alone.

## 10. Derived Field Semantics

Two fields are user-facing summaries, not trusted authority:

- `rollback.allowed`
- `overall_status`

During rollback, Aegis must recompute strict eligibility from manifest invariants. If the persisted summary fields do not match the recomputed result, rollback is denied.

`partial` and `degraded` always block strict rollback eligibility, even if the physical DB dump still exists.

## 11. Snapshot Flow

`SupabasePlugin::snapshot()` in v1 creates a strict DB-manifest snapshot.

### Snapshot sequence

1. Preflight validation
2. Create bundle directory and `artifacts/`
3. Create DB dump artifact at `artifacts/db.dump`
4. Compute SHA-256 checksum and size metadata
5. Build manifest model
6. Atomically commit `manifest.json`
7. Return opaque `snapshot_id`

### Preflight validation requirements

Preflight must fail closed unless all of the following hold:

- Supabase config is valid
- DB target descriptor is valid
- `pg_dump` is available for snapshot creation
- `pg_restore` is available to satisfy strict rollback-path eligibility

This preflight verifies tooling presence and config validity. It does **not** require proving target reachability or performing a live restore.

### Atomic manifest write

Manifest persistence is mandatory and atomic in v1:

- write to a temporary file in the same directory
- flush/fsync the file
- rename to `manifest.json`
- fsync the parent directory where supported

Platform contract:

- when parent-directory fsync is supported, it is required
- where unsupported or inapplicable, the implementation must perform a documented skip rather than silently ignoring it

### Success criteria

A snapshot is successful only if:

- bundle directory exists
- DB dump completed successfully
- checksum was computed successfully
- manifest is valid
- atomic manifest write completed
- recomputed invariants yield:
  - `artifacts.db.present = true`
  - `artifacts.db.complete = true`
  - `rollback.db_supported = true`
  - `rollback.allowed = true`
  - `partial = false`
  - `degraded = false`
  - `overall_status = complete`

If dump creation succeeds but manifest commit fails, the snapshot is still considered failed.

## 12. Snapshot Failure Semantics

### Preflight failure

Examples:

- invalid config
- invalid DB target descriptor
- missing `pg_dump`
- missing `pg_restore`

Result:

- no snapshot is created
- no `snapshot_id` is returned
- no successful snapshot record is emitted

### Dump/checksum failure

Examples:

- dump command fails
- dump output is unusable
- checksum computation fails

Result:

- snapshot fails closed
- manifest is not committed as a successful snapshot
- dump artifact is removed best effort
- partially created bundle content is cleaned best effort

### Manifest commit failure

Examples:

- temporary manifest write fails
- flush/fsync fails
- rename fails
- required parent-directory fsync fails on a platform that supports it

Result:

- snapshot fails closed
- manifest is treated as not committed
- dump artifact is removed best effort
- residue is reported honestly if cleanup also fails

## 13. Rollback Flow

`SupabasePlugin::rollback(snapshot_id)` performs strict DB-only rollback in v1.

### Rollback sequence

1. Decode and validate `snapshot_id`
2. Load and parse `manifest.json`
3. Validate manifest schema and recompute strict eligibility
4. Resolve DB artifact path relative to the manifest directory
5. Verify artifact integrity via SHA-256
6. Optionally compare current config target with manifest target
7. Restore using the manifest target descriptor
8. Report success only if the full restore succeeds

### Source of truth

Rollback uses the manifest target as the source of truth. Current config is only an optional fail-closed safety check when `require_config_target_match_on_rollback = true`.

### Strict rollback eligibility

Rollback is allowed only if recomputation proves all of the following:

- `provider == "supabase"`
- `manifest_version == 1`
- `target.db` is present and valid
- `artifacts.db.present = true`
- `artifacts.db.complete = true`
- `artifacts.db.path` is present
- `artifacts.db.checksum_sha256` is present
- `rollback.db_supported = true`
- derived `rollback.allowed = true`
- `partial = false`
- `degraded = false`
- derived `overall_status = complete`

### Integrity verification

Before restore, Aegis must recompute the SHA-256 of the resolved DB artifact and compare it to the manifest checksum.

Checksum mismatch is a separate hard failure class:

- `artifact integrity verification failed`

This is not downgraded into a softer “artifact missing/corrupt” branch that still attempts restore.

## 14. Rollback Denial Semantics

Rollback must fail closed, without attempting restore, when any of the following occur:

- manifest missing or unreadable
- manifest path decodes but escapes bundle boundaries
- malformed manifest
- wrong provider
- unsupported manifest version
- missing or invalid `target.db`
- missing artifact
- checksum missing
- checksum mismatch
- `partial = true`
- `degraded = true`
- recomputed derived fields disagree with persisted summary values
- config/manifest target mismatch when target matching is required

In these cases Aegis returns an explicit rollback denial, such as:

- `rollback not permitted for this snapshot`

## 15. Testing Strategy

### Unit tests

Add focused tests for:

- manifest schema validation
- derived-field recomputation
- `target.db` required in v1
- `checksum_sha256` required when DB artifact is present
- `partial/degraded => rollback denied`
- opaque `snapshot_id` encode/decode
- relative path resolution and bundle boundary validation

### Provider tests

Add provider-level tests for:

- explicit-config-only applicability
- applicability checks tooling presence plus config validity, not target reachability
- missing `pg_dump` => fail closed
- missing `pg_restore` => fail closed
- successful snapshot creates bundle, artifact, manifest, checksum
- manifest commit failure triggers failed snapshot plus cleanup attempt
- checksum mismatch denies rollback with integrity-verification failure
- config-target mismatch denies rollback when safety check is enabled
- manifest present but recomputed derived fields disagree with persisted values => rollback denied

### Integration / end-to-end tests

Add higher-level tests for:

- Supabase provider participating in snapshot plan when configured
- successful snapshot surfacing as `plugin = "supabase"` in audit output
- rollback through a valid `snapshot_id`
- malformed/foreign manifest denial
- partial/degraded rollback denial

### Security regression tests

Cover fail-closed scenarios such as:

- dump exists but manifest missing
- manifest exists but checksum missing
- checksum mismatch
- provider/version mismatch
- manifest summary lies about strict eligibility
- artifact path attempts to escape snapshot bundle root

## 16. Minimal Code Footprint

### New code

- `src/snapshot/supabase.rs`
- optional helper module for manifest schema/validation if it materially improves clarity

### Existing modules to update

- `src/snapshot/mod.rs`
  - register and materialize the `supabase` provider
- `src/config/model.rs`
  - add `auto_snapshot_supabase`
  - add Supabase config structs under `supabase_snapshot`
- `docs/config-schema.md`
  - document the new provider config honestly
- relevant tests under `tests/` and module-local test blocks

Audit integration should reuse existing snapshot record structures unless new fields are strictly necessary.

## 17. Documentation Requirements

Docs must state clearly that:

- `supabase` is the project-level provider name
- in Phase 1, its effective captured scope is **db-only manifest snapshot**
- rollback in v1 is **strict DB-only**
- this is not full Supabase project restore yet

Documentation changes should cover at least:

- `docs/config-schema.md`
- `README.md` if it lists supported snapshot providers or capabilities
- any architecture/provider lists that enumerate built-in snapshot providers

## 18. Expansion Path

The v1 manifest intentionally reserves room for future sections:

- `artifacts.storage`
- `artifacts.functions`
- `artifacts.project_config`

Planned future phases:

1. `supabase_db_manifest` — establish the provider, bundle, manifest, and strict DB-only rollback
2. `supabase_storage` — add Storage capture and rollback orchestration
3. `supabase_full` — add functions/project config and composite restore orchestration

This design keeps the public provider identity stable now while allowing later capability expansion without renaming the provider or breaking the manifest domain model.
