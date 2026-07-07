#!/usr/bin/env bash
# Local wire-level smoke test. It starts a server from an explicit temporary
# local config and verifies the TCP handshake without Unity or VPS.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/debug/openmines-server"
RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/openmines-dev-smoke.XXXXXX")"
WORK_DIR="$RUN_DIR/work"
LOG_FILE="$RUN_DIR/server.log"
SERVER_PID=""
KEEP_RUN_DIR=0

cleanup() {
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill -TERM "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  if [[ "$KEEP_RUN_DIR" -eq 0 ]]; then
    rm -rf "$RUN_DIR"
  fi
}
trap cleanup EXIT

choose_port() {
  python3 - <<'PY'
import socket

with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
    s.bind(("127.0.0.1", 0))
    print(s.getsockname()[1])
PY
}

PORT="$(choose_port)"

echo "==> Building openmines-server"
cargo build --quiet --bin openmines-server

mkdir -p "$WORK_DIR/configs"
ln -s "$ROOT/configs/cells.json" "$WORK_DIR/configs/cells.json"
ln -s "$ROOT/configs/buildings.json" "$WORK_DIR/configs/buildings.json"

cat > "$WORK_DIR/configs/config.json" <<JSON
{
  "world_name": "dev-smoke",
  "port": $PORT,
  "world_chunks_w": 4,
  "world_chunks_h": 4,
  "data_dir": "data",
  "logging": {
    "filter": "openmines_server=info,openmines_server::net=debug,tokio=warn",
    "format": "compact",
    "file": null
  },
  "cron": {
    "hourly_log_enabled": true
  },
  "gameplay": {
    "cooldowns": {
      "dig_ms": 200,
      "build_ms": 200,
      "geo_ms": 200
    },
    "combat": {
      "gun_fire_interval_ms": 500,
      "gun_damage": 60,
      "gun_radius_cells": 20
    },
    "bonus": {
      "cooldown_secs": 25200,
      "reward_money": 1000000
    },
    "skills": {
      "upgrade_cost_base": 100
    },
    "spawn": {
      "x": 10,
      "y": 10
    },
    "programmator": {
      "direct_action_delay_us": 333333,
      "blocked_move_penalty_ms": 200,
      "min_move_delay_ms": 20
    },
    "schedules": {
      "hazards_ms": 10,
      "physics_ms": 100,
      "guns_ms": 100,
      "programmator_ms": 100,
      "alive_ms": 5000,
      "building_effects_ms": 1000,
      "hourly_damage_ms": 3600000,
      "game_loop_tick_rate_ms": 10,
      "game_loop_panic_backoff_ms": 200,
      "session_disconnect_timeout_secs": 30
    },
    "rate_limits": {
      "chat_burst": 5,
      "chat_per_sec": 3,
      "gui_burst": 10,
      "gui_per_sec": 5
    }
  }
}
JSON

echo "==> Starting local smoke server"
echo "    workdir:  $WORK_DIR"
echo "    endpoint: 127.0.0.1:$PORT"
echo "    log:      $LOG_FILE"

pushd "$WORK_DIR" >/dev/null
env \
  -u M3R_PORT \
  -u M3R_DATA_DIR \
  -u M3R_REGEN_WORLD \
  -u M3R_GRANT_ADMIN \
  -u M3R_USE_CTRL_C \
  -u M3R_ABORT_ON_PANIC \
  -u M3R_LOG \
  -u RUST_LOG \
  "$BIN" >"$LOG_FILE" 2>&1 &
SERVER_PID="$!"
popd >/dev/null

python3 - "$PORT" "$SERVER_PID" "$LOG_FILE" <<'PY'
import os
import json
import socket
import struct
import sys
import time

port = int(sys.argv[1])
pid = int(sys.argv[2])
log_file = sys.argv[3]
deadline = time.monotonic() + 120


def die(message):
    print(f"ERROR: {message}", file=sys.stderr)
    try:
        with open(log_file, "r", encoding="utf-8", errors="replace") as f:
            lines = f.readlines()[-120:]
    except OSError as exc:
        print(f"ERROR: could not read server log: {exc}", file=sys.stderr)
    else:
        print("\n--- server log tail ---", file=sys.stderr)
        print("".join(lines), file=sys.stderr, end="")
        print("--- end server log ---", file=sys.stderr)
    sys.exit(1)


def process_alive():
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


sock = None
while time.monotonic() < deadline:
    if not process_alive():
        die("server process exited before TCP bind")
    try:
        sock = socket.create_connection(("127.0.0.1", port), timeout=0.5)
        break
    except OSError:
        time.sleep(0.25)

if sock is None:
    die("server did not accept TCP connections within 120s")


def recv_exact(n):
    out = bytearray()
    while len(out) < n:
        chunk = sock.recv(n - len(out))
        if not chunk:
            raise RuntimeError("connection closed while reading packet")
        out.extend(chunk)
    return bytes(out)


def read_packet():
    raw_len = recv_exact(4)
    (length,) = struct.unpack("<i", raw_len)
    if length < 7 or length > 65536:
        raise RuntimeError(f"invalid packet length: {length}")
    body = recv_exact(length - 4)
    data_type = chr(body[0])
    event = body[1:3].decode("ascii", errors="replace")
    payload = body[3:]
    return data_type, event, payload


def write_u(event, payload):
    payload_bytes = payload.encode("utf-8")
    body = b"U" + event.encode("ascii") + payload_bytes
    sock.sendall(struct.pack("<i", len(body) + 4) + body)


def write_ty(event4, x, y, sub_payload=b"", packet_time=0):
    if isinstance(sub_payload, str):
        sub_payload = sub_payload.encode("utf-8")
    inner = (
        event4.encode("ascii")
        + struct.pack("<III", packet_time & 0xFFFFFFFF, x & 0xFFFFFFFF, y & 0xFFFFFFFF)
        + sub_payload
    )
    body = b"B" + b"TY" + inner
    sock.sendall(struct.pack("<i", len(body) + 4) + body)


def write_gui(button):
    write_ty("GUI_", 0, 0, json.dumps({"b": button}, separators=(",", ":")).encode("utf-8"))


def connect_client():
    client = socket.create_connection(("127.0.0.1", port), timeout=5)
    client.settimeout(5)
    return client


def expect_handshake(label):
    initial = [read_packet() for _ in range(3)]
    initial_events = [p[1] for p in initial]
    if initial_events != ["ST", "AU", "PI"]:
        raise RuntimeError(f"{label}: initial events mismatch: {initial_events!r}")
    sid = initial[1][2].decode("utf-8", errors="strict")
    if len(sid) != 5:
        raise RuntimeError(f"{label}: session id length mismatch: {sid!r}")
    return sid


def wait_for_events(wanted, timeout=8):
    found = []
    end = time.monotonic() + timeout
    while time.monotonic() < end and wanted:
        sock.settimeout(max(0.1, min(0.5, end - time.monotonic())))
        try:
            packet = read_packet()
        except socket.timeout:
            continue
        found.append(packet)
        if packet[1] == wanted[0]:
            wanted.pop(0)
    if wanted:
        raise RuntimeError(
            f"missing events {wanted!r}; observed tail={[p[1] for p in found[-20:]]!r}"
        )
    return found


try:
    sock.settimeout(5)
    expect_handshake("auth-failure")

    time.sleep(1.5)
    write_u("AU", "smoke_NO_AUTH")

    expected = ["cf", "BI", "HB", "GU"]
    observed = [read_packet()[1] for _ in expected]
    if observed != expected:
        raise RuntimeError(f"auth-failure events mismatch: {observed!r}")
finally:
    sock.close()

sock = None
try:
    sock = connect_client()
    sock.settimeout(5)
    expect_handshake("gui-register")

    nick = f"smoke{int(time.time() * 1000) % 1_000_000}"
    write_u("AU", "smokereg_NO_x")
    wait_for_events(["cf", "BI", "HB", "GU"], timeout=8)

    write_gui("newakk")
    wait_for_events(["GU"], timeout=4)
    write_gui(f"newnick:{nick}")
    wait_for_events(["GU"], timeout=4)
    write_gui("passwd:pw")

    auth_packets = wait_for_events(["AH", "cf", "Gu"], timeout=8)
    ah_payload = auth_packets[0][2].decode("utf-8", errors="strict")
    if "_" not in ah_payload:
        raise RuntimeError(f"invalid AH payload after registration: {ah_payload!r}")

    init_packets = wait_for_events(["BD", "GE", "@L", "BI", "sp", "@B", "P$", "LV", "IN", "@T", "#S", "mO", "mU", "#F", "@P"], timeout=12)
    tp_payload = next(p[2] for p in init_packets if p[1] == "@T").decode("utf-8", errors="strict")
    xs, _, ys = tp_payload.partition(":")
    x = int(xs)
    y = int(ys)

    write_u("PO", f"0:{int(time.time() * 1000) & 0x7FFFFFFF}")
    write_ty("Xdig", x, y, "3")
    time.sleep(0.25)
    write_ty("Xmov", x + 1, y, "3", int(time.time() * 1000))
    wait_for_events(["PI"], timeout=8)
finally:
    if sock is not None:
        sock.close()

print("OK: local TCP smoke passed")
print("    initial: ST AU PI")
print("    auth-failure: cf BI HB GU")
print("    gui-register: AH cf Gu + init packets")
print("    gameplay: PO/Xdig/Xmov kept session responsive")
PY

echo "==> Stopping smoke server"
kill -TERM "$SERVER_PID"
wait "$SERVER_PID" || true
SERVER_PID=""
