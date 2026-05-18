#!/usr/bin/env bash

set -euo pipefail

rustup component add rustfmt clippy

cargo install --locked cargo-deny cargo-audit cargo-machete cargo-outdated cargo-geiger cargo-bloat

if command -v pre-commit >/dev/null 2>&1; then
  pre-commit install
else
  echo "pre-commit not found, install it manually (for example: brew install pre-commit)"
fi

echo "Bootstrap complete."
