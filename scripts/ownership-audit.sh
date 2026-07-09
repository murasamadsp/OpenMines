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
python3 - <<'PY'
from pathlib import Path
import re
import sys

ROOTS = [Path("crates/openmines-server/src")]
GUARD_RE = re.compile(
    r"^\s*let\s+(?:mut\s+)?(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*=\s*.*\.(?:lock|read|write)\s*\(\)\s*;"
)
DROP_RE = re.compile(r"\bdrop\s*\(\s*(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*\)")


def brace_delta(line: str) -> int:
    line = re.sub(r'"(?:\\.|[^"\\])*"', '""', line)
    line = re.sub(r"//.*", "", line)
    return line.count("{") - line.count("}")


errors: list[str] = []
for root in ROOTS:
    for path in root.rglob("*.rs"):
        depth = 0
        guards: list[tuple[str, int, int]] = []
        cfg_test_next = False
        test_depth: int | None = None

        for lineno, line in enumerate(path.read_text(errors="ignore").splitlines(), start=1):
            stripped = line.strip()
            delta = brace_delta(line)

            if test_depth is not None and depth < test_depth:
                test_depth = None
            if stripped.startswith("#[cfg(test)]"):
                cfg_test_next = True

            starts_test_mod = cfg_test_next and re.search(r"\bmod\s+tests\b", line)
            if starts_test_mod:
                test_depth = depth + max(delta, 1)
                cfg_test_next = False
            elif stripped and not stripped.startswith("#["):
                cfg_test_next = False

            in_test_mod = test_depth is not None and depth >= test_depth
            if not in_test_mod:
                match = GUARD_RE.match(line)
                if match:
                    guards.append((match.group("name"), depth, lineno))

                for drop_match in DROP_RE.finditer(line):
                    name = drop_match.group("name")
                    guards = [g for g in guards if g[0] != name]

                if ".await" in line and guards:
                    held = ", ".join(f"{name}@{start}" for name, _, start in guards)
                    errors.append(f"{path}:{lineno}: await while sync guard is live: {held}")

            depth += delta
            guards = [g for g in guards if depth >= g[1]]

if errors:
    print("\n".join(errors))
    sys.exit(1)
PY
if [[ $? -ne 0 ]]; then
  fail=1
fi

echo "==> Ownership audit summary"
printf 'Arc<GameState> refs: '
rg -n 'Arc\s*<\s*GameState|Arc\s*<\s*crate::game::GameState|std::sync::Arc\s*<\s*game::GameState' crates/openmines-server/src -g '*.rs' | wc -l | tr -d ' '
printf 'sync lock guard sites: '
rg -n 'let\s+(mut\s+)?[A-Za-z_][A-Za-z0-9_]*\s*=\s*.*\.(lock|read|write)\s*\(\)\s*;' crates/openmines-server/src -g '*.rs' | wc -l | tr -d ' '

exit "$fail"
