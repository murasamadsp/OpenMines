#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/quality-common.sh
source "$ROOT_DIR/scripts/quality-common.sh"

quality_run_rustfmt_check
quality_run_clippy_strict
quality_run_deny_if_available
quality_run_audit_if_available
quality_run_machete_if_available
quality_run_tests

if [[ "${PRE_COMMIT_EXTENDED:-0}" == "1" ]]; then
  quality_run_outdated_if_available
  quality_run_geiger_if_available
  quality_run_bloat_if_available
  quality_run_docs
fi
