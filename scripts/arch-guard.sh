#!/usr/bin/env bash
# Static architecture guard for runtime layering.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail=0

check_forbidden() {
  local label="$1"
  local path="$2"
  local pattern="$3"

  if rg -n "$pattern" "$path"; then
    echo "ERROR: forbidden $label in $path" >&2
    fail=1
  fi
}

check_forbidden "direct ECS write from TCP connection lifecycle" "server/src/net/session/connection.rs" 'state\.ecs\.write\('
check_forbidden "direct ECS schedule run from session layer" "server/src/net/session" '\.schedule\.write\(\)\.run\('
check_forbidden "direct background task spawn from gameplay modules" "server/src/game" 'tokio::spawn|spawn_blocking'

exit "$fail"
