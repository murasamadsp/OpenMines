#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/vps-common.sh
source "$ROOT_DIR/scripts/vps-common.sh"

vps_export_defaults
vps_need_commands

vps_assert_reachable "$REMOTE_HOST"

vps_rsync_repo "$ROOT_DIR" "$REMOTE_HOST" "$REMOTE_DIR"

vps_build_openmines_binary "$REMOTE_HOST" "$REMOTE_DIR"

echo "==> Сборка образа (Dockerfile.vps, BuildKit), затем up (DOCKER_BUILDKIT=1)"
COMPOSE_DOCKER_CLI_BUILD=1 DOCKER_BUILDKIT=1 vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" build "$SERVICE"
COMPOSE_DOCKER_CLI_BUILD=1 DOCKER_BUILDKIT=1 vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" up -d --force-recreate "$SERVICE"

echo "==> Проверяю статус контейнера $SERVICE"
vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" ps "$SERVICE"

echo "==> Последние логи $SERVICE"
vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" logs --tail=40 "$SERVICE"
