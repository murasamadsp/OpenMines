#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/quality-common.sh
source "$ROOT_DIR/scripts/quality-common.sh"

quality_run_rustfmt_check
quality_run_clippy_strict

echo "==> Running dependency policy checks"
quality_run_deny_if_available
quality_run_audit_if_available
quality_run_machete_if_available
quality_run_tests
quality_run_docs
