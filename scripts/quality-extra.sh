#!/usr/bin/env bash
# Manual extended quality tooling. Requires tools installed by bootstrap-quality.sh.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/quality-common.sh
source "$ROOT_DIR/scripts/quality-common.sh"

usage() {
  cat <<EOF
Usage:
  scripts/quality-extra.sh test       Run nextest
  scripts/quality-extra.sh features   Run cargo-hack feature matrix
  scripts/quality-extra.sh deps       Run cargo-shear
  scripts/quality-extra.sh coverage   Run cargo-llvm-cov + nextest
  scripts/quality-extra.sh mutants    Run cargo-mutants
  scripts/quality-extra.sh vet        Run cargo-vet
  scripts/quality-extra.sh fmod       Check FMOD event bank contract
  scripts/quality-extra.sh cache      Show sccache stats
  scripts/quality-extra.sh stop-cache Stop sccache server
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
  fmod)
    quality_run_fmod_events
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
