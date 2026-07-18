#!/usr/bin/env bash
# Guard: запрещает вложенные блокировки и wire-операции внутри modify_player closures.
# CRITICAL (exit 1): send_u_packet, ecs_write/read_profiled, query_player, modify_building
# WARNING (exit 0): format!
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/crates/server/openmines-server/src/game"

critical=0
warnings=0
while IFS= read -r -d '' file; do
    while IFS= read -r line; do
        if echo "$line" | grep -q "format!"; then
            echo "WARNING: $line"
            warnings=$((warnings + 1))
        else
            echo "CRITICAL: $line"
            critical=$((critical + 1))
        fi
    done < <(python3 -c "
import re, sys
with open('$file') as f:
    lines = f.readlines()

i = 0
while i < len(lines):
    line = lines[i]
    if '.modify_player(' in line and '|' in line:
        start = i + 1
        depth = 0
        in_closure = False
        j = i
        while j < min(i + 500, len(lines)):  # safety: max 500 lines
            cl = lines[j]
            for ch in cl:
                if ch == '{':
                    depth += 1
                    in_closure = True
                elif ch == '}':
                    depth -= 1
            if in_closure and depth > 0 and j > i:
                cs = cl.strip()
                if cs.startswith('//'):
                    j += 1
                    continue
                if 'send_u_packet' in cs:
                    print(f'$file:{j+1}: send_u_packet inside modify_player (started at {start})')
                elif 'ecs_write_profiled' in cs or 'ecs_read_profiled' in cs:
                    print(f'$file:{j+1}: ECS lock inside modify_player (started at {start})')
                elif '.query_player(' in cs:
                    print(f'$file:{j+1}: query_player inside modify_player (started at {start})')
                elif '.modify_building(' in cs:
                    print(f'$file:{j+1}: modify_building inside modify_player (started at {start})')
                elif 'format!' in cs:
                    print(f'$file:{j+1}: format! inside modify_player (started at {start})')
            if in_closure and depth <= 0:
                break
            j += 1
        i = j + 1
    else:
        i += 1
" 2>/dev/null)
done < <(find "$SRC" -name "*.rs" -print0)

echo ""
echo "Summary: $critical critical, $warnings warnings"
[ "$critical" -eq 0 ] && echo "OK: no critical violations" && exit 0
echo "FAIL: $critical critical violations"
exit 1
