#!/usr/bin/env bash
#
# Полная переустановка OpenMines на VPS: остановка стека, опционально сброс тома с данными,
# затем синхронизация репозитория и сборка с нуля.
#
# Использование:
#   ./scripts/full-reinstall-vps.sh              # мягко: down + deploy (данные в томе сохраняются)
#   ./scripts/full-reinstall-vps.sh --wipe-data  # УДАЛЯЕТ Docker-том с БД и миром (после подтверждения)
#   ./scripts/full-reinstall-vps.sh --wipe-data --yes
#
# Переменные как у deploy-vps.sh: REMOTE_HOST, REMOTE_DIR, COMPOSE_FILE, SERVICE

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/vps-common.sh
source "$ROOT_DIR/scripts/vps-common.sh"

vps_export_defaults
vps_need_commands

WIPE_DATA=false
SKIP_CONFIRM=false
for arg in "$@"; do
  case "$arg" in
    --wipe-data) WIPE_DATA=true ;;
    --yes) SKIP_CONFIRM=true ;;
    -h|--help)
      sed -n '1,20p' "$0"
      exit 0
      ;;
  esac
done

vps_assert_reachable "$REMOTE_HOST"

if [[ "$WIPE_DATA" == true ]]; then
  echo "!!! ВНИМАНИЕ: будет удалён Docker-том с данными сервера (БД, мир), см. compose volume name."
  if [[ "$SKIP_CONFIRM" != true ]]; then
    read -r -p "Продолжить? [yes/N] " ans
    case "$ans" in
      yes|YES) ;;
      *) echo "Отмена."; exit 1 ;;
    esac
  fi
fi

echo "==> Останавливаю стек на $REMOTE_HOST ($REMOTE_DIR)"
vps_ssh "$REMOTE_HOST" "mkdir -p $(printf '%q' "$REMOTE_DIR")"
DOWN_FLAGS=(down --remove-orphans)
if [[ "$WIPE_DATA" == true ]]; then
  DOWN_FLAGS+=(--volumes)
fi
# Игнорируем ошибку, если стека ещё не было
vps_ssh_compose "$REMOTE_HOST" "$REMOTE_DIR" "$COMPOSE_FILE" "${DOWN_FLAGS[@]}" || true

if [[ "$WIPE_DATA" == true ]]; then
  echo "==> Удаляю локальный образ на VPS (чистая пересборка)"
  vps_ssh "$REMOTE_HOST" \
    "docker rmi openmines-server-dev:latest 2>/dev/null || true"
fi

echo "==> Деплой (sync + build + up)"
exec "$ROOT_DIR/scripts/deploy-vps.sh"
