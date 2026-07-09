#!/usr/bin/env bash
# Inspect or prune Cargo build cache for this workspace.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$ROOT/target"
MAX_SIZE_GIB="${OPENMINES_TARGET_MAX_GIB:-8}"

usage() {
  cat <<EOF
Usage:
  scripts/target-cache.sh                    Show target size breakdown and budget
  scripts/target-cache.sh --prune --dry-run  Show what soft prune would delete
  scripts/target-cache.sh --prune            Delete soft, rebuildable bloat
  scripts/target-cache.sh --clean            Delete full Cargo target directory

Environment:
  OPENMINES_TARGET_MAX_GIB=8                 Warning budget for target/
EOF
}

bytes_of() {
  local path="$1"
  if [[ -e "$path" ]]; then
    du -sk "$path" 2>/dev/null | awk '{ print $1 * 1024 }'
  else
    echo 0
  fi
}

human_bytes() {
  local bytes="$1"
  awk -v b="$bytes" 'BEGIN {
    split("B KiB MiB GiB TiB", u, " ");
    i = 1;
    while (b >= 1024 && i < 5) { b /= 1024; i++ }
    if (i == 1) printf "%d %s", b, u[i]; else printf "%.1f %s", b, u[i]
  }'
}

print_size() {
  local label="$1"
  local path="$2"
  local bytes
  bytes="$(bytes_of "$path")"
  printf "%-28s %10s  %s\n" "$label" "$(human_bytes "$bytes")" "$path"
}

show_sizes() {
  if [[ ! -d "$TARGET" ]]; then
    echo "target cache is absent: $TARGET"
    return
  fi

  local total max_bytes
  total="$(bytes_of "$TARGET")"
  max_bytes=$((MAX_SIZE_GIB * 1024 * 1024 * 1024))

  echo "==> target budget"
  printf "%-28s %10s  %s\n" "target total" "$(human_bytes "$total")" "$TARGET"
  printf "%-28s %10s\n" "soft budget" "${MAX_SIZE_GIB} GiB"
  if (( total > max_bytes )); then
    printf "WARN: target exceeds soft budget by %s\n" "$(human_bytes "$((total - max_bytes))")" >&2
  fi
  echo

  echo "==> biggest target entries"
  du -sh "$TARGET"/* 2>/dev/null | sort -hr | head -40
  echo

  echo "==> known bloat"
  print_size "debug incremental" "$TARGET/debug/incremental"
  print_size "release incremental" "$TARGET/release/incremental"
  print_size "rust-analyzer cache" "$TARGET/ra"
  print_size "debug deps total" "$TARGET/debug/deps"
  echo
  echo "==> soft-prune candidates"
  print_soft_prune_candidates
}

sum_find_bytes() {
  local dir="$1"
  shift
  if [[ ! -d "$dir" ]]; then
    echo 0
    return
  fi
  find "$dir" "$@" -print0 2>/dev/null \
    | xargs -0 du -sk 2>/dev/null \
    | awk '{ total += $1 } END { print total * 1024 }'
}

print_soft_prune_candidates() {
  print_size "debug incremental" "$TARGET/debug/incremental"
  print_size "release incremental" "$TARGET/release/incremental"
  print_size "rust-analyzer cache" "$TARGET/ra"

  local test_bins rcgu_objects
  test_bins="$(sum_find_bytes "$TARGET/debug/deps" -maxdepth 1 -type f -perm -111 ! -name '*.d' ! -name '*.rlib' ! -name '*.rmeta' ! -name '*.dSYM')"
  rcgu_objects="$(sum_find_bytes "$TARGET/debug/deps" -maxdepth 1 -type f -name '*.rcgu.o')"
  printf "%-28s %10s  %s\n" "debug test binaries" "$(human_bytes "$test_bins")" "$TARGET/debug/deps"
  printf "%-28s %10s  %s\n" "debug rcgu objects" "$(human_bytes "$rcgu_objects")" "$TARGET/debug/deps"
}

prune_known_bloat() {
  local dry_run="$1"
  if [[ ! -d "$TARGET" ]]; then
    echo "target cache is absent: $TARGET"
    return 1
  fi

  echo "==> soft-prune candidates"
  print_soft_prune_candidates
  if [[ "$dry_run" == "1" ]]; then
    echo
    echo "dry-run: no files deleted"
    return
  fi

  echo
  echo "==> deleting incremental/rust-analyzer caches"
  rm -rf -- "$TARGET"/debug/incremental "$TARGET"/release/incremental "$TARGET"/ra

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
    dry_run=0
    if [[ "${2:-}" == "--dry-run" ]]; then
      dry_run=1
    elif [[ -n "${2:-}" ]]; then
      usage >&2
      exit 2
    fi
    if prune_known_bloat "$dry_run"; then
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
