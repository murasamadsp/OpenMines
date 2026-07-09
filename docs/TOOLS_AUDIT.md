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
| `scripts/check-fmod-events.sh` | active manual | Проверяет, что FMOD bank содержит все `event:/...` из `docs/reference/FMOD_EVENTS.txt`, и что они есть в `SoundManager.cs`. | Оставить как явный gate sound-трека; вызывается через `scripts/quality-extra.sh fmod` и `PRE_COMMIT_EXTENDED=1`. |
| `scripts/quality-extra.sh` | active manual | Тяжёлые/ручные проверки: nextest/features/coverage/mutants/vet/fmod/cache. | Оставить; `fmod` пока ожидаемо падает до сборки настоящих FMOD events. |
| `scripts/arch-guard.sh` | active | Static architecture gate. | Оставить в CI/full gate. |
| `scripts/arch-audit.sh` | diagnostic | Read-only отчёт по архитектурным leakage. | Оставить как manual audit. |
| `scripts/target-cache.sh` | dangerous/manual | Показывает или удаляет `target/`; `--prune` удаляет incremental cache и может замедлить следующий `cargo run`. | Оставить ручным; в pre-commit запускать только через `PRE_COMMIT_PRUNE_TARGET=1`. |
| `scripts/build-client.sh` | explicit client task | Unity client compile gate. | Оставить; запускать только при client-задачах. |
| `scripts/wipe-players.sh` | dangerous/dev | Деструктивная dev-утилита для игроков. | Требует отдельной проверки перед использованием. |
| `scripts/tools-audit.sh` | active | Read-only hygiene guard для scripts/tools. | Оставить в CI/full gate. |
| `scripts/ownership-audit.sh` | active | Static Rust ownership/cancellation guard: запрещает `async_trait`, boxed futures в сервере и sync-lock guard через `.await`. | Оставить в `arch-guard` и pre-commit. |
| `scripts/ub-audit.sh` | active | Static Rust soundness guard: allowlist для `unsafe`, запрет raw pointer/FFI зон и adjacent atomics без padding в server hot structs. | Оставить в `arch-guard` и pre-commit. |

## Rust Tools

| Файл | Статус | Назначение | Действие |
|---|---|---|---|
| `crates/openmines-loadtest` | active | Rust loadtest crate. | Оставить. |
| `crates/openmines-loadtest/Cargo.toml` | active | Manifest Rust loadtest crate. | Оставить. |
| `crates/openmines-proxy` | active | Rust proxy crate. | Оставить. |
| `crates/openmines-proxy/Cargo.toml` | active | Manifest Rust proxy crate. | Оставить. |
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
| `tools/om_net.py` | active | Shared Python network utilities. | Оставить. |
| `tools/requirements.txt` | active | Python deps (`telethon`). | Оставить пока есть `tg_parser.py`. |

## Tracked State Risk

Следующие файлы являются state/cache, а не исходниками. Они должны оставаться
untracked и игнорироваться Git:

- `tools/.repro_creds.json`
- `tools/.sim_creds.json`
- `tools/.p2_ref.json`

`tools/tg_parser_session.session`, `tools/tg_config.json`, `tools/tg_state.json`
уже игнорируются и не должны попадать в Git.

## Следующий срез

1. Решить судьбу `tg_parser.py`: оставить как external-data tool или вынести из
   основного repo tooling.
2. Добавить быстрый `scripts/toolbox.sh` или `cargo xtask` только если список
   ручных команд начнёт снова расползаться.
