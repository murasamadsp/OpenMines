#!/usr/bin/env bash
# Static audit for explicit Rust soundness boundaries.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> Checking explicit unsafe allowlist"
python3 scripts/ub-audit-unsafe.py || exit 1

echo "==> Checking raw pointer / UnsafeCell / PhantomData / FFI boundaries"
if rg -n 'UnsafeCell|PhantomData|NonNull|MaybeUninit|ManuallyDrop|\*mut\s|\*const\s|extern\s+"|repr\s*\(\s*packed' crates -g '*.rs'; then
  echo "ERROR: raw memory/FFI boundary found; add a reviewed abstraction and update scripts/ub-audit.sh" >&2
  exit 1
fi

echo "==> Checking hot adjacent atomics"
python3 scripts/ub-audit-atomics.py || exit 1
