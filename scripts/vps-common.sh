# Общие переменные и синхронизация для deploy / full-reinstall (source из bash).
# shellcheck shell=bash

vps_export_defaults() {
  export LANG=C
  export LC_ALL=C
  export LANGUAGE=C
  export REMOTE_HOST="${REMOTE_HOST:-vps}"
  export REMOTE_DIR="${REMOTE_DIR:-/home/admin/openmines-deploy}"
  export COMPOSE_FILE="${COMPOSE_FILE:-ops/docker-compose.vps.yml}"
  export SERVICE="${SERVICE:-server}"
  # Удалённый `docker compose build`: только BuildKit (Compose / buildx), не classic builder.
  export VPS_DOCKER_COMPOSE_LEAD="${VPS_DOCKER_COMPOSE_LEAD:-DOCKER_BUILDKIT=1}"
  # rsync вызывает ssh: не передаём локаль macOS на VPS (иначе bash: setlocale: LC_ALL: cannot change locale).
  export RSYNC_RSH="env LC_ALL=C LANG=C LANGUAGE=C ssh -o BatchMode=yes -o ConnectTimeout=120 -o ServerAliveInterval=30"
}

# SSH на VPS: нейтральная локаль (иначе с macOS уезжает en_US.UTF-8 → setlocale на Debian без locale-gen).
vps_ssh() {
  LC_ALL=C LANG=C LANGUAGE=C \
    LC_ADDRESS=C LC_COLLATE=C LC_CTYPE=C LC_IDENTIFICATION=C LC_MEASUREMENT=C \
    LC_MESSAGES=C LC_MONETARY=C LC_NAME=C LC_NUMERIC=C LC_PAPER=C LC_TELEPHONE=C LC_TIME=C \
    ssh -o BatchMode=yes -o ConnectTimeout=10 "$@"
}

vps_need_commands() {
  local cmd
  for cmd in rsync ssh; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      echo "ERROR: required command '$cmd' is not available" >&2
      exit 1
    fi
  done
}

vps_assert_reachable() {
  local remote_host=$1
  echo "==> Проверка доступности VPS ($remote_host)"
  vps_ssh "$remote_host" "echo OK" >/dev/null
}

# Синхронизирует на VPS только то, что нужно для сборки Docker-образа сервера.
vps_rsync_repo() {
  local root_dir=$1
  local remote_host=$2
  local remote_dir=$3

  echo "==> Синхронизирую серверные файлы в $remote_host:$remote_dir"
  rsync -az --delete \
    --progress \
    --exclude ".git/" \
    --exclude "client/" \
    --exclude "target/" \
    --exclude "node_modules/" \
    --exclude "artifacts/" \
    --exclude "tools/" \
    --exclude ".claude/" \
    --exclude ".omc/" \
    --exclude "*.db" \
    --exclude "*.db-shm" \
    --exclude "*.db-wal" \
    --exclude "*.mapb" \
    "$root_dir/server" \
    "$root_dir/Cargo.toml" \
    "$root_dir/Cargo.lock" \
    "$root_dir/cells.json" \
    "$root_dir/buildings.json" \
    "$root_dir/config.json" \
    "$root_dir/.dockerignore" \
    "$remote_host:$remote_dir/"

  rsync -az --delete \
    "$root_dir/ops/" \
    "$remote_host:$remote_dir/ops/"
}

vps_ssh_compose() {
  local remote_host=$1
  local remote_dir=$2
  local compose_file=$3
  shift 3
  # shellcheck disable=SC2145
  vps_ssh "$remote_host" \
    "cd $(printf '%q' "$remote_dir") && $VPS_DOCKER_COMPOSE_LEAD docker compose -f $(printf '%q' "$compose_file") $(printf '%q ' "$@")"
}

vps_ssh_compose_multi() {
  local remote_host=$1
  local remote_dir=$2
  local base_compose_file=$3
  shift 3

  local compose_flags
  compose_flags="-f $(printf '%q' "$base_compose_file")"
  while [[ "${1:-}" == "--compose" ]]; do
    local extra=$2
    compose_flags+=" -f $(printf '%q' "$extra")"
    shift 2
  done

  # shellcheck disable=SC2145
  vps_ssh "$remote_host" \
    "cd $(printf '%q' "$remote_dir") && $VPS_DOCKER_COMPOSE_LEAD docker compose $compose_flags $(printf '%q ' "$@")"
}

vps_sync_runtime_configs() {
  local remote_host=$1
  local remote_dir=$2
  local compose_file=$3
  local service=$4

  echo "==> Синхронизирую config.json, cells.json, buildings.json в том контейнера (/data)"
  vps_ssh_compose "$remote_host" "$remote_dir" "$compose_file" cp config.json "$service:/data/config.json"
  vps_ssh_compose "$remote_host" "$remote_dir" "$compose_file" cp cells.json "$service:/data/cells.json"
  vps_ssh_compose "$remote_host" "$remote_dir" "$compose_file" cp buildings.json "$service:/data/buildings.json"
  echo "==> Рестарт $service (подхватить новые config/cells/buildings)"
  vps_ssh_compose "$remote_host" "$remote_dir" "$compose_file" restart "$service"
}
