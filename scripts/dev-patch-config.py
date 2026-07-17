#!/usr/bin/env python3
"""Patch a server config.json for local development.

Usage:
    dev-patch-config.py <src> <dst> <port> <chunks_w> <chunks_h> <log_filter>
"""

import json
import sys

src, dst, port, chunks_w, chunks_h, log_filter = sys.argv[1:]
with open(src, encoding="utf-8") as f:
    cfg = json.load(f)

cfg["world_name"] = "local-dev"
cfg["port"] = int(port)
cfg["world_chunks_w"] = int(chunks_w)
cfg["world_chunks_h"] = int(chunks_h)
cfg["data_dir"] = "data"
cfg["logging"] = {"filter": log_filter, "format": "compact", "file": None}

with open(dst, "w", encoding="utf-8") as f:
    json.dump(cfg, f, ensure_ascii=False, indent=2)
    f.write("\n")
