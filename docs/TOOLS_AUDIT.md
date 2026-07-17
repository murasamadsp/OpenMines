# Tools Audit

Дата актуализации: 2026-07-17 (rev 4).

Цель: привести `scripts/` и `tools/` к понятному dev-контуру без удаления
полезных отладочных инструментов вслепую.

## Scripts

Структура: `scripts/<группа>/<имя>`.

### `scripts/dev/` — локальный dev-loop

| Файл | Статус | Назначение |
|---|---|---|
| `scripts/dev/run.sh` | active | `cargo run` + sccache + dev-token. Основной локальный запуск. |
| `scripts/dev/server.sh` | active | Изолированный Unity-dev контур в `.local/`. Патчит конфиг inline. |
| `scripts/dev/smoke.sh` | active | Быстрый local wire smoke без Unity/VPS. |
| `scripts/dev/wipe-players.sh` | dangerous/dev | Деструктивный dev-вайп (players, programs, buildings, clans, clan_requests, chat_messages). Требует `--yes`. |

### `scripts/quality/` — quality gate

| Файл | Статус | Назначение |
|---|---|---|
| `.githooks/pre-commit` | active wrapper | Git hook entrypoint через `core.hooksPath=.githooks`. Вызывает `scripts/quality/pre-commit.sh`. |
| `scripts/quality/pre-commit.sh` | active | Единый tracked pre-commit pipeline. |
| `scripts/quality/ci.sh` | active | CI/full quality gate. |
| `scripts/quality/common.sh` | active | Shared функции для pre-commit/CI/manual tools. **Ядро.** |
| `scripts/quality/extra.sh` | active manual | Тяжёлые/ручные проверки. Субкоманды: `test features deps coverage mutants vet fmod ub arch outdated geiger bloat cache stop-cache`. |
| `scripts/quality/bootstrap.sh` | active | Одноразовая установка cargo tooling + hooksPath. |
| `scripts/quality/target-cache.sh` | dangerous/manual | Inspect/prune Cargo `target/`. `--prune` удаляет incremental cache. |

### `scripts/guards/` — static guards (вызываются из quality gate)

| Файл | Статус | Назначение |
|---|---|---|
| `scripts/guards/arch.sh` | active | Architecture fail-fast gate + `--report` для read-only отчёта. |
| `scripts/guards/ownership.sh` | active | Rust ownership guard: `async_trait`, boxed futures, sync-lock через `.await` (Python heredoc). |
| `scripts/guards/soundness.sh` | active | Rust soundness guard: unsafe allowlist, raw pointer/FFI, adjacent atomics. |
| `scripts/guards/hygiene.sh` | active | Hygiene guard: git hook topology, exec bits, registry coverage. |
| `scripts/guards/ecs-bypass.py` | active | ECS bypass baseline checker. Вызывается из `guards/arch.sh`. |
| `scripts/guards/soundness.py` | active | Python: unsafe allowlist + adjacent atomics detector. Вызывается из `guards/soundness.sh`. |

### `scripts/client/` — клиентские задачи

| Файл | Статус | Назначение |
|---|---|---|
| `scripts/client/build.sh` | explicit client task | Headless Unity build (Win64/macOS). |
| `scripts/client/fmod-check.sh` | active manual | FMOD bank contract check. Вызывается через `scripts/quality/extra.sh fmod`. |

## Rust Tools

| Файл | Статус | Назначение |
|---|---|---|
| `crates/openmines-loadtest/Cargo.toml` | active | Manifest Rust loadtest crate. |
| `crates/openmines-proxy/Cargo.toml` | active | Manifest Rust proxy crate. |

## Python Tools

Структура: `tools/<группа>/<имя>` + shared lib в корне `tools/`.

### `tools/` (корень) — shared

| Файл | Статус | Назначение |
|---|---|---|
| `tools/om_net.py` | active | Shared Python сетевые утилиты. Импортируется из `tools/probes/`. |
| `tools/requirements.txt` | active | Python deps (`telethon`). |

### `tools/probes/` — живые TCP-репро (требуют creds)

| Файл | Статус | Назначение |
|---|---|---|
| `tools/probes/chat_probe.py` | live probe | FED/chat wire probe. |
| `tools/probes/chat_probe_pass2.py` | live probe | Chat persistence pass-2 probe. |
| `tools/probes/repro_freeze.py` | live repro | TCP repro фриза через auth/keepalive/move/dig. |
| `tools/probes/sim_players.py` | live load repro | Multi-player TCP simulator. |
| `tools/probes/proxy_smoke.py` | active | E2E smoke для proxy restart/replay. |

### `tools/audit/` — read-only статические анализаторы

| Файл | Статус | Назначение |
|---|---|---|
| `tools/audit/mapdump.py` | active diagnostic | Read-only dump `_v2.map`. |
| `tools/audit/ui_layout_audit.py` | active diagnostic | Read-only Unity UI layout audit. |

### `tools/external/` — внешние данные, не часть dev-loop

| Файл | Статус | Назначение |
|---|---|---|
| `tools/external/tg_parser.py` | external data tool | Telegram parser. Требует секреты/session. |
| `tools/external/download_fodinae.py` | reference fetch | Скачивает JS reference assets. Manual only. |

## Tracked State Risk

Следующие файлы являются state/cache, а не исходниками. Они должны оставаться
untracked и игнорироваться Git:

- `tools/.repro_creds.json`
- `tools/.sim_creds.json`
- `tools/.p2_ref.json`
- `tools/tg_parser_session.session`, `tools/tg_config.json`, `tools/tg_state.json`

## Changelog

- **2026-07-17 rev 2**: исправлен баг — `quality/extra.sh features|deps|coverage|mutants|vet` падал с `command not found`.
- **2026-07-17 rev 3**: консолидация 22 → 17 файлов: удалены `arch-audit.sh`, `dev-patch-config.py`, `ownership-audit-lock-guard.py`, `ub-audit-unsafe.py`; `ub-audit-atomics.py` → `ub-audit.py`.
- **2026-07-17 rev 4**: реструктуризация в подпапки + единый конвент имён. `scripts/` → 4 группы (dev/quality/guards/client). `tools/` → 3 группы (probes/audit/external).
