# Aegis v1.0 Gate Register

Этот документ — рабочий реестр проверки готовности к v1.0.0.

- Реестр **mutable**: обновляется по мере выполнения gate-pack задач.
- `status` отражает текущее состояние и может стать `stale`, если не обновлён вовремя.
- `blocking_v1` явно показывает, блокирует ли данный gate релиз v1.0 при непрохождении.
- Для динамических gate-ов (fuzz/perf/CI) применяется TTL в днях; по истечению срока gate требует повторной валидации.

## Поля записи

- `gate_id`: уникальный идентификатор (`G1` ... `G11`)
- `title`: формулировка gate
- `status`: `planned | in_progress | blocked | passed | stale`
- `owner`: ответственный человек/роль
- `depends_on`: зависимые gate-ы
- `evidence`: ссылка на тесты/CI/документ/лог (или `TBD`)
- `last_verified_at`: дата последней проверки в ISO-формате `YYYY-MM-DD` или `TBD`
- `ttl_days`: срок актуальности статуса (в днях)
- `blocking_v1`: `true`, если непрохождение gейта блокирует v1.0
- `next_owner`: кто следующим завершает или перепроверяет
- `notes`: уточнения и риски

## Release Gates (v1.0.0)

| gate_id | title | status | owner | depends_on | evidence | last_verified_at | ttl_days | blocking_v1 | next_owner | notes |
|---|---|---|---|---|---|---|---:|---|---|---|
| G1 | Parser and scanner fuzzing exists, maintained, and is exercised in CI / release validation | passed | Ilias | none | `fuzz/fuzz_targets/scanner.rs`, `fuzz/Cargo.toml`, `fuzz/corpus/scanner`, `.github/workflows/ci.yml`, `docs/v1/specs/v1.0-pack-01-fuzz-regression.md`; local runs: `LSAN_OPTIONS=detect_leaks=0 cargo +nightly fuzz run parser fuzz/corpus/parser -- -runs=200 -max_len=32768`, `LSAN_OPTIONS=detect_leaks=0 cargo +nightly fuzz run scanner fuzz/corpus/scanner -- -runs=200 -max_len=32768`; CI job `fuzz` with the same commands | 2026-04-15 | 14 | true | Ilias | Pack 01 passed: scanner target added, CI integrated, and no unresolved high/critical findings after triage.
| G2 | Parser/scanner regression coverage includes tricky edge cases and historical failures | passed | Ilias | G1 | `tests/shell_edge_regressions.rs`, `docs/v1/specs/v1.0-pack-01-fuzz-regression.md` | 2026-04-15 | 30 | true | Ilias | Edge-case suite now includes heredoc, inline script, pipes/chains, quotes/escapes, subshell/multiline cases; no open high/critical findings linked to this pack.
| G3 | Safe-path performance expectations are documented and checked against a baseline | passed | Ilias | none | `docs/performance-baseline.md`, `perf/scanner_bench_baseline.toml`, `rtk cargo bench --bench scanner_bench`, `rtk cargo run --quiet --bin aegis_benchcheck -- --baseline perf/scanner_bench_baseline.toml --criterion-root target/criterion`, `rtk cargo bench --bench scanner_bench` result: PASS (1.443 ms, 556.325 µs, 155.723 µs vs baselines 2.800/1.000/0.300 ms) | 2026-04-15 | 14 | true | Ilias | Pack 03: `G3` закрыт; все tracked benchmark-ы `PASS`, regressions на текущем окружении не обнаружены. |
| G4 | Supported platform matrix is documented and tested | passed | Ilias | none | `docs/platform-support.md`, `tests/platform_support_docs.rs` (`platform_support_doc_exists_and_declares_unix_only_matrix`, `readme_links_to_platform_support_policy`), `rtk cargo test --test platform_support_docs` | 2026-04-15 | 30 | true | Ilias | Pack 04: матрица платформ поддерживается и задокументирована; валидация текста документа + тесты покрывают Linux/macOS/WSL2 expectations и Windows unsupported.
| G5 | Config schema, audit log format, and exit-code compatibility promises are documented | passed | Ilias | none | `docs/config-schema.md`, `README.md`, `tests/config_schema_docs.rs` (`config_schema_doc_exists_and_describes_versioning_and_migration`, `readme_links_to_config_schema_policy`), `tests/contracts_docs.rs` (`config_schema_contract_covers_exit_code_compatibility`), `rtk cargo test --test config_schema_docs`, `rtk cargo test --test contracts_docs` | 2026-04-15 | 90 | true | Ilias | G5 закрыт в Pack 05 после синхронизации docs/config-schema.md и покрытий тестами (`tests/contracts_docs.rs`). Зафиксированы совместимые контракты для `schema_version`, JSON output и exit-code поведения.
| G6 | Threat model, limitations, and non-goals are current and honest | passed | Ilias | none | `docs/threat-model.md`, `README.md`, `docs/architecture-decisions.md`, `tests/contracts_docs.rs` (`threat_model_is_current_and_documents_non_goals_honestly`, `readme_links_to_contract_docs`), `rtk cargo test --test contracts_docs` | 2026-04-15 | 90 | true | Ilias | G6 закрыт в Pack 05: threat-model содержит явные ограничения, резидуальные риски и не-цели в согласованном виде с `docs/architecture-decisions.md`; README дублирует link.
| G7 | Snapshot and rollback flows are fail-closed and regression-tested across providers | passed | Ilias | G2 | `src/snapshot/git.rs` (`rollback_rejects_malformed_snapshot_id`, `rollback_errors_when_stash_entry_not_found`), `src/snapshot/docker.rs` (`rollback_fails_on_malformed_snapshot_id`, `rollback_fails_when_image_is_missing_from_runtime`), `src/snapshot/postgres.rs` (`rollback_errors_on_malformed_snapshot_id`, `rollback_errors_when_dump_file_missing`), `src/snapshot/mysql.rs` (`rollback_errors_on_malformed_id`, `rollback_errors_when_dump_file_missing`), `src/snapshot/sqlite.rs` (`rollback_errors_when_snapshot_id_is_malformed`, `rollback_errors_when_dump_file_missing`), `src/snapshot/supabase.rs` (`rollback_rejects_malformed_snapshot_id`, `rollback_denies_when_manifest_dump_is_missing`), `tests/full_pipeline.rs` (`rollback_with_known_provider_but_malformed_id_fails_closed`, `rollback_with_unknown_plugin_is_rejected_without_fallback`, `rollback_with_malformed_project_config_fails_closed_instead_of_falling_back`, `rollback_with_malformed_project_config_uses_standard_config_load_error_format`), `tests/snapshot_integration.rs` (git rollback e2e coverage) ; evidence commands: `rtk cargo test --lib`, `rtk cargo test --test full_pipeline rollback_`, `rtk cargo test --test snapshot_integration` | 2026-04-15 | 30 | true | Ilias | Pack 02 закрыт: matrix provider fail-closed regression-кейсов закрыт unit/CLI проверками; snapshot policy для rollback сохраняет все нужные settings. |
| G8 | Release artifacts are checksumed and verifiable | passed | Ilias | none | `scripts/install.sh` (`download` + `.sha256` download + `verify_downloaded_binary`), `tests/installer_flow.rs` (`install_script_rejects_checksum_mismatch_before_touching_bindir`, `install_script_rejects_missing_checksum_before_touching_bindir`, `install_script_falls_back_to_shasum_when_sha256sum_is_missing`, `install_script_fails_when_no_supported_checksum_tool_exists`), `rtk cargo test --test installer_flow` | 2026-04-15 | 30 | true | Ilias | Pack 04: artifact integrity is verified via installer tests; checksum path is verified before installation and fail-closed on missing/mismatched checksum.
| G9 | Install, upgrade, and uninstall flows are documented and validated | passed | Ilias | G8 | `README.md` (install/uninstall sections + troubleshooting link), `tests/installer_flow.rs` (`install_script_configures_shell_wrapper_block_once`, `install_script_prefers_aegis_real_shell_when_shell_already_points_to_wrapper`, `install_script_rejects_checksum_mismatch_before_touching_bindir`, `install_script_rejects_missing_checksum_before_touching_bindir`, `install_script_fails_when_no_supported_checksum_tool_exists`, `install_script_falls_back_to_shasum_when_sha256sum_is_missing`, `uninstall_script_removes_managed_block_and_binary`), `rtk cargo test --test installer_flow` | 2026-04-15 | 30 | true | Ilias | Pack 04: install/upgrade/reinstall и uninstall покрыты unit-тестами и документированы.
| G10 | CI, security, and dependency-policy gates pass consistently | passed | Ilias | G1, G3, G4, G8, G9 | `rtk cargo fmt --check`, `rtk cargo clippy -- -D warnings`, `rtk cargo test`, `rtk cargo audit`, `rtk cargo deny check` (локальный proof bundle), `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `docs/ci.md`, `docs/performance-baseline.md` (см. `docs/v1/specs/v1.0-pack-03-perf-ci.md`) | 2026-04-15 | 14 | true | Ilias | Pack 03 завершён и G10 переведён в `passed`: локальные проверки зелёные; остаётся под TTL=14 для периодической валидации и подтверждения стабильности.
| G11 | Troubleshooting and recovery guidance exists for common operational failures | passed | Ilias | G7, G9 | `docs/troubleshooting.md`, `docs/platform-support.md`, `docs/threat-model.md`; `README.md` link; includes shell-wrapper, checksum, rollback recovery snippets | 2026-04-15 | 90 | false | Ilias | Pack 04: создан эксплуатационный runbook; common installer/snapshot failure paths теперь имеют recovery команды и next-actions.

## Статус gate-пакетов (для планирования)

### Execution order

1. **Pack 01**: `G1 + G2` (parser/scanner fuzzing + regression expansions)
2. **Pack 02**: `G7` (snapshot/rollback fail-closed invariants)
3. **Pack 03**: `G3 + G10` (perf + critical pipeline reliability)
4. **Pack 04**: `G4 + G8 + G9 + G11` (release/ops reliability)
5. **Pack 05**: `G5 + G6` (contracts/docs)

## Процедура обновления статуса

1. При любом изменении, относящемся к gate, обновить `evidence` и `last_verified_at`.
2. Для `planned -> in_progress -> passed`:
   - зафиксировать тесты, команды и CI run identifiers;
   - добавить примечания по остаточным рискам.
3. Если подтверждение старше `ttl_days`, перевести статус в `stale` до нового прогона.
4. Если gate зависит от неразрешённого high/critical finding — статус `blocked`.
5. Если gate не закрыт и `blocking_v1 = true`, решение о переносе на v1.1 должно быть явно зафиксировано в `notes` и подтверждено человеком.

## Примеры критериев «passed» для критичных gate-ов

- **G1**: артефакты fuzz запуска есть, severity triage выполнен, unresolved high/critical ⇒ `blocked`.
- **G2**: regression suite расширен на перечисленные классы edge-cases и все тесты green с документированными негативными кейсами.
- **G8**: есть проверяемые artifact (checksum + инструкцию верификации + журнал ручной/автоматической проверки).
- **G10**: устойчивый тренд выполнения CI-гейтов по последнему релизному каналу.
