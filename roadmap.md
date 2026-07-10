# Roadmap разработки `orbita`

Этот документ описывает рекомендуемый порядок разработки, небольшие ветки и
атомарные коммиты. План построен так, чтобы каждый релиз оставался
работоспособным, а публичные контракты (`Schema`, формат workflow, WIT/RPC и
`RunStore`) фиксировались до начала зависящей от них реализации.

## 1. Правила работы с Git

- `main` содержит только опубликованные версии. В него попадают только
  завершённые `release/*` и срочные `hotfix/*`.
- `develop` — основная интеграционная ветка: она всегда собирается и содержит
  изменения следующего релиза.
- Каждая задача выполняется в короткоживущей `feature/*` от актуального
  `develop` и после проверки вливается обратно в `develop`. Ветки не
  объединяются друг с другом напрямую: их зависимости берутся из обновлённого
  `develop`.
- Для подготовки версии создаётся `release/<version>` от `develop`; после
  приёмки она вливается в `main` и обратно в `develop`. Для критичных проблем
  опубликованной версии используется `hotfix/<issue>` от `main`.
- Имена рабочих веток: `feature/...`. Например:
  `feature/runtime-dag-scheduler`.
- Коммиты использовать в стиле Conventional Commits:
  `feat(runtime): add bounded DAG scheduler`,
  `fix(workflow): reject cyclic graph`.
- В одном коммите не смешивать изменение контракта, его реализацию и
  рефакторинг. Исключение — минимальная реализация, без которой контракт
  невозможно проверить тестом.
- Перед merge обязательны `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` и релевантные тесты. После появления WASM и sidecar к ним добавляются contract-тесты.

## 2. Вехи и порядок зависимостей

```text
Каркас
  └─ 0.1: core + workflow + SDK + memory store/registry + validate CLI
       └─ 0.2: ExecutionPlan + runtime DAG + resilient execution + run CLI
            └─ 0.3: RunStore + checkpoint + recovery
                 └─ 0.4: packages + resolver/lock + WASM host/sandbox
                      └─ 0.5: sidecar RPC + supervisor + OS sandbox
                           └─ 1.0: security audit, benchmarks, API freeze
```

## 3. Подготовка: каркас

Цель этапа — получить пустой, но собираемый workspace. Это не релиз
функциональности.

| Очерёдность | Ветка | Что входит | Предлагаемые коммиты |
| --- | --- | --- | --- |
| 1 | `feature/workspace-skeleton` | Cargo workspace и пустые crates из ТЗ, facade `orbita`, общие lint и MSRV/платформы | `chore(workspace): add orbita crate layout`; `chore: configure workspace lint policy` |

После merge этого блока зафиксировать базовый tag, например
`bootstrap-0.0.0`. В `Cargo.toml` уже есть workspace, поэтому его следует
развить, а не создавать заново.

## 4. Релиз `0.1.0` — модель и валидация workflow

Результат: приложение может подключить `orbita`, описать workflow в Rust или
YAML/JSON, проверить его до запуска и получить диагностируемую ошибку.
Реальное асинхронное выполнение появится в `0.2.0`.

| Очерёдность | Ветка | Что входит | Предлагаемые коммиты |
| --- | --- | --- | --- |
| 1 | `feature/core-identifiers-errors` | `NodeId`, `PluginId`, `RunId`, порты, единая публичная иерархия ошибок, `#[non_exhaustive]` | `feat(core): add domain identifiers`; `feat(core): add classified public errors`; `test(core): cover identifier parsing` |
| 2 | `feature/core-schema` | `Schema`, `TypeRef`, canonical representation, SHA-256 fingerprint, проверка payload | `feat(core): add canonical schema model`; `feat(core): compute stable type fingerprints`; `test(core): add schema compatibility cases` |
| 3 | `feature/workflow-document` | версии формата, serde-модель YAML/JSON, миграции и диагностические span/позиции | `feat(workflow): add versioned workflow document`; `feat(workflow): support YAML and JSON decoding`; `test(workflow): add format fixtures` |
| 4 | `feature/workflow-validator` | проверка нод/портов, обязательных входов, кратности, типов, ссылок и запрет произвольных циклов | `feat(workflow): validate graph references and ports`; `feat(workflow): validate type-compatible edges`; `feat(workflow): reject unsupported cycles` |
| 5 | `feature/registry-memory` | описание ноды, in-memory `NodeRegistry`, разрешение локальных Rust-нод без плагинов | `feat(registry): add node descriptor and in-memory registry`; `test(registry): cover node lookup errors` |
| 6 | `feature/store-memory` | object-safe базовые traits и in-memory реализация в границах, не требующая БД | `feat(store): define initial storage traits`; `feat(store): add in-memory implementations` |
| 7 | `feature/sdk-typed-builder` | `Node`, `Schema` derive/macro, `Input<T>`/`Output<T>`, builder с compile-time проверкой соединений | `feat(sdk): add Rust node trait`; `feat(sdk): add Schema derive macro`; `feat(sdk): add type-safe workflow builder`; `test(sdk): add compile-fail port fixtures` |
| 8 | `feature/cli-validate` | `orbita validate workflow.yaml`, human/JSON output, корректные exit codes | `feat(cli): add validate command`; `feat(cli): add JSON diagnostic output`; `test(cli): cover validation exit codes` |
| 9 | `release/0.1.0` | проверка acceptance-сценариев этапа | `test: add 0.1 acceptance workflows`; `chore(release): prepare v0.1.0` |

Перед созданием тега `v0.1.0` критерии: несовместимый edge отклоняется Rust
builder-ом на компиляции либо валидатором файла до запуска; ошибка называет
ноду, порт и оба типа; `orbita validate` работает локально и в JSON-режиме.

## 5. Релиз `0.2.0` — план и асинхронный runtime

Результат: workflow компилируется в неизменяемый `ExecutionPlan` и выполняется
в `tokio` с ограничением параллелизма, retry, timeout, отменой и tracing.

| Очерёдность | Ветка | Что входит | Предлагаемые коммиты |
| --- | --- | --- | --- |
| 1 | `feature/workflow-execution-plan` | разрешённые ноды, топологический порядок, policy, `plan_hash`, cache key workflow + lock | `feat(workflow): compile validated document into execution plan`; `test(workflow): cover deterministic plan hashes` |
| 2 | `feature/runtime-run-model` | `Run`, состояния, `RunOptions`, `NodeContext`, correlation ID, deadline/cancellation context | `feat(runtime): add run lifecycle model`; `feat(runtime): add node execution context` |
| 3 | `feature/runtime-dag-scheduler` | конкурентное DAG-исполнение, fan-out/fan-in, глобальный и per-workflow semaphore, bounded queues | `feat(runtime): execute independent DAG nodes concurrently`; `feat(runtime): bound concurrency and queue capacity`; `test(runtime): cover fan-out and fan-in` |
| 4 | `feature/runtime-resilience` | timeout на попытку и Run, exponential backoff с jitter, retry-классификация, отмена и at-least-once/idempotency key | `feat(runtime): add retry policy and backoff`; `feat(runtime): enforce attempt and run timeouts`; `feat(runtime): propagate cancellation to nodes`; `test(runtime): cover retry and cancellation` |
| 5 | `feature/runtime-control-flow` | reference-ноды `core.if`, `core.foreach`, `join`, `delay`; явное моделирование control-flow вместо циклов графа | `feat(runtime): add core if node`; `feat(runtime): add foreach and join control flow`; `test(runtime): cover control-flow execution` |
| 6 | `feature/runtime-observability` | `tracing` spans, `EventSink`, redacted поля, метрики runtime | `feat(runtime): emit run and node tracing spans`; `feat(runtime): add event sink and metrics hooks`; `test(runtime): verify payload redaction` |
| 7 | `feature/cli-run` | `orbita run workflow.yaml --input input.json`, serialisation результата и машиночитаемых ошибок | `feat(cli): add local run command`; `test(cli): add run command fixtures` |
| 8 | `release/0.2.0` | examples Rust-нод и нагрузочные базовые сценарии | `test: add 0.2 acceptance suite`; `chore(release): prepare v0.2.0` |

Тег `v0.2.0` возможен, когда независимые ноды действительно запускаются
конкурентно, лимиты соблюдаются, retry не превышает policy, а отмена не
оставляет ожидающие задачи без завершения.

## 6. Релиз `0.3.0` — сохранение состояния и восстановление

Публичный `RunStore` нельзя менять «по ходу» без версионирования.

| Очерёдность | Ветка | Что входит | Предлагаемые коммиты |
| --- | --- | --- | --- |
| 1 | `feature/store-run-store` | object-safe `RunStore`, модели attempts/events/checkpoints, in-memory реализация контракта | `feat(store): add RunStore v1 contract`; `feat(store): persist run events in memory`; `test(store): cover status transitions` |
| 2 | `feature/runtime-checkpoints` | запись переходов и checkpoint до/после узла, уникальность `(run_id, node_id, attempt)` | `feat(runtime): checkpoint node attempts`; `test(runtime): prevent duplicate attempt records` |
| 3 | `feature/runtime-recovery` | восстановление незавершённого Run, пропуск подтверждённых нод, повтор неподтверждённых | `feat(runtime): resume incomplete persisted runs`; `test(runtime): recover after simulated crash` |
| 4 | `feature/test-persistent-adapter` | тестовый persistent adapter без драйвера БД в core dependencies, contract suite для адаптеров | `feat(testkit): add RunStore contract tests`; `test(store): add persistent adapter recovery fixture` |
| 5 | `release/0.3.0` | acceptance test аварийного завершения | `test: add 0.3 recovery acceptance suite`; `chore(release): prepare v0.3.0` |

## 7. Релиз `0.4.0` — пакеты и WASM-плагины

Сначала проверяется пакет и его manifest, затем разрешаются зависимости, и
лишь после этого загружается WASM-компонент.

| Очерёдность | Ветка | Что входит | Предлагаемые коммиты |
| --- | --- | --- | --- |
| 1 | `feature/plugin-api-wit` | WIT-файлы, общие payload/RPC-типы, ABI version negotiation и golden fixtures | `feat(plugin-api): add WIT v1 interfaces`; `test(plugin-api): add ABI golden fixtures` |
| 2 | `feature/plugin-manifest-package` | `.orb`, `plugin.orbita.toml`, проверка структуры архива, размера, path traversal и schema files | `feat(registry): parse plugin manifest`; `feat(registry): validate orb package layout`; `test(registry): reject malicious archives` |
| 3 | `feature/registry-resolver-lock` | SemVer resolution, `PluginStore`, trust roots, SHA-256/signature verification, `orbita.lock` | `feat(registry): resolve plugin dependencies`; `feat(registry): write reproducible lockfile`; `feat(registry): verify package digest and signature`; `test(registry): cover strict and development policies` |
| 4 | `feature/plugin-host-wasm` | загрузка Component Model, вызов `execute`, валидация payload на границе, lifecycle хоста | `feat(plugin-host): load WASM component plugins`; `feat(plugin-host): validate plugin boundary payloads`; `test(plugin-host): execute reference component` |
| 5 | `feature/wasm-capability-policy` | deny-by-default WASI, capability handles, URL allowlist, secret access/audit, отказ при невозможности изолировать | `feat(plugin-host): enforce deny-by-default WASI policy`; `feat(plugin-host): add audited secret handles`; `test(plugin-host): deny filesystem network and environment access` |
| 6 | `feature/plugin-contract-testkit` | testkit, reference `http.request` как плагин, общий contract suite | `feat(testkit): add plugin contract harness`; `feat(plugin): add capability-gated HTTP reference node`; `test: run WASM plugin contract suite` |
| 7 | `release/0.4.0` | security negative tests | `test: add 0.4 security acceptance suite`; `chore(release): prepare v0.4.0` |

Критерий релиза: плагин с неверным API, digest, подписью или capability
отклоняется до выполнения, а непривилегированный WASM-плагин не получает доступ
к файлам, сети или секретам хоста.

## 8. Релиз `0.5.0` — sidecar-плагины

| Очерёдность | Ветка | Что входит | Предлагаемые коммиты |
| --- | --- | --- | --- |
| 1 | `feature/plugin-api-sidecar-rpc` | length-prefixed transport, handshake, execute, logs, cancellation, healthcheck, нормализованные ошибки | `feat(plugin-api): implement sidecar RPC v1 framing`; `test(plugin-api): add RPC protocol golden tests` |
| 2 | `feature/plugin-host-sidecar` | запуск, supervision, graceful shutdown/restart, передача cancellation, лимиты времени | `feat(plugin-host): supervise sidecar lifecycle`; `feat(plugin-host): forward execution and cancellation over RPC`; `test(plugin-host): handle unhealthy sidecars` |
| 3 | `feature/sidecar-sandbox-windows` | Job Objects и проверка лимитов на Windows x86_64 | `feat(plugin-host): sandbox sidecars with Windows Job Objects`; `test(plugin-host): verify Windows sidecar limits` |
| 4 | `feature/sidecar-sandbox-linux` | cgroups/namespaces либо согласованный container adapter на Linux x86_64 | `feat(plugin-host): sandbox sidecars on Linux`; `test(plugin-host): verify Linux sidecar limits` |
| 5 | `feature/sidecar-python-go-examples` | минимальные SDK/examples Python и Go, одна нода в двух формах — WASM и sidecar | `feat(examples): add Python sidecar plugin`; `feat(examples): add Go sidecar plugin`; `test: run shared WASM and sidecar contracts` |
| 6 | `release/0.5.0` | ограничения платформ и acceptance suite ABI | `test: add 0.5 cross-runtime acceptance suite`; `chore(release): prepare v0.5.0` |

Если изоляцию невозможно применить на целевой платформе, sidecar не должен
стартовать. Понижение уровня защиты ради удобства — ошибка реализации.

## 9. Релиз `1.0.0` — стабилизация

| Очерёдность | Ветка | Что входит | Предлагаемые коммиты |
| --- | --- | --- | --- |
| 1 | `feature/coverage-and-regressions` | 80% coverage детерминированных модулей, негативные security tests, regression fixtures | `test: add core workflow registry runtime coverage gates`; `test: add security regression suite` |
| 2 | `feature/runtime-baselines` | baseline: graph из 100 нод, 1 000 коротких Run, fan-out 100; порог регрессии 10% | `bench: add workflow compilation baseline`; `bench: add runtime throughput baselines`; `bench: record performance regression threshold` |
| 3 | `feature/security-audit` | аудит capabilities/secret redaction/архивов/sandbox | `test(security): add audit remediation regressions` |
| 4 | `release/1.0.0-rc1` | заморозка публичного API, release candidate, исправление только блокирующих дефектов | `chore(release): prepare v1.0.0-rc.1` |
| 5 | `release/1.0.0` | финальные теги | `chore(release): prepare v1.0.0` |

## 10. Pull request и release checklist

Каждая feature-ветка должна содержать тест, воспроизводящий ожидаемое
поведение, и негативный тест для границ валидации или безопасности.

Для каждого релизного тега дополнительно проверить acceptance-критерии
соответствующего этапа из `task.md`, обе целевые платформы и отсутствие
незакоммиченных изменений. Следующие крупные стадии допускается начинать после
тега предыдущей: так зависимости, lock-файлы и публичные API остаются
предсказуемыми.
