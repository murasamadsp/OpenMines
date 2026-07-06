#!/usr/bin/env bash
# Inspect or clear Cargo build cache for this workspace.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$ROOT/target"

usage() {
  cat <<EOF
Usage:
  scripts/target-cache.sh          Show target size breakdown
  scripts/target-cache.sh --clean  Delete target cache
EOF
}

case "${1:-}" in
  "")
    if [[ ! -d "$TARGET" ]]; then
      echo "target cache is absent: $TARGET"
      exit 0
    fi
    echo "==> target total"
    du -sh "$TARGET"
    echo
    echo "==> biggest target entries"
    du -sh "$TARGET"/* 2>/dev/null | sort -hr | head -40
    ;;
  --clean)
    echo "==> deleting Cargo target cache"
    echo "    $TARGET"
    rm -rf "$TARGET"
    echo "done"
    ;;
  -h|--help)
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
