# Общие шаги для ci-quality.sh и pre-commit.sh
# shellcheck shell=bash

quality_run_rustfmt_check() {
  echo "==> Running rustfmt check"
  cargo fmt --all -- --check
}

quality_run_clippy_strict() {
  echo "==> Running clippy strict checks"
  cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery
}

quality_run_deny_if_available() {
  if command -v cargo-deny >/dev/null 2>&1; then
    echo "==> Running dependency policy check"
    cargo deny check
  else
    echo "==> cargo-deny not installed, skipping"
  fi
}

quality_run_audit_if_available() {
  if command -v cargo-audit >/dev/null 2>&1; then
    echo "==> Running security audit"
    cargo audit
  else
    echo "==> cargo-audit not installed, skipping"
  fi
}

quality_run_machete_if_available() {
  if command -v cargo-machete >/dev/null 2>&1; then
    echo "==> Checking unused dependencies"
    cargo machete
  else
    echo "==> cargo-machete not installed, skipping"
  fi
}

quality_run_outdated_if_available() {
  if command -v cargo-outdated >/dev/null 2>&1; then
    echo "==> Checking outdated dependencies"
    cargo outdated
  else
    echo "==> cargo-outdated not installed, skipping"
  fi
}

quality_run_geiger_if_available() {
  if command -v cargo-geiger >/dev/null 2>&1; then
    echo "==> Auditing unsafe usage (cargo-geiger)"
    cargo geiger
  else
    echo "==> cargo-geiger not installed, skipping"
  fi
}

quality_run_bloat_if_available() {
  if command -v cargo-bloat >/dev/null 2>&1; then
    echo "==> Checking biggest binary contributors"
    cargo bloat --release -n 25
  else
    echo "==> cargo-bloat not installed, skipping"
  fi
}

quality_run_tests() {
  echo "==> Running tests"
  cargo test --all-targets --all-features
}

quality_run_docs() {
  echo "==> Running docs build"
  cargo doc --no-deps --all-features
}

