#!/usr/bin/env bash
# Static audit for explicit Rust soundness boundaries.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> Checking explicit unsafe allowlist"
python3 - <<'PY'
from pathlib import Path
import re
import sys

allowed = {
    ("crates/openmines-server/src/cli.rs", 108): "test-only serialized env mutation",
    ("crates/openmines-server/src/cli.rs", 123): "test-only serialized env cleanup",
    ("crates/openmines-world/src/lib.rs", 167): "private mmap after file length set",
}

errors = []
for root in [Path("crates")]:
    for path in root.rglob("*.rs"):
        for lineno, line in enumerate(path.read_text(errors="ignore").splitlines(), start=1):
            if re.search(r"\bunsafe\b", line):
                key = (path.as_posix(), lineno)
                if key not in allowed:
                    errors.append(f"{path}:{lineno}: unreviewed unsafe boundary: {line.strip()}")

missing = [f"{path}:{line} ({why})" for (path, line), why in allowed.items() if not Path(path).exists()]

if missing:
    errors.extend(f"missing unsafe allowlist target: {item}" for item in missing)

if errors:
    print("\n".join(errors))
    sys.exit(1)
PY

echo "==> Checking raw pointer / UnsafeCell / PhantomData / FFI boundaries"
if rg -n 'UnsafeCell|PhantomData|NonNull|MaybeUninit|ManuallyDrop|\*mut\s|\*const\s|extern\s+"|repr\s*\(\s*packed' crates -g '*.rs'; then
  echo "ERROR: raw memory/FFI boundary found; add a reviewed abstraction and update scripts/ub-audit.sh" >&2
  exit 1
fi

echo "==> Checking hot adjacent atomics"
python3 - <<'PY'
from pathlib import Path
import re
import sys

errors = []
field_re = re.compile(r"^\s*(?:pub\s+)?[A-Za-z_][A-Za-z0-9_]*\s*:\s*(?P<ty>[^,]+),")
struct_start_re = re.compile(r"^\s*struct\s+([A-Za-z_][A-Za-z0-9_]*)\b")

for path in Path("crates/openmines-server/src").rglob("*.rs"):
    lines = path.read_text(errors="ignore").splitlines()
    in_struct = False
    struct_name = ""
    depth = 0
    previous_atomic = None

    for lineno, line in enumerate(lines, start=1):
        if not in_struct:
            match = struct_start_re.match(line)
            if match and "{" in line:
                in_struct = True
                struct_name = match.group(1)
                depth = line.count("{") - line.count("}")
                previous_atomic = None
            continue

        depth += line.count("{") - line.count("}")
        match = field_re.match(line)
        if match:
            ty = match.group("ty")
            is_atomic = "Atomic" in ty
            is_padded = "CachePadded" in ty
            if is_atomic and not is_padded and previous_atomic is not None:
                prev_line, prev_ty = previous_atomic
                errors.append(
                    f"{path}:{lineno}: adjacent unpadded atomics in {struct_name}: "
                    f"{prev_ty}@{prev_line} then {ty.strip()}"
                )
            previous_atomic = (lineno, ty.strip()) if is_atomic and not is_padded else None
        elif line.strip() and not line.strip().startswith("//"):
            previous_atomic = None

        if depth <= 0:
            in_struct = False
            struct_name = ""
            previous_atomic = None

if errors:
    print("\n".join(errors))
    sys.exit(1)
PY
