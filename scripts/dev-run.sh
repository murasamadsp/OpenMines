#!/usr/bin/env bash
# Fast local server runner. Uses sccache when available, without making it a
# hard requirement for every developer environment.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER="${RUSTC_WRAPPER:-sccache}"
fi

exec cargo run --bin openmines-server -- "$@"
