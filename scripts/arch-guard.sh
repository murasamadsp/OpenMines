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

check_forbidden "direct ECS write from TCP connection lifecycle" "crates/openmines-server/src/net/session/connection.rs" 'state\.ecs\.write\('
check_forbidden "direct ECS schedule run from session layer" "crates/openmines-server/src/net/session" '\.schedule\.write\(\)\.run\('
check_forbidden "direct background task spawn from gameplay modules" "crates/openmines-server/src/game" 'tokio::spawn|spawn_blocking'
check_forbidden "async task spawn inside simulation owner" "crates/openmines-server/src/tasks/simulation" 'tokio::spawn|spawn_blocking'
check_forbidden "direct database access inside simulation owner" "crates/openmines-server/src/tasks/simulation" 'state\.db|\.db\.(insert|update|delete|save|add|finalize|list|get|load|create)'

scripts/ownership-audit.sh || fail=1
scripts/ub-audit.sh || fail=1

exit "$fail"
