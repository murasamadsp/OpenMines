#!/usr/bin/env python3
"""Check for unreviewed unsafe boundaries in crates/."""

from pathlib import Path
import re
import sys

allowed = {
    ("crates/openmines-server/src/cli.rs", 108): "test-only serialized env mutation",
    ("crates/openmines-server/src/cli.rs", 123): "test-only serialized env cleanup",
    ("crates/openmines-world/src/lib.rs", 182): "private mmap after file length set",
    ("crates/openmines-world/src/world/layer.rs", 73): "private mmap after file length set (durability layer)",
    ("crates/openmines-server/src/console.rs", 45): "poll initialized stdin descriptor with bounded timeout",
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
