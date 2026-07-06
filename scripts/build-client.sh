#!/usr/bin/env bash
# Headless-сборка Unity-клиента (Win64 + macOS). Запускать на машине с Unity 6 и нужными build modules.
# Переменные:
#   UNITY_EDITOR — путь к бинарнику Unity (по умолчанию Hub + версия из ProjectVersion.txt)
#   CLIENT_BUILD_ROOT — каталог вывода (по умолчанию: <корень репо>/client-builds)
#
# Usage:
#   scripts/build-client.sh <all|win|mac> <local|dev|production>

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLIENT_DIR="$ROOT_DIR/client"
VERSION_FILE="$CLIENT_DIR/ProjectSettings/ProjectVersion.txt"
OUT_ROOT="${CLIENT_BUILD_ROOT:-$ROOT_DIR/client-builds}"

if [[ ! -f "$VERSION_FILE" ]]; then
  echo "ERROR: не найден $VERSION_FILE" >&2
  exit 1
fi

# m_EditorVersion: 6000.6.0a2
UNITY_VER="$(grep -E '^m_EditorVersion:' "$VERSION_FILE" | awk '{print $2}' | tr -d '\r')"
if [[ -z "$UNITY_VER" ]]; then
  echo "ERROR: не удалось прочитать версию Unity из $VERSION_FILE" >&2
  exit 1
fi

DEFAULT_UNITY_MAC="/Applications/Unity/Hub/Editor/${UNITY_VER}/Unity.app/Contents/MacOS/Unity"
DEFAULT_UNITY_WIN="/c/Program Files/Unity/Hub/Editor/${UNITY_VER}/Editor/Unity.exe"

if [[ -n "${UNITY_EDITOR:-}" ]]; then
  UNITY="$UNITY_EDITOR"
elif [[ -x "$DEFAULT_UNITY_MAC" ]]; then
  UNITY="$DEFAULT_UNITY_MAC"
elif [[ -f "/Applications/Unity/Hub/Editor/${UNITY_VER}/Unity.app/Contents/MacOS/Unity" ]]; then
  UNITY="/Applications/Unity/Hub/Editor/${UNITY_VER}/Unity.app/Contents/MacOS/Unity"
else
  echo "ERROR: Unity не найден. Установи Editor ${UNITY_VER} через Hub или задай UNITY_EDITOR." >&2
  echo "Ожидался: $DEFAULT_UNITY_MAC" >&2
  exit 1
fi

TARGET="${1:-}"
PROFILE="${2:-}"
if [[ -z "$TARGET" || -z "$PROFILE" ]]; then
  echo "Usage: $0 <all|win|mac> <local|dev|production>" >&2
  exit 2
fi

case "$TARGET" in
  win|windows|win64)
    METHOD="CommandLineBuild.BuildWindows64"
    ;;
  mac|macos|darwin|osx)
    METHOD="CommandLineBuild.BuildMac"
    ;;
  all)
    METHOD="CommandLineBuild.BuildAll"
    ;;
  *)
    echo "Usage: $0 <all|win|mac> <local|dev|production>" >&2
    exit 1
    ;;
esac

case "$PROFILE" in
  local|dev|production)
    ;;
  *)
    echo "Usage: $0 <all|win|mac> <local|dev|production>" >&2
    exit 1
    ;;
esac

mkdir -p "$OUT_ROOT"
export CLIENT_BUILD_ROOT="$OUT_ROOT"

echo "==> Unity: $UNITY"
echo "==> Project: $CLIENT_DIR"
echo "==> Output:  $OUT_ROOT"
echo "==> Method:  $METHOD"
echo "==> Env:     $PROFILE"

"$UNITY" -batchmode -nographics -quit \
  -projectPath "$CLIENT_DIR" \
  -executeMethod "$METHOD" \
  --openmines-env "$PROFILE" \
  -logFile "$OUT_ROOT/last-unity-build.log"

echo "==> Готово. Лог: $OUT_ROOT/last-unity-build.log"
ls -la "$OUT_ROOT/win-x64" 2>/dev/null || true
ls -la "$OUT_ROOT/macos" 2>/dev/null || true
