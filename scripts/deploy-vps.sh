#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/vps-common.sh
source "$ROOT_DIR/scripts/vps-common.sh"

vps_export_defaults
vps_need_commands

vps_assert_reachable "$REMOTE_HOST"

vps_rsync_repo "$ROOT_DIR" "$REMOTE_HOST" "$REMOTE_DIR"

echo "==> Собираю и рестартую сервис (DOCKER_BUILDKIT=1): docker compose -f $COMPOSE_FILE up -d --build --force-recreate $SERVICE"
vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" up -d --build --force-recreate "$SERVICE"

# Образ кладёт `cp -n` в CMD — первый том сохраняет старые config/cells/buildings. Подтягиваем с rsync-дерева в /data.
vps_sync_runtime_configs "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" "$SERVICE"

echo "==> Проверяю статус контейнера $SERVICE"
vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" ps "$SERVICE"

echo "==> Последние логи $SERVICE"
vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" logs --tail=40 "$SERVICE"
