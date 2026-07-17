#!/usr/bin/env python3
"""Check for sync lock guards held across .await points in openmines-server."""

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
