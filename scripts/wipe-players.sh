#!/usr/bin/env bash
# Удаляет всех игроков и связанные данные (здания, кланы, заявки, сообщения чата).
# Чаты-каналы (таблица chats) не трогаем.
#
# Локально:  ./scripts/wipe-players.sh [путь/к/openmines.db] --yes
# Пример:    ./scripts/wipe-players.sh data/openmines.db --yes
# Путь по умолчанию: data/openmines.db (или M3R_DB_PATH)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DB="${1:-${M3R_DB_PATH:-$ROOT/data/openmines.db}}"

if [[ "${2:-}" != "--yes" ]]; then
  echo "Удалит ВСЕХ игроков, здания, кланы, заявки в клан, сообщения чата в БД:" >&2
  echo "  $DB" >&2
  echo "Запуск: $0 [путь/к/openmines.db] --yes" >&2
  exit 2
fi

if [[ ! -f "$DB" ]]; then
  echo "ERROR: файл не найден: $DB" >&2
  exit 1
fi

if ! command -v sqlite3 >/dev/null 2>&1; then
  echo "ERROR: нужен sqlite3 в PATH" >&2
  exit 1
fi

sqlite3 "$DB" <<'SQL'
PRAGMA foreign_keys = OFF;
BEGIN;
DELETE FROM buildings;
DELETE FROM chat_messages;
DELETE FROM clan_requests;
DELETE FROM clans;
DELETE FROM players;
DELETE FROM sqlite_sequence WHERE name IN ('players', 'buildings', 'clans', 'clan_requests', 'chat_messages');
COMMIT;
SQL

echo "OK: players + buildings + clans + clan_requests + chat_messages очищены в $DB"
