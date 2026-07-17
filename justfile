# openmines task runner

# Показать список доступных команд
default:
    @just --list --justfile {{justfile()}}

# Быстрый локальный запуск с dev-токеном
dev-run *args:
    ./scripts/dev-run.sh {{args}}

# Локальный Unity-dev сервер с изолированным состоянием
dev-server *args:
    ./scripts/dev-server.sh {{args}}

# Запуск локальных smoke-тестов
dev-smoke:
    ./scripts/dev-smoke.sh

# Запуск полного pre-commit контроля качества
pre-commit:
    ./scripts/pre-commit.sh

# Базовая проверка: cargo check + тесты openmines-world
verify:
    cargo check
    cargo test -p openmines-world

# Запуск тестов Cargo
test:
    cargo test --all-targets --all-features

# Запуск тестов через cargo-nextest
nextest:
    cargo nextest run --all-targets --all-features

# Запуск конкретного теста (пример: just test-one lost_wake)
test-one test_name:
    cargo test {{test_name}} -- --nocapture

# Запуск clippy с жесткими флагами
clippy:
    cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery

# Запуск форматирования
fmt:
    cargo fmt --all

# Сброс базы данных игроков
wipe-players:
    ./scripts/wipe-players.sh

# Очистить кэш сборки Cargo target
prune-cache:
    ./scripts/target-cache.sh --prune

# Полная очистка кэша target
clean-cache:
    ./scripts/target-cache.sh --clean

# Сборка Unity-клиента
build-client:
    ./scripts/build-client.sh
