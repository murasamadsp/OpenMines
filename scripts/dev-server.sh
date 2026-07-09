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
ADMIN_TOKEN="${M3R_DEV_ADMIN_TOKEN:-local-dev-admin}"
WORLD_CHUNKS_W="${M3R_DEV_WORLD_CHUNKS_W:-8}"
WORLD_CHUNKS_H="${M3R_DEV_WORLD_CHUNKS_H:-8}"
LOG_FILTER="${M3R_DEV_LOG:-openmines_server=info,openmines_runtime=info,openmines_world=info,openmines_storage=info,tickprof=error,scheduler=error,tokio=warn,h2=warn}"

mkdir -p "$CONFIG_DIR" "$STATE_DIR"
ln -sfn "$ROOT/configs/cells.json" "$CONFIG_DIR/cells.json"
ln -sfn "$ROOT/configs/buildings.json" "$CONFIG_DIR/buildings.json"
python3 - "$ROOT/configs/config.json" "$CONFIG_DIR/config.json" \
  "$PORT" "$WORLD_CHUNKS_W" "$WORLD_CHUNKS_H" "$LOG_FILTER" <<'PY'
import json
import sys

src, dst, port, chunks_w, chunks_h, log_filter = sys.argv[1:]
with open(src, encoding="utf-8") as f:
    cfg = json.load(f)

cfg["world_name"] = "local-dev"
cfg["port"] = int(port)
cfg["world_chunks_w"] = int(chunks_w)
cfg["world_chunks_h"] = int(chunks_h)
cfg["data_dir"] = "data"
cfg["logging"] = {"filter": log_filter, "format": "compact", "file": None}

with open(dst, "w", encoding="utf-8") as f:
    json.dump(cfg, f, ensure_ascii=False, indent=2)
    f.write("\n")
PY

echo "==> OpenMines local dev server"
echo "    root:      $ROOT"
echo "    workdir:   $WORK_DIR"
echo "    config:    $CONFIG_DIR/config.json"
echo "    state:     $STATE_DIR"
echo "    world:     ${WORLD_CHUNKS_W}x${WORLD_CHUNKS_H} chunks"
echo "    endpoint:  127.0.0.1:$PORT"
echo "    admin:     http://127.0.0.1:$ADMIN_PORT/?token=$ADMIN_TOKEN"
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
  M3R_ADMIN_TOKEN="$ADMIN_TOKEN" \
  cargo run --manifest-path "$ROOT/Cargo.toml" -- --admin-port "$ADMIN_PORT" "$@"
