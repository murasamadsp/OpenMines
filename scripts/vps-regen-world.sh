#!/usr/bin/env bash
# Перегенерация мира на VPS: том /data сохраняется, удаляются только слои .mapb + здания в SQLite.
# Игроки и прочая БД не трогаются. M3R_REGEN_WORLD только на эту команду — в обычном shell не остаётся.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/vps-common.sh
source "$ROOT_DIR/scripts/vps-common.sh"

vps_export_defaults
vps_need_commands

echo "==> Rsync на VPS (compose + ops/docker-compose.regen-world.yml)"
vps_rsync_repo "$ROOT_DIR" "$REMOTE_HOST" "$REMOTE_DIR"

REGEN_OVERRIDE="ops/docker-compose.regen-world.yml"
echo "==> VPS $REMOTE_HOST — regen (override $REGEN_OVERRIDE; --force-recreate $SERVICE; VPS_REGEN_BUILD=1 для --build)"
compose_up_flags="-d --force-recreate"
if [[ "${VPS_REGEN_BUILD:-}" == "1" ]]; then
  compose_up_flags="-d --build --force-recreate"
fi
vps_ssh "$REMOTE_HOST" \
  "cd $(printf '%q' "$REMOTE_DIR") && $VPS_DOCKER_COMPOSE_LEAD docker compose -f $(printf '%q' "$COMPOSE_FILE") -f $(printf '%q' "$REGEN_OVERRIDE") up $compose_up_flags $(printf '%q' "$SERVICE")"

echo "==> Статус и хвост логов (regen)"
vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" ps "$SERVICE"
vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" logs --tail=40 "$SERVICE"

echo "==> Убираю M3R_REGEN_WORLD из контейнера (иначе каждый рестарт снова сотрёт мир)"
vps_ssh "$REMOTE_HOST" \
  "cd $(printf '%q' "$REMOTE_DIR") && $VPS_DOCKER_COMPOSE_LEAD docker compose -f $(printf '%q' "$COMPOSE_FILE") up -d --force-recreate $(printf '%q' "$SERVICE")"

echo "OK. Обычный deploy: deploy-vps.sh или compose up без M3R_REGEN_WORLD=1."
