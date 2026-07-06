#!/usr/bin/env bash
# Modern Rust tooling entrypoint. Requires tools installed by bootstrap-quality.sh.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/quality-common.sh
source "$ROOT_DIR/scripts/quality-common.sh"

usage() {
  cat <<EOF
Usage:
  scripts/rust-modern.sh test       Run nextest
  scripts/rust-modern.sh features   Run cargo-hack feature matrix
  scripts/rust-modern.sh deps       Run cargo-shear
  scripts/rust-modern.sh coverage   Run cargo-llvm-cov + nextest
  scripts/rust-modern.sh mutants    Run cargo-mutants
  scripts/rust-modern.sh vet        Run cargo-vet
  scripts/rust-modern.sh cache      Show sccache stats
  scripts/rust-modern.sh stop-cache Stop sccache server
EOF
}

cd "$ROOT_DIR"

case "${1:-}" in
  test)
    quality_run_tests
    ;;
  features)
    quality_run_feature_matrix
    ;;
  deps)
    quality_run_dependency_shear
    ;;
  coverage)
    quality_run_coverage
    ;;
  mutants)
    quality_run_mutants
    ;;
  vet)
    quality_run_vet
    ;;
  cache)
    sccache --show-stats
    ;;
  stop-cache)
    sccache --stop-server
    ;;
  -h|--help)
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
