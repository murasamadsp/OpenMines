#!/usr/bin/env bash
# Первичная настройка окружения разработки.
# Запускать один раз после клонирования репозитория.

set -euo pipefail

echo "==> Устанавливаем компоненты rustup"
rustup component add rustfmt clippy

echo "==> Устанавливаем cargo-инструменты качества"
cargo install --locked cargo-deny cargo-audit cargo-machete cargo-outdated cargo-geiger cargo-bloat

echo "==> Подключаем git-хук из .githooks/ (tracked, не требует pre-commit)"
git config core.hooksPath .githooks
echo "    git config core.hooksPath = .githooks  ✓"

echo "Bootstrap complete."
