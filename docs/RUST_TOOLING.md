# Rust Tooling

Дата актуализации: 2026-07-07.

Цель: держать Rust-инструментарий практичным. Добавлять в обязательный контур
только то, что ловит реальные ошибки или заметно ускоряет локальную проверку.

## Fast gate

```bash
cargo fmt --all --check
cargo run -- --doctor
cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery
cargo test --all-targets --all-features
scripts/dev-smoke.sh
```

Не запускать `cargo test` и `cargo clippy` параллельно на dev-машине. Это не
ускоряет цикл линейно, зато легко съедает RAM из-за одновременной компиляции
одних и тех же тяжёлых crate-графов.

## Compile speed

Базовая цель: быстрый локальный цикл должен сначала ловить ошибку дешёвыми
проверками, а полный gate запускать один раз перед коммитом.

Рекомендуемый локальный порядок до полного pre-commit:

```bash
cargo check-server
cargo doctor
scripts/dev-smoke.sh
cargo nextest run --workspace
```

Трек ускорения:

- Для запуска сервера использовать `scripts/dev-run.sh`: он включает `sccache`,
  если тот установлен, но не делает его обязательной зависимостью.
- Для коротких команд использовать cargo aliases:
  `cargo check-server`, `cargo doctor`, `cargo server`.
- Проверить быстрый linker: `mold` на Linux, `lld` там где стабильно доступен.
- Использовать `cargo nextest` как основной test runner вместо голого
  `cargo test` для рабочих прогонов.
- Разделить gates: быстрый per-change (`check/doctor/dev-smoke`) и полный
  pre-commit (`fmt/clippy/all tests/quality tools`).
- Не feature-trim'ить зависимости вслепую: сначала измерить build time и размер,
  потом резать конкретные default features.

## Target cache policy

`target/` — это rebuildable cache, но не бездонная свалка. Нормальный контур:

```bash
scripts/target-cache.sh
scripts/target-cache.sh --prune --dry-run
scripts/target-cache.sh --prune
```

Soft budget по умолчанию: 8 GiB (`OPENMINES_TARGET_MAX_GIB=...`). `--prune`
удаляет только мягкий мусор: incremental cache, rust-analyzer cache, test
binaries и `.rcgu.o`. Он не удаляет `target/debug/deps/*.rlib`/`*.rmeta`, потому
что это полезный warm cache для следующего `cargo check/run`.

`--clean`/`cargo clean` — только ручной крайний случай: освобождает почти всё,
но следующая сборка снова пойдёт с нуля.

## Dependency audit

Текущий статус:

- `cargo machete` — чисто, прямых неиспользуемых зависимостей не найдено.
- `cargo deny check` — чисто: advisories, bans, licenses, sources.
- `cargo audit` — чисто, известных уязвимостей не найдено.
- `cargo outdated --workspace --depth 1` — прямые зависимости актуальны.

`cargo tree --duplicates` показывает только транзитивные дубли из экосистемы
Bevy/SQLx/Tokio: разные версии `foldhash`, `getrandom`, `hashbrown`, `syn`,
`thiserror`, `winnow` и повторяющиеся `sqlx-sqlite` ветки. Это не повод резать
зависимости вслепую: прямой мусор должен сначала проявиться в `cargo machete`,
а feature-trimming нужно делать отдельным измеряемым срезом.

## Periodic checks

```bash
cargo machete
cargo deny check
cargo audit
cargo outdated --workspace --depth 1
cargo tree --duplicates
cargo tree -e features --depth 1
```

Периодические тяжёлые проверки:

```bash
cargo geiger
cargo bloat --release --crates
scripts/ub-audit.sh
```

`cargo geiger` и `cargo bloat` не являются fast gate. Они нужны для осознанных
аудитов unsafe/размера бинаря, а не для блокировки каждой правки.

`scripts/ub-audit.sh` — fast gate для soundness-границ проекта. Новый `unsafe`,
raw pointers, `UnsafeCell`, FFI или adjacent hot atomics должны быть явным
архитектурным решением, а не случайным побочным эффектом.

## Feature trimming

В `cargo tree -e features --depth 1` видно, что часть прямых зависимостей пока
использует default features (`tokio`, `sqlx`, `axum`, `bevy_ecs`, `rand`,
`tracing-subscriber`). Это потенциальный трек оптимизации, но не срочный фикс:
резать features можно только отдельным PR/коммитом с `cargo check`, тестами,
doctor/dev-smoke и сравнением build/runtime эффекта.
