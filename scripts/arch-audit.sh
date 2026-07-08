#!/usr/bin/env bash
# Report known architecture leaks without gating CI.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> direct ECS writes from session layer"
rg -n 'state\.ecs\.write\(' crates/openmines-server/src/net/session || true

echo
echo "==> async task spawns from gameplay modules"
rg -n 'tokio::spawn|spawn_blocking' crates/openmines-server/src/game || true

echo
echo "==> session handlers that dispatch gameplay directly"
rg -n 'dispatch_ty_packet|state\.incoming_actions|state\.modify_player|state\.modify_building' crates/openmines-server/src/net/session || true
