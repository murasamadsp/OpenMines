# Журнал изменений

Формат журнала изменений основан на `Keep a Changelog`.

## Не выпущено

- Simulation owner переведён с fixed `100 Hz` idle loop на event-driven wait по
  command, persistence progress и domain deadlines; `10ms` остался budget active
  cycle.
- Boom, Protector и Raz переведены на bounded `DueActionQueue` с typed effects;
  building delete и persistence completions получили guarded owner flow.
- Добавлен канонический checkpoint `SERVER_MIGRATION_STATUS.md` с проверенными
  gates, runtime evidence и следующим migration slice.
- Добавлен CI/CD на GitHub Actions: проверки (`fmt`/`clippy`/тесты) на PR и push, сборка Docker-образа с публикацией в GHCR и автоматический деплой.
- `ops/Dockerfile`: рабочий каталог `/app` (том состояния больше не затеняет запечённые конфиги), публикация порта метрик `8091`.
- Обновлена "сетевая" документация для репозитория и добавлены базовые метаданные проекта.
- Добавлены шаблоны Issue, PR, `README`, `CONTRIBUTING`, `SECURITY`, `CODE_OF_CONDUCT`, `.gitattributes`, `config.example.json`.
- Добавлен workflow для релизов: сборка Docker-образа и публикация GitHub Release.

## [0.0.2] — 2026-04-16

- Базовая реализация Rust-сервера с сетевым протоколом, логированием, world/db слоями.
- Инициализация документации для быстрого старта и интеграции.
