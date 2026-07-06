#!/usr/bin/env bash
# Local development server loop. Uses an explicit checked-in shape of runtime
# config inside .local/ so checks do not depend on or mutate any remote/VPS
# state.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK_DIR="$ROOT/.local/openmines-dev"
CONFIG_DIR="$WORK_DIR/configs"
STATE_DIR="$WORK_DIR/data"
PORT="8090"

mkdir -p "$CONFIG_DIR" "$STATE_DIR"
ln -sfn "$ROOT/configs/cells.json" "$CONFIG_DIR/cells.json"
ln -sfn "$ROOT/configs/buildings.json" "$CONFIG_DIR/buildings.json"
cat > "$CONFIG_DIR/config.json" <<JSON
{
  "world_name": "local-dev",
  "port": $PORT,
  "world_chunks_w": 64,
  "world_chunks_h": 64,
  "data_dir": "data",
  "logging": {
    "filter": "openmines_server=debug,openmines_server::net::session=debug,tokio=warn,h2=warn",
    "format": "compact",
    "file": null
  },
  "cron": {
    "hourly_log_enabled": true
  },
  "gameplay": {
    "cooldowns": {
      "dig_ms": 200,
      "build_ms": 200
    },
    "skills": {
      "upgrade_cost_base": 100
    }
  }
}
JSON

echo "==> OpenMines local dev server"
echo "    root:      $ROOT"
echo "    workdir:   $WORK_DIR"
echo "    config:    $CONFIG_DIR/config.json"
echo "    state:     $STATE_DIR"
echo "    endpoint:  127.0.0.1:$PORT"
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
  cargo run --manifest-path "$ROOT/Cargo.toml" -- "$@"
