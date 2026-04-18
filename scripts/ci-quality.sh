#!/usr/bin/env bash

set -euo pipefail

echo "==> Running rustfmt check"
cargo fmt --all -- --check

echo "==> Running clippy strict checks"
cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery

echo "==> Running dependency policy checks"
cargo deny check
cargo audit

if command -v cargo-machete >/dev/null 2>&1; then
  echo "==> Checking unused dependencies"
  cargo machete
else
  echo "==> cargo-machete not installed, skipping"
fi

echo "==> Running tests"
cargo test --all-targets --all-features

echo "==> Running docs build"
cargo doc --no-deps --all-features
