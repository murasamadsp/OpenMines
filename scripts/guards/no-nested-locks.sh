#!/usr/bin/env bash
# Guard: запрещает вложенные блокировки и wire-операции внутри modify_player closures.
#
# CRITICAL (exit 1):
# - send_u_packet: держит lock пока строит пакеты
# - ecs_write_profiled/ecs_read_profiled: двойная блокировка
# - query_player: вложенный read lock внутри write lock
# - modify_building: вложенный write lock
#
# WARNING (exit 0, но выводит):
# - format!: может быть тяжёлым, лучше вынести
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
import re
with open('$file') as f:
    lines = f.readlines()
in_modify = 0
for i, line in enumerate(lines, 1):
    if '.modify_player(' in line:
        in_modify = 1
        start = i
    if in_modify:
        stripped = line.strip()
        # Critical: wire operations
        if 'send_u_packet' in stripped:
            print(f'$file:{i}: send_u_packet inside modify_player (started at {start})')
        # Critical: nested ECS locks
        elif 'ecs_write_profiled' in stripped or 'ecs_read_profiled' in stripped:
            print(f'$file:{i}: ECS lock inside modify_player (started at {start})')
        # Critical: nested query/modify
        elif '.query_player(' in stripped:
            print(f'$file:{i}: query_player inside modify_player (started at {start})')
        elif '.modify_building(' in stripped:
            print(f'$file:{i}: modify_building inside modify_player (started at {start})')
        # Warning: heavy operations
        elif 'format!' in stripped and not stripped.startswith('//'):
            print(f'$file:{i}: format! inside modify_player (started at {start}) - consider extracting')
        # End of closure
        if re.match(r'.*\}\)\s*(\.\w+\(.*?\))*\s*;', stripped):
            in_modify = 0
" 2>/dev/null)
done < <(find "$SRC" -name "*.rs" -print0)

echo ""
echo "Summary: $critical critical, $warnings warnings"

if [ "$critical" -gt 0 ]; then
    echo "FAIL: $critical critical violations found."
    echo "modify_player closures must be fast and lock-free."
    echo "Extract values from modify_player, then do I/O outside."
    exit 1
fi

if [ "$warnings" -gt 0 ]; then
    echo "WARN: $warnings warnings (format! inside modify_player)."
    echo "Consider extracting string formatting outside the closure."
fi

echo "OK: no critical nested lock violations"
