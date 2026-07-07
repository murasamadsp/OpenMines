# Tools Audit

Дата актуализации: 2026-07-07.

Цель: привести `scripts/` и `tools/` к понятному dev-контуру без удаления
полезных отладочных инструментов вслепую.

## Scripts

| Файл | Статус | Назначение | Действие |
|---|---|---|---|
| `scripts/pre-commit.sh` | active | Единый tracked pre-commit pipeline. | Оставить. |
| `.githooks/pre-commit` | active wrapper | Единственный Git hook entrypoint через `core.hooksPath=.githooks`. | Оставить. |
| `scripts/quality-common.sh` | active | Общие quality steps для pre-commit/CI/manual tools. | Оставить, держать DRY. |
| `scripts/ci-quality.sh` | active | CI/full quality gate. | Оставить. |
| `scripts/bootstrap-quality.sh` | active | Первичная установка cargo tooling и hooksPath. | Оставить. |
| `scripts/dev-server.sh` | active | Локальный Unity-dev сервер в `.local/`. | Оставить. |
| `scripts/dev-smoke.sh` | active | Быстрый local wire smoke без Unity/VPS. | Оставить. |
| `scripts/dev-run.sh` | active | Упрощённый `cargo run` с optional `sccache`. | Оставить. |
| `scripts/rust-modern.sh` | active manual | Тяжёлые ручные проверки: nextest/features/coverage/mutants/vet/cache. | Оставить, не pre-commit. |
| `scripts/arch-guard.sh` | active | Static architecture gate. | Оставить в CI/full gate. |
| `scripts/arch-audit.sh` | diagnostic | Read-only отчёт по архитектурным leakage. | Оставить как manual audit. |
| `scripts/target-cache.sh` | dangerous/manual | Показывает или удаляет `target/`. | Оставить, но `--clean` запускать только явно. |
| `scripts/build-client.sh` | explicit client task | Unity client compile gate. | Оставить; запускать только при client-задачах. |
| `scripts/wipe-players.sh` | dangerous/dev | Деструктивная dev-утилита для игроков. | Требует отдельной проверки перед использованием. |
| `scripts/tools-audit.sh` | active | Read-only hygiene guard для scripts/tools. | Оставить в CI/full gate. |

## Rust Tools

| Файл | Статус | Назначение | Действие |
|---|---|---|---|
| `tools/loadtest` | active | Rust loadtest crate. | Оставить. |
| `tools/loadtest/Cargo.toml` | active | Manifest Rust loadtest crate. | Оставить. |
| `tools/proxy` | active | Rust proxy crate. | Оставить. |
| `tools/proxy/Cargo.toml` | active | Manifest Rust proxy crate. | Оставить. |
| `tools/proxy_smoke.py` | active | E2E smoke для proxy restart/replay. | Оставить. |

## Python Diagnostics

| Файл | Статус | Назначение | Действие |
|---|---|---|---|
| `tools/mapdump.py` | active diagnostic | Read-only dump `_v2.map`. | Оставить. |
| `tools/ui_layout_audit.py` | active diagnostic | Read-only Unity UI layout audit. | Оставить. |
| `tools/repro_freeze.py` | live repro | TCP repro фриза через auth/keepalive/move/dig. | Оставить, требует локальных creds. |
| `tools/sim_players.py` | live load repro | Multi-player TCP simulator. | Оставить, требует локальных creds. |
| `tools/chat_probe.py` | live probe | FED/chat wire probe. | Оставить, требует локальных creds. |
| `tools/chat_probe_pass2.py` | live probe | Chat persistence pass-2 probe. | Оставить, требует local ref/cache. |
| `tools/download_fodinae.py` | reference fetch | Скачивает JS reference assets. | Manual only; не pre-commit. |
| `tools/tg_parser.py` | external data tool | Telegram parser. | Под вопросом: не игровой dev-loop, требует секреты/session. |
| `tools/requirements.txt` | active | Python deps (`telethon`). | Оставить пока есть `tg_parser.py`. |

## Tracked State Risk

Следующие файлы сейчас являются state/cache, а не исходниками. Их надо
перевести в untracked через `git rm --cached` отдельным подтверждённым шагом:

- `tools/.repro_creds.json`
- `tools/.sim_creds.json`
- `tools/.p2_ref.json`

`tools/tg_parser_session.session`, `tools/tg_config.json`, `tools/tg_state.json`
уже игнорируются и не должны попадать в Git.

## Следующий срез

1. Подтвердить и выполнить `git rm --cached` для tracked probe-state файлов.
2. Решить судьбу `tg_parser.py`: оставить как external-data tool или вынести из
   основного repo tooling.
3. Перевести warnings `scripts/tools-audit.sh` по tracked probe-state в strict
   errors после `git rm --cached`.
