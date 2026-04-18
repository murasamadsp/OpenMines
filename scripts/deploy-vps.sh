#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/vps-common.sh
source "$ROOT_DIR/scripts/vps-common.sh"

vps_export_defaults
vps_need_commands

echo "==> Проверка доступности VPS ($REMOTE_HOST)"
vps_ssh "$REMOTE_HOST" "echo OK" >/dev/null

vps_rsync_repo "$ROOT_DIR" "$REMOTE_HOST" "$REMOTE_DIR"

echo "==> Собираю и рестартую сервис (DOCKER_BUILDKIT=1): docker compose -f $COMPOSE_FILE up -d --build --force-recreate $SERVICE"
vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" up -d --build --force-recreate "$SERVICE"

# Образ кладёт `cp -n` в CMD — первый том сохраняет старый config/cells. Подтягиваем с rsync-дерева в /data.
echo "==> Синхронизирую config.json и cells.json в том контейнера (/data)"
vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" cp config.json "$SERVICE:/data/config.json"
vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" cp cells.json "$SERVICE:/data/cells.json"
echo "==> Рестарт $SERVICE (подхватить новый config/cells)"
vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" restart "$SERVICE"

echo "==> Проверяю статус контейнера $SERVICE"
vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" ps "$SERVICE"

echo "==> Последние логи $SERVICE"
vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" logs --tail=40 "$SERVICE"
