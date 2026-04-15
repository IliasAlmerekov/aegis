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
| G1 | Parser and scanner fuzzing exists, maintained, and is exercised in CI / release validation | planned | TBD | none | TBD | TBD | 14 | true | TBD | Пакет: **Pack 01**. Для Pass требуется обработка всех high/critical findings.
| G2 | Parser/scanner regression coverage includes tricky edge cases and historical failures | planned | TBD | G1 | TBD | TBD | 30 | true | TBD | Depends on Pack 01 and parser/scanner hardening tasks.
| G3 | Safe-path performance expectations are documented and checked against a baseline | planned | TBD | none | TBD | TBD | 14 | true | TBD | Baseline/thresholds из `docs/performance-baseline.md` должны иметь актуальную проверку.
| G4 | Supported platform matrix is documented and tested | planned | TBD | none | TBD | TBD | 30 | true | TBD | Включает Linux/macOS и ожидания по WSL2.
| G5 | Config schema, audit log format, and exit-code compatibility promises are documented | planned | TBD | none | TBD | TBD | 90 | true | TBD | Связано с Track B; обновление при любых изменениях контрактов.
| G6 | Threat model, limitations, and non-goals are current and honest | planned | TBD | none | TBD | TBD | 90 | true | TBD | Не может быть застрявшим после изменения scanner/parser/snapshot behavior.
| G7 | Snapshot and rollback flows are fail-closed and regression-tested across providers | planned | TBD | G2 | TBD | TBD | 30 | true | TBD | Пакет с инвариантами fail-closed для snapshot/rollback.
| G8 | Release artifacts are checksumed and verifiable | planned | TBD | none | TBD | TBD | 30 | true | TBD | Требует верификации end-to-end install artifact checks.
| G9 | Install, upgrade, and uninstall flows are documented and validated | planned | TBD | G8 | TBD | TBD | 30 | true | TBD | Включает позитивные и негативные сценарии конфигурации shell-обёртки.
| G10 | CI, security, and dependency-policy gates pass consistently | planned | TBD | G1, G3, G4, G8, G9 | TBD | TBD | 14 | true | TBD | Не только green-статус, но и стабильные артефакты.
| G11 | Troubleshooting and recovery guidance exists for common operational failures | planned | TBD | G7, G9 | TBD | TBD | 90 | false | TBD | Некритичный по сроку/релизу, но обязательный для production usability.

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
