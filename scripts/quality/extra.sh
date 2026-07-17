#!/usr/bin/env bash
# Manual extended quality tooling. Requires tools installed by bootstrap-quality.sh.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=scripts/quality/common.sh
source "$ROOT_DIR/scripts/quality/common.sh"

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
  scripts/quality-extra.sh ub         Run unsafe/soundness boundary audit
  scripts/quality-extra.sh arch       Read-only architecture leak report
  scripts/quality-extra.sh outdated   Check outdated dependencies
  scripts/quality-extra.sh geiger     Audit unsafe via cargo-geiger
  scripts/quality-extra.sh bloat      Report binary bloat
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
  ub)
    scripts/guards/soundness.sh
    ;;
  arch)
    scripts/guards/arch.sh --report
    ;;
  outdated)
    quality_run_outdated_if_available
    ;;
  geiger)
    quality_run_geiger_if_available
    ;;
  bloat)
    quality_run_bloat_if_available
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
