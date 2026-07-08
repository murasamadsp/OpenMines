#!/usr/bin/env bash
# Fast local server runner. Uses sccache when available and provides the
# explicit local admin token required by the server fail-fast startup policy.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
fi

export M3R_ADMIN_TOKEN="${M3R_ADMIN_TOKEN:-local-dev-admin}"
ADMIN_PORT="${M3R_ADMIN_PORT:-8091}"

echo "==> OpenMines dev run"
echo "    admin: http://127.0.0.1:${ADMIN_PORT}/?token=${M3R_ADMIN_TOKEN}"
echo

exec cargo run --bin openmines-server -- "$@"
