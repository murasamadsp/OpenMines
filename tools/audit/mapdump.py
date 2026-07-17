#!/usr/bin/env python3
"""Декодер `_v2.map` (формат MapModel.SaveMapV2) — дамп региона клеток.

Читает персистентный файл карты напрямую (без сервера/генерации) и печатает
сетку байт-значений клеток вокруг (X,Y) + гистограмму `значение → имя × count`
с флагами хазардов из cells.json. Для отладки порчи карты — сверять реальные
байты, а не визуал клиента.

Формат файла (LE):
  [0..4]  width  i32
  [4..8]  height i32
  [8..]   index-таблица: blocks_w*blocks_h × i32 (blockId -> slot, -1 = пусто)
  далее   slots × 1024 байт блоков (slot s по смещению idx_end + s*1024)
  blockId = (x>>5) + (y>>5)*blocks_w ; индекс клетки в блоке = (x&31) + 32*(y&31)

Использование:
  python3 tools/mapdump.py <map_file> -x 100 -y 100 [-r 12] [--cells configs/cells.json]
"""
import argparse
import json
import struct
import sys
from collections import Counter

BLOCK = 32
BLOCK_CELLS = BLOCK * BLOCK
HEADER = 8


class MapStore:
    def __init__(self, path):
        with open(path, "rb") as f:
            data = f.read()
        if len(data) < HEADER:
            sys.exit(f"file shorter than header: {len(data)} bytes")
        self.width, self.height = struct.unpack_from("<ii", data, 0)
        self.blocks_w = self.width // BLOCK
        self.blocks_h = self.height // BLOCK
        grid = self.blocks_w * self.blocks_h
        idx_end = HEADER + grid * 4
        if len(data) < idx_end:
            sys.exit(f"truncated index table: have {len(data)}, need {idx_end}")
        self.indexes = list(struct.unpack_from(f"<{grid}i", data, HEADER))
        self.data = data
        self.idx_end = idx_end

    def get_cell(self, x, y):
        if x < 0 or y < 0 or x >= self.width or y >= self.height:
            return None
        block_id = (x >> 5) + (y >> 5) * self.blocks_w
        slot = self.indexes[block_id]
        if slot < 0:
            return 0  # неаллоцированный блок = пусто (MapModel.GetCell)
        off = self.idx_end + slot * BLOCK_CELLS + ((x & 31) + BLOCK * (y & 31))
        return self.data[off]


def load_cell_names(path):
    if not path:
        return {}
    try:
        cells = json.load(open(path, encoding="utf-8"))
    except OSError:
        return {}
    out = {}
    for c in cells:
        t = c.get("type")
        if t is None:
            continue
        flags = []
        if c.get("damage"):
            flags.append(f"dmg={c['damage']}")
        if c.get("fall_damage"):
            flags.append(f"fall={c['fall_damage']}")
        if c.get("isSand"):
            flags.append("sand")
        if c.get("isBoulder"):
            flags.append("boulder")
        if c.get("isEmpty"):
            flags.append("empty")
        out[t] = (c.get("name") or "?", " ".join(flags))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("map_file")
    ap.add_argument("-x", type=int, required=True)
    ap.add_argument("-y", type=int, required=True)
    ap.add_argument("-r", "--radius", type=int, default=12)
    ap.add_argument("--cells", default="configs/cells.json")
    a = ap.parse_args()

    m = MapStore(a.map_file)
    names = load_cell_names(a.cells)
    r = a.radius
    print(f"── {a.map_file}: {m.width}x{m.height}, centre ({a.x},{a.y}) r{r} "
          f"block ({a.x>>5},{a.y>>5}) ──")
    print("      " + "".join(f"{a.x+dx:>4}" for dx in range(-r, r + 1)))
    hist = Counter()
    for dy in range(-r, r + 1):
        row = []
        for dx in range(-r, r + 1):
            c = m.get_cell(a.x + dx, a.y + dy)
            if c is None:
                row.append("   .")
            else:
                hist[c] += 1
                row.append(f"{c:>4}")
        print(f"{a.y+dy:>5}:" + "".join(row))

    print("── histogram (value: name [flags] × count) ──")
    for val, count in sorted(hist.items()):
        name, flags = names.get(val, ("?", ""))
        tag = f" [{flags}]" if flags else ""
        print(f"  {val:>3}: {name}{tag} × {count}")


if __name__ == "__main__":
    main()
