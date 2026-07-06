#!/usr/bin/env bash
# Первичная настройка окружения разработки.
# Запускать один раз после клонирования репозитория.

set -euo pipefail

echo "==> Устанавливаем компоненты rustup"
rustup component add rustfmt clippy

echo "==> Устанавливаем cargo-инструменты качества"
cargo install --locked cargo-nextest
cargo install --locked cargo-llvm-cov
cargo install --locked cargo-hack
cargo install --locked cargo-shear
cargo install --locked cargo-mutants
cargo install --locked cargo-vet
cargo install --locked cargo-deny cargo-audit cargo-machete cargo-outdated cargo-geiger cargo-bloat
cargo install --locked sccache --no-default-features

echo "==> Подключаем git-хук из .githooks/ (tracked, не требует pre-commit)"
git config core.hooksPath .githooks
echo "    git config core.hooksPath = .githooks  ✓"

echo "Bootstrap complete."
