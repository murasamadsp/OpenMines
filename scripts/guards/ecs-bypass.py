#!/usr/bin/env python3
import sys
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
NET_DIR = ROOT / "crates/server/openmines-server/src/net"
BASELINE_PATH = ROOT / "docs/reference/ecs_bypass_baseline.txt"

# Ищем любые обращения к ECS или методам GameState, обходящим команды
PATTERNS = [
    re.compile(r"\bstate\s*\.\s*ecs\b"),
    re.compile(r"\bstate\s*\.\s*ecs_read_profiled\b"),
    re.compile(r"\bstate\s*\.\s*ecs_write_profiled\b"),
    re.compile(r"\bquery_player"),
    re.compile(r"\bmodify_player"),
]

def brace_delta(line: str) -> int:
    line = re.sub(r'"(?:\\.|[^"\\])*"', '""', line)
    line = re.sub(r"//.*", "", line)
    return line.count("{") - line.count("}")

def collect_violations():
    violations = []
    for path in NET_DIR.rglob("*.rs"):
        if path.name == "tests.rs" or "test" in path.name:
            continue
            
        content = path.read_text(errors="ignore")
        lines = content.splitlines()
        
        depth = 0
        cfg_test_next = False
        test_depth = None
        
        for lineno, line in enumerate(lines, start=1):
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
            if not in_test_mod and not stripped.startswith("//") and not stripped.startswith("/*"):
                for pattern in PATTERNS:
                    if pattern.search(line):
                        rel_path = path.relative_to(ROOT)
                        violations.append((f"{rel_path}", stripped))
                        break
                        
            depth += delta
            
    return sorted(violations)

def main():
    if len(sys.argv) < 2:
        print("Usage: ecs-bypass-guard.py [--generate | --check]")
        sys.exit(1)
        
    mode = sys.argv[1]
    
    if mode == "--generate":
        violations = collect_violations()
        BASELINE_PATH.parent.mkdir(parents=True, exist_ok=True)
        with open(BASELINE_PATH, "w") as f:
            for file_path, content in violations:
                f.write(f"{file_path} >>> {content}\n")
        print(f"Generated {len(violations)} baseline exceptions in {BASELINE_PATH.relative_to(ROOT)}")
        
    elif mode == "--check":
        if not BASELINE_PATH.exists():
            print(f"Error: Baseline file {BASELINE_PATH} does not exist. Run with --generate first.")
            sys.exit(1)
            
        with open(BASELINE_PATH) as f:
            baseline = set(line.strip() for line in f if line.strip())
            
        current = set(f"{file_path} >>> {content}" for file_path, content in collect_violations())
        
        # Находим новые нарушения (есть в current, но нет в baseline)
        new_violations = current - baseline
        if new_violations:
            print("ERROR: New direct ECS accesses detected in net/ layer which are not in baseline:")
            for v in sorted(new_violations):
                print(f"  {v}")
            print("\nPlease refactor these using the typed command pipeline instead of direct ECS locks.")
            sys.exit(1)
            
        # Находим исправленный долг (есть в baseline, но больше нет в current)
        resolved = baseline - current
        if resolved:
            print(f"Good news! {len(resolved)} ECS bypasses have been resolved. Please run python3 scripts/ecs-bypass-guard.py --generate to update the baseline.")
            
        print("ECS bypass check passed successfully.")
        
if __name__ == "__main__":
    main()
