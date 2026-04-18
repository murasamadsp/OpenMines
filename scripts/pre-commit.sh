#!/usr/bin/env bash

set -euo pipefail

echo "==> Running rustfmt check"
cargo fmt --all -- --check

echo "==> Running clippy strict checks"
cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery

if command -v cargo-deny >/dev/null 2>&1; then
  echo "==> Running dependency policy check"
  cargo deny check
else
  echo "==> cargo-deny not installed, skipping (bootstrap-quality.sh will install it)"
fi

if command -v cargo-audit >/dev/null 2>&1; then
  echo "==> Running security audit"
  cargo audit
else
  echo "==> cargo-audit not installed, skipping (bootstrap-quality.sh will install it)"
fi

if command -v cargo-machete >/dev/null 2>&1; then
  echo "==> Checking unused dependencies"
  cargo machete
else
  echo "==> cargo-machete not installed, skipping (bootstrap-quality.sh will install it)"
fi

echo "==> Running tests"
cargo test --all-targets --all-features

if [[ "${PRE_COMMIT_EXTENDED:-0}" == "1" ]]; then
  if command -v cargo-outdated >/dev/null 2>&1; then
    echo "==> Checking outdated dependencies"
    cargo outdated
  else
    echo "==> cargo-outdated not installed, skipping"
  fi

  if command -v cargo-geiger >/dev/null 2>&1; then
    echo "==> Auditing unsafe usage (cargo-geiger)"
    cargo geiger
  else
    echo "==> cargo-geiger not installed, skipping"
  fi

  if command -v cargo-bloat >/dev/null 2>&1; then
    echo "==> Checking biggest binary contributors"
    cargo bloat --release -n 25
  else
    echo "==> cargo-bloat not installed, skipping"
  fi

  echo "==> Building docs"
  cargo doc --no-deps --all-features
fi
