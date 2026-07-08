#!/usr/bin/env bash
# Inspect or clear Cargo build cache for this workspace.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$ROOT/target"

usage() {
  cat <<EOF
Usage:
  scripts/target-cache.sh          Show target size breakdown
  scripts/target-cache.sh --prune  Delete known safe generated bloat
  scripts/target-cache.sh --clean  Delete full Cargo target directory
EOF
}

show_sizes() {
  if [[ ! -d "$TARGET" ]]; then
    echo "target cache is absent: $TARGET"
    return
  fi

  echo "==> target total"
  du -sh "$TARGET"
  echo
  echo "==> biggest target entries"
  du -sh "$TARGET"/* 2>/dev/null | sort -hr | head -40
  echo
  echo "==> known bloat"
  du -sh \
    "$TARGET"/debug/incremental \
    "$TARGET"/release/incremental \
    "$TARGET"/ra \
    "$TARGET"/debug/deps 2>/dev/null || true
}

prune_known_bloat() {
  if [[ ! -d "$TARGET" ]]; then
    echo "target cache is absent: $TARGET"
    return 1
  fi

  echo "==> deleting incremental/rust-analyzer caches"
  rm -rf "$TARGET"/debug/incremental "$TARGET"/release/incremental "$TARGET"/ra

  echo "==> deleting executable test binaries from target/debug/deps"
  if [[ -d "$TARGET/debug/deps" ]]; then
    find "$TARGET/debug/deps" -maxdepth 1 -type f -perm -111 \
      ! -name '*.d' \
      ! -name '*.rlib' \
      ! -name '*.rmeta' \
      ! -name '*.dSYM' \
      -delete

    find "$TARGET/debug/deps" -maxdepth 1 -type f -name '*.rcgu.o' -delete
  fi
}

case "${1:-}" in
  "")
    show_sizes
    ;;
  --prune)
    if prune_known_bloat; then
      echo
      show_sizes
    fi
    ;;
  --clean)
    echo "==> deleting full Cargo target cache"
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
