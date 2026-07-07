# Общие шаги для ci-quality.sh и pre-commit.sh
# shellcheck shell=bash

: "${SCCACHE_CACHE_SIZE:=8G}"
export SCCACHE_CACHE_SIZE

# Авто-форматирование ПЕРЕД линтерами: форматируем только застейдженные .rs
# (чужой незакоммиченный WIP в working tree не трогаем) и ре-стейджим их, чтобы
# отформатированная версия попала в коммит и rustfmt --check ниже прошёл.
quality_run_rustfmt_apply_staged() {
  echo "==> Auto-formatting staged Rust files"
  local staged
  staged=$(git diff --cached --name-only --diff-filter=ACM -- '*.rs')
  if [ -n "$staged" ]; then
    echo "$staged" | xargs rustfmt --edition 2024
    echo "$staged" | xargs git add --
  fi
}

quality_run_rustfmt_check() {
  echo "==> Running rustfmt check"
  cargo fmt --all -- --check
}

quality_run_clippy_strict() {
  echo "==> Running clippy strict checks"
  RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery
}

quality_run_doctor() {
  echo "==> Running server doctor"
  RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 cargo run -- --doctor
}

quality_run_arch_guard() {
  echo "==> Running architecture guard"
  scripts/arch-guard.sh
}

quality_run_tools_audit() {
  echo "==> Running tools hygiene audit"
  scripts/tools-audit.sh
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
  echo "==> Running tests with cargo-nextest"
  RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 cargo nextest run --all-targets --all-features
}

quality_run_wire_smoke() {
  echo "==> Running local wire smoke"
  scripts/dev-smoke.sh
}

quality_run_docs() {
  echo "==> Running docs build"
  RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 cargo doc --no-deps --all-features
}

quality_run_feature_matrix() {
  echo "==> Running feature matrix with cargo-hack"
  RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0 cargo hack check --workspace --all-targets --feature-powerset --depth 2
}

quality_run_dependency_shear() {
  echo "==> Running dependency placement check with cargo-shear"
  cargo shear
}

quality_run_coverage() {
  echo "==> Running LLVM coverage"
  RUSTC_WRAPPER=sccache cargo llvm-cov nextest --workspace --all-features --lcov --output-path target/llvm-cov/lcov.info
}

quality_run_mutants() {
  echo "==> Running mutation tests"
  RUSTC_WRAPPER=sccache cargo mutants --workspace --in-place
}

quality_run_vet() {
  echo "==> Running supply-chain audit with cargo-vet"
  cargo vet
}
