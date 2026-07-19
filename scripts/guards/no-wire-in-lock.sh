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
import re
with open('$file') as f:
    lines = f.readlines()
depth = 0
in_modify = False
modify_start = 0
for i, line in enumerate(lines, 1):
    stripped = line.strip()
    if '.modify_player(' in line and not in_modify:
        in_modify = True
        modify_start = i
        depth = 0
    if in_modify:
        depth += line.count('{') - line.count('}')
        # Skip comments
        code_part = line.split('//')[0] if '//' in line else line
        if 'send_u_packet' in code_part:
            print(f'$file:{i}: send_u_packet inside modify_player (started at {modify_start})')
        # Closure ends when brace depth returns to 0
        if depth <= 0:
            in_modify = False
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
