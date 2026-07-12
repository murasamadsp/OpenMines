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
check_forbidden "gameplay delay timer in UI adapter" "crates/openmines-server/src/net/session/ui" 'tokio::time::sleep|tokio_handle\.spawn'
check_forbidden "wire or async capability in delayed consumable apply" "crates/openmines-server/src/game/logic/consumables.rs" 'crate::net|tokio::spawn|tokio::time|tokio_handle|state\.db'
check_forbidden "legacy delayed consumable path" "crates/openmines-server/src" 'use_protector|use_razryadka|prot_detonate|raz_detonate'
check_forbidden "async task spawn inside simulation owner" "crates/openmines-server/src/tasks/simulation" 'tokio::spawn|spawn_blocking'
check_forbidden "direct database access inside simulation owner" "crates/openmines-server/src/tasks/simulation" 'state\.db|\.db\.(insert|update|delete|save|add|finalize|list|get|load|create)'
check_forbidden "authoritative state access from presentation owner" "crates/openmines-server/src/net/presentation.rs" 'state\.(ecs|world|db)|state\.(query|modify)_[a-z_]+'
check_forbidden "legacy building-delete contract" "crates/openmines-server/src" 'BuildingRemoval|ApplyRemovedBuilding|delete_building_runtime|snapshot_building_removal|apply_removed_building|spawn_building_remove_task|prepare_building_removal|delete_destroyed_building_db'
check_forbidden "direct building delete outside persistence owner" "crates/openmines-server/src" '(state\.)?db\.delete_building\(|Database::delete_building\('
check_forbidden "untyped building delete storage API" "crates/openmines-storage/src" 'fn delete_building\(|fn delete_resp_building_and_clear_bindings\('

if rg -n 'GameState::new' crates/openmines-server/src --glob '*.rs' \
  | rg -v 'crates/openmines-server/src/(main|test_support)\.rs:'; then
  echo "ERROR: test GameState must be created by ServerTestHarness" >&2
  fail=1
fi
check_forbidden "local test-state fixture" "crates/openmines-server/src" 'struct [A-Za-z0-9_]*TestState'
if rg -n '^\s*fn drain_events' crates/openmines-server/src --glob '*.rs' \
  | rg -v 'crates/openmines-server/src/test_support\.rs:'; then
  echo "ERROR: packet drain helper must live in ServerTestHarness" >&2
  fail=1
fi
if rg -n '\bconnect_in_tick\(' crates/openmines-server/src --glob '*.rs' \
  | rg -v 'crates/openmines-server/src/(test_support|net/session/player/init)\.rs:'; then
  echo "ERROR: tests must connect through ServerTestHarness" >&2
  fail=1
fi
if rg -n -U '(\.database\(\)|\.state\s*\.\s*db)\s*\.\s*create_player\(' \
  crates/openmines-server/src --glob '*.rs' \
  | rg -v 'crates/openmines-server/src/test_support\.rs:'; then
  echo "ERROR: additional test players must be created by ServerTestHarness" >&2
  fail=1
fi
check_forbidden "item catalog outside game logic" "crates/openmines-server/src" \
  'PACK_NAMES|inventory_building_item_spec|building_db_code_for_item|shpaak_item_index'
if rg -n '"COCK"|"http://pi\.door/"' crates/openmines-server/src --glob '*.rs' \
  | rg -v 'crates/openmines-server/src/net/session/auth/mod\.rs:'; then
  echo "ERROR: auth world-info constants must have one owner" >&2
  fail=1
fi

scripts/ownership-audit.sh || fail=1
scripts/ub-audit.sh || fail=1

exit "$fail"
