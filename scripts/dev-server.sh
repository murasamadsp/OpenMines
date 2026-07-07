#!/usr/bin/env bash
# Local development server loop. Uses an explicit checked-in shape of runtime
# config inside .local/ so checks do not depend on or mutate any remote/VPS
# state.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK_DIR="$ROOT/.local/openmines-dev"
CONFIG_DIR="$WORK_DIR/configs"
STATE_DIR="$WORK_DIR/data"
PORT="${M3R_DEV_PORT:-8090}"
ADMIN_PORT="${M3R_DEV_ADMIN_PORT:-8091}"
WORLD_CHUNKS_W="${M3R_DEV_WORLD_CHUNKS_W:-8}"
WORLD_CHUNKS_H="${M3R_DEV_WORLD_CHUNKS_H:-8}"
LOG_FILTER="${M3R_DEV_LOG:-openmines_server=info,openmines_shared=info,tickprof=error,scheduler=error,tokio=warn,h2=warn}"

mkdir -p "$CONFIG_DIR" "$STATE_DIR"
ln -sfn "$ROOT/configs/cells.json" "$CONFIG_DIR/cells.json"
ln -sfn "$ROOT/configs/buildings.json" "$CONFIG_DIR/buildings.json"
cat > "$CONFIG_DIR/config.json" <<JSON
{
  "world_name": "local-dev",
  "port": $PORT,
  "world_chunks_w": $WORLD_CHUNKS_W,
  "world_chunks_h": $WORLD_CHUNKS_H,
  "data_dir": "data",
  "logging": {
    "filter": "$LOG_FILTER",
    "format": "compact",
    "file": null
  },
  "cron": {
    "hourly_log_enabled": true
  },
  "gameplay": {
    "cooldowns": {
      "dig_ms": 200,
      "build_ms": 200,
      "geo_ms": 200
    },
    "combat": {
      "gun_fire_interval_ms": 500,
      "gun_damage": 60,
      "gun_radius_cells": 20
    },
    "bonus": {
      "cooldown_secs": 25200,
      "reward_money": 1000000
    },
    "skills": {
      "upgrade_cost_base": 100
    },
    "spawn": {
      "x": 10,
      "y": 10
    },
    "programmator": {
      "direct_action_delay_us": 333333,
      "blocked_move_penalty_ms": 200,
      "min_move_delay_ms": 20
    },
    "schedules": {
      "hazards_ms": 10,
      "physics_ms": 100,
      "guns_ms": 100,
      "programmator_ms": 100,
      "alive_ms": 5000,
      "building_effects_ms": 1000,
      "hourly_damage_ms": 3600000,
      "game_loop_tick_rate_ms": 10,
      "game_loop_panic_backoff_ms": 200,
      "session_disconnect_timeout_secs": 30
    },
    "rate_limits": {
      "chat_burst": 5,
      "chat_per_sec": 3,
      "gui_burst": 10,
      "gui_per_sec": 5
    }
  }
}
JSON

echo "==> OpenMines local dev server"
echo "    root:      $ROOT"
echo "    workdir:   $WORK_DIR"
echo "    config:    $CONFIG_DIR/config.json"
echo "    state:     $STATE_DIR"
echo "    world:     ${WORLD_CHUNKS_W}x${WORLD_CHUNKS_H} chunks"
echo "    endpoint:  127.0.0.1:$PORT"
echo "    admin:     http://127.0.0.1:$ADMIN_PORT/?token=admin"
echo "    log:       $LOG_FILTER"
echo
echo "Unity Editor:"
echo "    OpenMines -> Environment -> Local"
echo "    then press Play. The client reads Assets/Resources/OpenMines/EnvironmentCatalog.asset."
echo
echo "Wire smoke:"
echo "    scripts/dev-smoke.sh"
echo

cd "$WORK_DIR"
exec env \
  -u M3R_DATA_DIR \
  -u M3R_PORT \
  -u M3R_USE_CTRL_C \
  -u M3R_ABORT_ON_PANIC \
  -u M3R_LOG \
  -u RUST_LOG \
  cargo run --manifest-path "$ROOT/Cargo.toml" -- --admin-port "$ADMIN_PORT" "$@"
