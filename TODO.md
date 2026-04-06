# Aegis — Production Readiness TODO

Цель: довести проект до production-grade уровня как безопасного CLI guardrail для команд AI/пользователя.

## Принципы планирования

- Сначала устраняем **contract drift** между конфигом, CLI и фактическим runtime-поведением.
- Затем усиливаем **policy correctness** и убираем опасные silent fallback.
- Потом улучшаем **execution model**, чтобы не ломать уже сделанные policy-решения.
- После этого усиливаем **security hardening**, UX, observability и performance.
- Порядок фаз обязателен: ряд более поздних задач зависит от архитектурных решений ранних фаз.
- Внутри фаз тикеты отсортированы так, чтобы минимизировать переделки.

---

# P1 — Contract Integrity (КРИТИЧЕСКОЕ)

> Цель: привести публичный контракт проекта в соответствие с фактическим поведением.
>
> Почему это первая фаза: пока конфиг, CLI и runtime не совпадают, любые дальнейшие улучшения будут строиться на ложной модели системы.

---

## Ticket 1.1 — Подключить `custom_patterns` к Scanner (Done)

### Проблема

В конфиге заявлена поддержка `custom_patterns`, но runtime-сканер фактически использует только встроенные паттерны. Это создаёт ложное ожидание у пользователя и подрывает доверие к policy engine.

### Что нужно сделать

- Вынести сборку итогового набора паттернов в отдельный pipeline:
  1. загрузка builtin patterns,
  2. загрузка user-defined patterns из `Config`,
  3. нормализация структуры,
  4. валидация,
  5. компиляция в runtime `PatternSet`.
- Добавить корректную маркировку `PatternSource::Custom`.
- Обеспечить единый интерфейс для scanner, чтобы ему было всё равно, builtin это или custom pattern.
- Запретить ситуацию, когда custom patterns загружены в config, но не применены при оценке.

### Технические шаги

- Добавить `PatternSet::from_sources(...)` или аналогичный builder.
- Добавить конвертацию `UserPattern -> Pattern`.
- Решить политику при конфликте `id`:
  - либо ошибка,
  - либо жёсткое правило запрета дубликатов.
- Явно документировать порядок: builtin + custom.

### Acceptance criteria

- Custom pattern реально влияет на `Assessment`.
- В audit и UI отображается source=`custom`.
- Есть unit tests на merge.
- Есть integration test: конфиг с custom pattern меняет итоговую классификацию.

### Риск, если не сделать

- Пользователь считает, что система защищает от его кастомных сценариев, хотя этого не происходит.

---

## Ticket 1.2 — Ввести единый `RuntimeContext` (Done)

### Проблема

Сейчас разные части системы создают зависимости через `.default()` или локальную инициализацию. Это ведёт к тому, что config не управляет всем runtime консистентно.

### Что нужно сделать

Создать единый контейнер runtime-зависимостей, например:

```rust
struct RuntimeContext {
    config: Config,
    scanner: Scanner,
    snapshot_registry: SnapshotRegistry,
    policy_engine: PolicyEngine,
}
```

### Цель

- Убрать рассинхронизацию между тем, что загрузилось из конфига, и тем, что реально используется в момент принятия решения.
- Централизовать инициализацию всех subsystem.

### Технические шаги

- В `main` строить `RuntimeContext` один раз.
- Передавать контекст в:
  - command assessment,
  - decision flow,
  - snapshot handling,
  - audit append pipeline.
- Убрать скрытое создание scanner/snapshot registry внутри helper-функций.
- Минимизировать использование глобальных singleton-инициализаций там, где нужен config-aware runtime.

### Acceptance criteria

- В runtime-коде нет неявного создания scanner/snapshot registry в обход конфига.
- Все ключевые path-и используют один и тот же context.
- Локальные helper-функции принимают контекст или его части как dependency.

### Риск, если не сделать

- Последующие фазы опять будут страдать от “конфиг есть, но на поведение не влияет”.

---

## Ticket 1.3 — Сделать `SnapshotRegistry` config-aware (Done)

### Проблема

Флаги `auto_snapshot_git` и `auto_snapshot_docker` есть, но registry всё равно регистрирует плагины по умолчанию. Это прямое нарушение пользовательского контракта.

### Что нужно сделать

- Перевести `SnapshotRegistry::default()` в безопасный internal-only вариант.
- Добавить `SnapshotRegistry::from_config(&Config)`.
- Включать Git/Docker snapshot plugins только если это разрешено config.

### Технические шаги

- Явно определить mapping:
  - `auto_snapshot_git = true` → GitPlugin active
  - `auto_snapshot_docker = true` → DockerPlugin active
- Подготовить registry к будущим plugin-типам.
- Обновить decision flow, чтобы snapshots создавались через config-aware registry.

### Acceptance criteria

- При `auto_snapshot_git=false` Git snapshot не создаётся.
- При `auto_snapshot_docker=false` Docker snapshot не создаётся.
- Есть integration tests на оба флага.
- UI и audit корректно отражают только реально созданные snapshots.

### Риск, если не сделать

- Snapshot subsystem останется непредсказуемой и опасной операционно.

---

## Ticket 1.4 — Реализовать `Mode` полностью (Done)

### Проблема

`Protect`, `Audit`, `Strict` задекларированы, но фактически не определяют runtime-поведение. Это мёртвый API.

### Что нужно сделать

Реализовать чёткую semantics:

- `Protect`
  - текущее ожидаемое поведение: prompt/block/log.
- `Audit`
  - никогда не блокирует,
  - всегда логирует,
  - может печатать предупреждение,
  - useful for dry-run adoption.
- `Strict`
  - максимально жёсткий режим,
  - deny everything кроме `Safe`,
  - allowlist-override должен быть отдельно управляемым и явно ограниченным.

### Технические шаги

- Вынести mode-handling в decision layer, а не размазывать по UI.
- Документировать влияние mode на:
  - interactive mode,
  - CI,
  - allowlist,
  - snapshots,
  - audit logging.

### Acceptance criteria

- Значение `mode` реально меняет `Decision`.
- Есть тесты для всех трёх режимов.
- `config init` и `config show` отражают реальное supported behavior.

### Риск, если не сделать

- Конфиг остаётся частично декоративным, а продукт — непредсказуемым.

---

## ~~Ticket 1.5 — Удалить или реализовать `watch`~~ ✅ ЗАКРЫТ

`aegis watch` реализован: читает NDJSON-фреймы из stdin, прогоняет через полный pipeline (assess → policy → snapshots → /dev/tty dialog → audit → execute), стримит NDJSON-события обратно в stdout. 287 тестов проходят.

---

## Ticket 1.6 — Инициализировать `tracing_subscriber`

### Проблема

Система использует `tracing`, но оператор может не видеть warnings/errors, особенно связанных с malformed config или snapshot failures.

### Что нужно сделать

- Инициализировать `tracing_subscriber` в `main`.
- Настроить уровни логирования:
  - default = info или warn,
  - verbose = debug.
- Привязать это к CLI-флагу `--verbose`.

### Технические шаги

- Добавить init в bootstrap phase.
- Исключить двойную инициализацию.
- Подумать о JSON-режиме логов в будущей фазе, но сейчас достаточно plain text.

### Acceptance criteria

- Warnings действительно видны пользователю.
- Verbose mode показывает расширенный технический контекст.
- Ошибки конфигурации и snapshot path не теряются.

### Риск, если не сделать

- Silent degradation останется даже после исправления части логики.

---

# P2 — Policy Correctness & Safety

> Цель: сделать policy layer строгим, предсказуемым и безопасным.
>
> Почему после P1: сначала нужно заставить config реально влиять на runtime, и только потом ужесточать саму policy-логику.

---

## Ticket 2.1 — Перевести config loading в fail-fast режим

### Проблема

Сейчас malformed project config может silently пропускаться с fallback на global/default config. Для security tool это опасное поведение: policy может не примениться, а пользователь об этом не узнает.

### Что нужно сделать

- Сделать invalid config по умолчанию fatal error.
- Прервать выполнение с internal/policy error exit code.
- Опционально добавить explicit bypass-флаг для dev/debug use cases.

### Политика

- Production default: **fail closed**.
- Dev override: только явный и сознательный.

### Acceptance criteria

- Битый config не игнорируется silently.
- Пользователь получает понятное сообщение:
  - какой файл,
  - какая ошибка,
  - как исправить.
- Есть integration test на malformed config.

### Риск, если не сделать

- Даже хороший policy engine не имеет смысла, если policy silently не загружается.

---

## Ticket 2.2 — Fail-fast на invalid allowlist entries

### Проблема

Некорректные allowlist patterns сейчас могут silently отбрасываться. Это недопустимо для policy-конфига.

### Что нужно сделать

- Перевести compilation allowlist rules на `Result`, а не `Option`.
- Любая ошибка правила должна:
  - либо валить весь config,
  - либо инвалидировать allowlist целиком.
- Предпочтительный режим: **ошибка загрузки конфига**.

### Acceptance criteria

- Invalid allowlist entry приводит к видимой ошибке.
- Нет silent drop behavior.
- Есть unit tests и config integration tests.

### Риск, если не сделать

- Пользователь может думать, что критическое исключение работает, хотя оно просто проигнорировано.

---

## Ticket 2.3 — Перевести allowlist на структурированную модель

### Проблема

Строковые wildcard-patterns слишком грубые и слишком мощные. Они плохо ограничиваются контекстом и легко становятся опасным bypass-каналом.

### Что нужно сделать

Вместо:

```toml
allowlist = ["terraform destroy *"]
```

перейти к структуре вида:

```toml
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "/srv/infra"
user = "ci"
expires_at = "2026-01-01T00:00:00Z"
reason = "ephemeral test teardown"
```

### Поля

Минимально:

- `pattern`
- `cwd` (optional)
- `user` (optional)
- `expires_at` (optional)
- `reason` (required or strongly recommended)

### Acceptance criteria

- Allowlist rule матчится не только по command string, но и по context.
- Есть serializable/deserializable schema.
- Есть validation и тесты на scope matching.

### Риск, если не сделать

- Allowlist останется самой слабой и самой опасной частью системы.

---

## Ticket 2.4 — Ограничить силу allowlist override

### Проблема

Сейчас allowlist может auto-approve слишком опасные команды. Даже если это осознанный trade-off, production-система должна уметь ограничивать глубину bypass.

### Что нужно сделать

Добавить управляемую политику, например:

```toml
allowlist_override_level = "Warn"
```

Поддерживаемые варианты:

- `Warn`
- `Danger`
- `Never`

### Recommended default

- `Warn`

### Поведение

- `Block` никогда не bypass-ится.
- `Danger` bypass-ится только если это явно разрешено policy.
- `Strict` mode может полностью отключать allowlist override для non-safe commands.

### Acceptance criteria

- Allowlist override level реально влияет на decision flow.
- Есть тесты на Warn/Danger/Block.

### Риск, если не сделать

- Слишком широкий allowlist может превратить guardrail в формальность.

---

## Ticket 2.5 — Добавить `aegis config validate`

### Проблема

Сейчас нет отдельного способа проверить policy до запуска реальной команды.

### Что нужно сделать

Добавить CLI-команду:

```bash
aegis config validate
```

### Что должна проверять

- корректность TOML/schema;
- custom_patterns;
- allowlist rules;
- mode;
- snapshot flags;
- policy conflicts;
- дубликаты ids;
- expired allowlist entries;
- потенциально опасные слишком широкие allowlist rules.

### Дополнительно

- exit code 0 → config valid
- non-zero → config invalid
- человекочитаемый вывод
- опционально `--output json`

### Acceptance criteria

- Команда usable в CI.
- Ошибки и warnings разделены.
- Есть integration tests.

### Риск, если не сделать

- Policy quality остаётся “угадайкой” до реального execution.

---

# P3 — Execution Model & Runtime

> Цель: стабилизировать execution pipeline и убрать архитектурные решения, которые будут мешать дальнейшему развитию.
>
> Почему после P2: сначала policy должна стать строгой, иначе улучшенный runtime будет исполнять всё ту же нестрогую логику.

---

## Ticket 3.1 — Перейти на persistent Tokio runtime

### Проблема

Сейчас runtime может создаваться на каждый snapshot path. Это лишний overhead и плохая основа для дальнейших async subsystem.

### Что нужно сделать

- Инициализировать один Tokio runtime в bootstrap phase.
- Передавать или использовать его централизованно.
- Убрать локальный `build().block_on(...)` из hot path.

### Acceptance criteria

- Runtime создаётся один раз.
- Snapshot path использует persistent runtime.
- Нет повторной инициализации внутри отдельных операций.

### Риск, если не сделать

- Рост latency и сложность развития async-функций.

---

## Ticket 3.2 — Добавить управляемую snapshot policy

### Проблема

Snapshot-логика слишком грубая: она может быть или просто включена, или выключена, но этого мало для production.

### Что нужно сделать

Добавить более точную модель, например:

```toml
snapshot_policy = "none | selective | full"
```

### Семантика

- `none` — never snapshot
- `selective` — только определённые plugin/path/use-case
- `full` — текущая наиболее агрессивная модель

### Acceptance criteria

- Snapshot поведение определяется policy, а не только флагом.
- Есть тесты на все режимы.

### Риск, если не сделать

- Snapshot останется либо слишком дорогим, либо слишком непрозрачным.

---

## Ticket 3.3 — Ограничить scope Docker snapshot

### Проблема

Docker snapshot всех запущенных контейнеров может быть чрезмерно дорогим и операционно неожиданным.

### Что нужно сделать

Поддержать выбор контейнеров:

- по label,
- по whitelist,
- по regex/prefix name,
- возможно по explicit opt-in.

### Recommended default

- snapshot only labeled containers

### Acceptance criteria

- Docker plugin не делает blanket snapshot всех running containers без policy-разрешения.
- Есть tests на selection logic.

### Риск, если не сделать

- Инструмент будет слишком дорогой для реальных хостов и быстро потеряет доверие.

---

## Ticket 3.4 — Добавить `rollback` CLI

### Проблема

Snapshots создаются, но у пользователя нет явного и удобного публичного UX для rollback.

### Что нужно сделать

Добавить:

```bash
aegis rollback <snapshot-id>
```

### Что должна уметь команда

- rollback конкретного snapshot;
- понятные ошибки;
- recovery hints;
- audit logging rollback action.

### Acceptance criteria

- Есть public rollback flow.
- Snapshot IDs из audit можно использовать для восстановления.
- Есть tests минимум для git path.

### Риск, если не сделать

- Snapshot subsystem выглядит недоделанной и малоценной с продуктовой точки зрения.

---

## Ticket 3.5 — Добавить structured JSON output для decision path

### Проблема

Инструмент рассчитан в том числе на automation/AI-agent use cases, но вывод ориентирован в основном на человека.

### Что нужно сделать

Добавить JSON-режим для результатов policy-evaluation:

```bash
aegis -c "rm -rf /tmp" --output json
```

### JSON должен включать

- command
- risk
- decision
- matched patterns
- allowlist match
- snapshots created
- mode
- ci state

### Acceptance criteria

- JSON schema стабильна и документирована.
- Machine consumers могут использовать output без парсинга stderr.
- Есть integration tests.

### Риск, если не сделать

- Интеграция в automation останется хрупкой.

---

# P4 — Security Hardening

> Цель: повысить реальную устойчивость guardrail-а к обходам и усложнить bypass через shell forms.
>
> Почему после P3: сначала нужен стабильный execution model и policy plumbing, иначе hardening будет встраиваться в нестабильную архитектуру.

---

## Ticket 4.1 — Улучшить нормализацию команд перед анализом

### Проблема

Текущий анализ во многом опирается на raw shell string. Это даёт хороший baseline, но не покрывает достаточно глубоко некоторые shell forms.

### Что нужно сделать

Усилить normalization layer:

- quotes
- separators
- subshell groups
- command substitutions
- environment-prefix forms
- multiline input normalization

### Acceptance criteria

- Normalized representation стабильна и тестируема.
- Scanner работает по чётко определённой intermediate model.

### Риск, если не сделать

- Система останется слишком уязвимой к syntactic variations.

---

## Ticket 4.2 — Усилить анализ inline scripts и nested execution

### Проблема

`bash -c`, heredoc, inline interpreters, `eval`, sourced fragments — ключевой bypass surface для heuristic guardrail.

### Что нужно сделать

- Вынести nested execution analysis в отдельный subsystem.
- Рекурсивно анализировать:
  - `bash -c`
  - `sh -c`
  - heredoc bodies
  - inline Python/Node/Perl/Ruby
  - `source <(...)`
  - `eval "$VAR"` и похожие формы

### Acceptance criteria

- Есть формализованный recursive scan path.
- Есть отдельные tests на nested forms.
- Risk повышается на основе содержимого вложенного payload.

### Риск, если не сделать

- Самые очевидные обходы останутся открытыми.

---

## Ticket 4.3 — Усилить `Strict` mode как security policy

### Проблема

Strict mode должен быть не просто “ещё чуть строже”, а реально отдельным policy profile.

### Что нужно сделать

В strict mode блокировать по умолчанию:

- remote execution patterns,
- eval-based execution,
- inline interpreters,
- shell process substitution execution,
- потенциально unknown high-risk forms.

### Acceptance criteria

- Strict mode имеет отдельный documented policy profile.
- Есть tests на набор строго запрещённых execution forms.

### Риск, если не сделать

- Strict mode останется маркетинговой меткой без сильной ценности.

---

## Ticket 4.4 — Добавить более глубокий анализ pipeline semantics

### Проблема

Команды вида `cmd1 | cmd2 | cmd3` опасны не только своими отдельными сегментами, но и смыслом передачи данных между процессами.

### Что нужно сделать

- Улучшить policy analysis для pipeline chains.
- Отдельно распознавать dangerous sinks:
  - `| sh`
  - `| bash`
  - `| xargs rm`
  - передача секретных данных в network sink
- По возможности учитывать левый и правый контекст сегмента.

### Acceptance criteria

- Pipeline risk analysis покрыт тестами.
- Есть корректная классификация dangerous sink patterns.

### Риск, если не сделать

- Некоторые destructive chains будут недооценены.

---

## Ticket 4.5 — Вынести decision logic в `PolicyEngine`

### Проблема

Когда число policy-факторов растёт (mode, ci, allowlist, strictness, snapshots), decision flow в `main.rs` быстро становится хрупким.

### Что нужно сделать

Ввести отдельный компонент:

```rust
trait PolicyEngine {
    fn evaluate(&self, input: PolicyInput) -> PolicyDecision;
}
```

### В `PolicyInput`

- assessment
- mode
- ci state
- allowlist result
- config flags
- execution context

### В `PolicyDecision`

- decision
- rationale
- requires_confirmation
- snapshots_required

### Acceptance criteria

- Decision logic не размазана по `main.rs`.
- Есть unit tests именно на policy layer.
- UI и exec зависят от результата policy engine, а не сами принимают решения.

### Риск, если не сделать

- Последующие изменения будут ломать decision flow и множить скрытые ветки.

---

# P5 — UX Improvements

> Цель: сделать поведение инструмента понятным, безопасным и удобным для реальной эксплуатации.
>
> Почему после P4: сначала нужно зафиксировать security semantics, иначе UX-промпты будут описывать ещё меняющееся поведение.

---

## Ticket 5.1 — Усилить подтверждение Danger-команд

### Проблема

Подтверждение через простое `yes` неидеально: оно лучше, чем `y`, но всё ещё не лучший вариант для реально опасных действий.

### Что нужно сделать

Поддержать один из вариантов:

- ввод полного command fragment,
- ввод случайного токена,
- ввод конкретного matched dangerous fragment.

### Recommended default

- random confirmation token или full dangerous fragment

### Acceptance criteria

- Accidentally approval становится сложнее.
- UX остаётся понятным.
- Есть tests на prompt semantics.

### Риск, если не сделать

- У Danger останется слишком дешёвое подтверждение.

---

## Ticket 5.2 — Сделать Warn prompt безопаснее

### Проблема

Сейчас модель “Enter = approve, n = deny” слишком рискованна и легко приводит к случайному продолжению.

### Что нужно сделать

Перевести Warn prompt на явную схему:

- `y/N`
- Enter = deny по умолчанию

### Acceptance criteria

- Нет implicit approve на пустой Enter.
- Поведение отражено в help и tests.

### Риск, если не сделать

- Даже аккуратные пользователи могут случайно подтверждать Warn-команды.

---

## Ticket 5.3 — Разделить quiet / standard / verbose output modes

### Проблема

Разным сценариям нужен разный объём вывода:

- интерактивный человек,
- CI,
- automation,
- debugging.

### Что нужно сделать

Добавить как минимум:

- `--quiet`
- default mode
- `--verbose`

### Поведение

- `quiet`: минимум сообщений, удобно для scripts
- `verbose`: технические детали, source, pattern ids, config decisions

### Acceptance criteria

- Уровень detail управляется явно.
- Логи не дублируются хаотично между stdout/stderr.

### Риск, если не сделать

- UX останется либо шумным, либо недостаточно информативным.

---

## Ticket 5.4 — Улучшить UX audit-команды

### Проблема

Audit log есть, но доступ к нему ещё не дотягивает до удобного operational tooling.

### Что нужно сделать

Добавить:

- фильтрацию по времени;
- фильтрацию по command substring;
- фильтрацию по decision;
- табличный/human-friendly вывод;
- улучшенный JSON/NDJSON use case.

### Acceptance criteria

- Оператор может быстро ответить на вопросы:
  - что было заблокировано,
  - что auto-approved,
  - какие patterns срабатывали чаще всего.

### Риск, если не сделать

- Audit останется лишь сырой технической функцией.

---

## Ticket 5.5 — Улучшить CI UX и сообщения policy failure

### Проблема

CI path должен быть максимально понятен: automation не должна гадать, почему команда не выполнилась.

### Что нужно сделать

- Уточнить сообщения:
  - blocked by CI policy,
  - blocked by strict mode,
  - blocked by explicit pattern,
  - denied due to non-interactive mode.
- Добавить actionable hints:
  - validate config,
  - inspect allowlist,
  - use JSON output.

### Acceptance criteria

- CI failure messages краткие, точные и actionable.
- Есть integration tests на CI modes.

### Риск, если не сделать

- Инструмент будет раздражать в pipelines и его отключат.

---

# P6 — Observability & Ops

> Цель: сделать систему пригодной для эксплуатации, расследований и интеграции в реальные operational workflows.
>
> Почему после UX: сначала нужно понять стабильный пользовательский интерфейс и decision model, затем уже строить observability вокруг неё.

---

## Ticket 6.1 — Добавить structured logging

### Проблема

Текущий вывод ориентирован в основном на человека. Для ops и интеграции полезны структурированные логи.

### Что нужно сделать

- Добавить JSON logging mode.
- Стандартизировать поля:
  - timestamp
  - command
  - risk
  - decision
  - pattern ids
  - mode
  - ci
  - allowlist match

### Acceptance criteria

- Structured logs можно отправлять в ELK/Loki и similar systems.
- JSON не смешивается с human output случайно.

---

## Ticket 6.2 — Добавить базовые metrics

### Проблема

Без численных метрик сложно понимать реальную полезность и cost инструмента.

### Что нужно сделать

Минимально считать:

- total commands scanned
- blocked / denied / approved / auto-approved
- warn vs danger vs block distribution
- snapshot creation rate
- config validation failures

### Acceptance criteria

- Есть хотя бы базовый metrics surface.
- Метрики можно читать локально или выгружать программно.

---

## Ticket 6.3 — Сделать audit logging concurrency-safe

### Проблема

Append + rotation без межпроцессной координации может породить race conditions.

### Что нужно сделать

- Добавить file locking или другой механизм межпроцессной синхронизации.
- Отдельно защитить:
  - append path
  - rotation path
  - archive enumeration path

### Acceptance criteria

- Параллельные запуски не портят audit log.
- Есть concurrency tests.

### Риск, если не сделать

- Audit лог как forensic source не будет надёжным.

---

## Ticket 6.4 — Добавить audit integrity mechanisms

### Проблема

Для production расследований полезно понимать, не был ли audit log модифицирован постфактум.

### Что нужно сделать

Опционально добавить:

- checksums,
- chained hashes,
- signing strategy,
- tamper-evident mode.

### Acceptance criteria

- Есть documented integrity mode.
- Читатель лога может обнаружить нарушение целостности.

### Риск, если не сделать

- Audit остаётся полезным, но менее надёжным как доказательная база.

---

# P7 — Performance

> Цель: убрать лишние runtime-затраты и зафиксировать performance baseline.
>
> Почему после ops: сначала система должна стать корректной и эксплуатируемой, и только потом имеет смысл оптимизировать её узкие места.

---

## Ticket 7.1 — Исключить лишнюю компиляцию regex в UI/highlighting path

### Проблема

UI для подсветки не должен пересобирать regex-ы, если scanner уже сделал основную работу.

### Что нужно сделать

- Передавать match ranges или enriched match metadata из scanner.
- Не компилировать regex заново в confirmation UI.

### Acceptance criteria

- Highlighting использует уже вычисленные данные.
- Нет redundant regex compilation в hot path.

---

## Ticket 7.2 — Оптимизировать highlighted command rendering

### Проблема

Подсветка полезна, но не должна становиться дорогой на длинных командах и больших match sets.

### Что нужно сделать

- Работать с уже отсортированными диапазонами.
- Минимизировать лишние аллокации.
- Добавить stress tests на длинный command line / heredoc-like input.

### Acceptance criteria

- Rendering остаётся быстрым на больших input.
- Нет некорректной подсветки при overlapping matches.

---

## Ticket 7.3 — Интегрировать benchmark-проверки в CI

### Проблема

Есть бенчмарки, но без контроля regressions их ценность ограничена.

### Что нужно сделать

- Добавить repeatable benchmark strategy.
- Минимум:
  - baseline document,
  - regression threshold policy,
  - optional scheduled performance job.

### Acceptance criteria

- Performance regressions можно замечать до релиза.
- Benchmark results интерпретируемы.

---

## Ticket 7.4 — Ограничить размер и сложность scan input

### Проблема

Очень длинные heredoc / inline scripts / generated blobs могут сделать scan слишком дорогим.

### Что нужно сделать

- Ввести разумные лимиты:
  - на длину command,
  - на длину inline script,
  - на глубину recursive parsing.
- Для превышений — явная policy:
  - deny,
  - warn,
  - truncate + mark uncertain.

### Recommended default

- fail closed или at least explicit uncertain classification

### Acceptance criteria

- Огромный input не убивает процесс и не делает latencies непредсказуемыми.
- Есть tests на oversized inputs.

---

# P8 — Testing Expansion

> Цель: закрыть разрыв между модульной корректностью и системной корректностью.
>
> Почему после performance: к этому моменту архитектура уже стабильна, и можно фиксировать end-to-end поведение большими тестами.

---

## Ticket 8.1 — Добавить end-to-end CLI integration tests

### Проблема

Много unit-тестов — это хорошо, но нужен системный уровень проверки.

### Что нужно сделать

Покрыть:

- `aegis -c ...`
- config load
- decision
- audit append
- exit code contract
- JSON output
- CI behavior

### Acceptance criteria

- Есть реальные process-level tests.
- Проверяются stdout/stderr/exit code.

---

## Ticket 8.2 — Добавить config integration tests

### Проблема

Главный historical gap проекта — config/runtime drift. Это должно быть закрыто системными тестами.

### Что нужно сделать

Проверять:

- custom patterns действительно работают;
- mode действительно влияет;
- allowlist scope действительно учитывается;
- snapshot flags действительно активируют/деактивируют plugins.

### Acceptance criteria

- Regression этого класса ловится тестами.

---

## Ticket 8.3 — Расширить snapshot integration tests

### Проблема

Snapshot subsystem сложная и внешне-зависимая. Ей нужны более реалистичные сценарии.

### Что нужно сделать

Покрыть:

- git repos
- subdirs/worktrees
- docker selection policy
- rollback UX
- conflict/error paths

### Acceptance criteria

- Основные snapshot сценарии проходят не только через mock logic.

---

## Ticket 8.4 — Добавить concurrency tests

### Проблема

Audit logger и rotation без stress-проверки остаются потенциальным источником corruption.

### Что нужно сделать

- Параллельные append
- append + rotation
- multiple process simulation

### Acceptance criteria

- Нет corruption/race-induced data loss в тестовых сценариях.

---

## Ticket 8.5 — Добавить security regression suite для bypass attempts

### Проблема

Guardrail-инструмент обязан иметь corpus обходов, чтобы не регрессировать по security posture.

### Что нужно сделать

Поддерживать тестовый набор:

- nested shell
- heredoc
- eval
- process substitution
- encoded payload wrappers
- multiline script bodies
- environment-prefixed execution

### Acceptance criteria

- Есть выделенный security regression suite.
- Новые bypass cases легко добавлять.

---

# P9 — Packaging & Distribution

> Цель: довести продукт до состояния, в котором его можно стабильно устанавливать, обновлять и сопровождать.
>
> Почему в конце: упаковка имеет смысл только после стабилизации behavior и contract.

---

## Ticket 9.1 — Подготовить production installer / setup flow

### Проблема

CLI-инструмент должен уметь корректно встраиваться в shell workflow пользователя.

### Что нужно сделать

- Подготовить install/uninstall scripts.
- Корректно обрабатывать:
  - shell wrapper setup,
  - `AEGIS_REAL_SHELL`,
  - recursion protection,
  - rollback uninstall path.

### Acceptance criteria

- Установка и удаление детерминированы.
- Нету случайного циклического shell wrapping.

---

## Ticket 9.2 — Определить и реализовать cross-platform strategy

### Проблема

Сейчас проект в первую очередь ориентирован на Unix-like shell model. Это нормально, но должно быть явно оформлено.

### Варианты

- либо официально поддерживать только Unix-like системы;
- либо проектировать отдельный Windows strategy.

### Что нужно сделать

- Явно определить support matrix.
- Если Windows входит в scope:
  - продумать `cmd.exe` / PowerShell semantics,
  - path handling,
  - process model.

### Acceptance criteria

- Платформенная поддержка не “подразумевается”, а документирована и протестирована.

---

## Ticket 9.3 — Ввести versioned config schema

### Проблема

По мере роста конфигурации появится риск breaking changes и migration pain.

### Что нужно сделать

- Добавить `config_version`.
- Документировать schema evolution.
- Подготовить migration path:
  - old → new allowlist format,
  - old mode semantics,
  - deprecated fields.

### Acceptance criteria

- Новые релизы не ломают конфиг “вслепую”.
- Есть migration story.

---

# Production Readiness Checklist

Проект считается production-ready, когда выполнены все условия:

- [ ] Config полностью влияет на runtime behavior
- [ ] `custom_patterns` реально работают
- [ ] `Mode` полностью реализован
- [ ] Нет silent fallback на malformed config
- [ ] Нет silent ignore invalid allowlist entries
- [ ] Allowlist структурирован и ограничен по scope
- [ ] Allowlist не может опасно bypass-ить policy без явного разрешения
- [ ] Snapshot subsystem управляется policy, а не только defaults
- [ ] Есть rollback CLI
- [ ] Есть structured JSON output
- [ ] CI behavior детерминирован и хорошо объясним
- [ ] UX не допускает случайного подтверждения опасной команды
- [ ] Audit logging race-safe
- [ ] Есть end-to-end integration tests
- [ ] Есть security regression suite
- [ ] Есть понятная packaging/install story
- [ ] Есть versioned config schema

---

# Критический путь выполнения

Основной путь, без которого проект не стоит считать production candidate:

```text
P1 → P2 → P3
```

После завершения:

- **P1** проект перестаёт врать пользователю о своих возможностях.
- **P2** policy становится строгой и надёжной.
- **P3** runtime становится пригодным для стабильной эксплуатации.

Дальше:

- **P4** превращает проект из аккуратного guardrail prototype в более зрелый security-oriented инструмент.
- **P5–P6** делают его пригодным для реальной эксплуатации и расследований.
- **P7–P9** доводят проект до уровня зрелого production продукта.

---

# Краткий итог

Сейчас проект ближе к:

- **сильному prototype / portfolio repo**

После выполнения фаз:

- **P1–P3** → usable production candidate
- **P4–P6** → security-grade operational tool
- **P7–P9** → зрелый production-ready продукт
