#!/usr/bin/env bash
# Static audit for Rust ownership/cancellation hazards in server code.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail=0

err() {
  echo "ERROR: $*" >&2
  fail=1
}

echo "==> Checking async trait-object allocation hazards"
if rg -n '#\s*\[\s*async_trait|async_trait::async_trait' crates/openmines-server/src crates/openmines-storage/src; then
  err "async_trait is forbidden in server/storage live code; prefer inherent async fns or explicit actor messages"
fi

if rg -n 'Box\s*<\s*dyn\s+Future|Pin\s*<\s*Box\s*<\s*dyn\s+Future' crates/openmines-server/src; then
  err "boxed dyn Future is forbidden in openmines-server hot code"
fi

echo "==> Checking sync lock guards across await"
python3 scripts/ownership-audit-lock-guard.py || fail=1

echo "==> Ownership audit summary"
printf 'Arc<GameState> refs: '
rg -n 'Arc\s*<\s*GameState|Arc\s*<\s*crate::game::GameState|std::sync::Arc\s*<\s*game::GameState' crates/openmines-server/src -g '*.rs' | wc -l | tr -d ' '
printf 'sync lock guard sites: '
rg -n 'let\s+(mut\s+)?[A-Za-z_][A-Za-z0-9_]*\s*=\s*.*\.(lock|read|write)\s*\(\)\s*;' crates/openmines-server/src -g '*.rs' | wc -l | tr -d ' '

exit "$fail"
