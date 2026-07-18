#!/usr/bin/env bash
# Guard: запрещает send_u_packet внутри modify_player closures.
# Такой паттерн держит ECS write lock пока строятся wire-пакеты.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/crates/server/openmines-server/src/game/logic"

violations=0
while IFS= read -r -d '' file; do
    while IFS= read -r line; do
        echo "VIOLATION: $line"
        violations=$((violations + 1))
    done < <(python3 -c "
import sys
with open('$file') as f:
    lines = f.readlines()
in_modify = 0
for i, line in enumerate(lines, 1):
    if '.modify_player(' in line:
        in_modify = 1
        start = i
    if in_modify:
        if 'send_u_packet' in line:
            print(f'$file:{i}: send_u_packet inside modify_player (started at {start})')
        if line.strip() == '});' or line.strip() == '})':
            in_modify = 0
" 2>/dev/null)
done < <(find "$SRC" -name "*.rs" -print0)

if [ "$violations" -gt 0 ]; then
    echo ""
    echo "FAIL: $violations violations found."
    echo "send_u_packet must NOT be called inside modify_player closures."
    echo "Extract values from modify_player, then build packets outside."
    exit 1
fi

echo "OK: no wire-in-lock violations"
