#!/usr/bin/env bash
# Единая точка входа для локальной проверки всех видов тестов и гейт-проверок.
# Запускает тот же набор, что CI, но без тяжёлых шагов (docs/coverage/bloat).
#
# Использование:
#   scripts/test-all.sh            # полный локальный прогон (fmt, guards, clippy, nextest, smoke)
#   scripts/test-all.sh --quick    # только nextest (без линтеров)
#   scripts/test-all.sh --no-smoke # nextest + clippy, но без wire-smoke

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/quality/common.sh
source "$ROOT_DIR/scripts/quality/common.sh"

QUICK=0
NO_SMOKE=0
for arg in "$@"; do
  case "$arg" in
    --quick) QUICK=1 ;;
    --no-smoke) NO_SMOKE=1 ;;
    -h|--help)
      echo "Usage: scripts/test-all.sh [--quick] [--no-smoke]"
      exit 0
      ;;
    *) echo "Unknown arg: $arg" >&2; exit 1 ;;
  esac
done

if [[ "$QUICK" -eq 1 ]]; then
  quality_run_tests
  exit 0
fi

quality_run_rustfmt_check
quality_run_arch_guard
quality_run_no_wire_in_lock
quality_run_tools_audit
quality_run_clippy_strict
quality_run_tests

if [[ "$NO_SMOKE" -eq 0 ]]; then
  quality_run_wire_smoke
fi

echo ""
echo "==> All local test gates passed."
