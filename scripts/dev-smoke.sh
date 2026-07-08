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

python3 - "$ROOT/configs/config.json" "$WORK_DIR/configs/config.json" "$PORT" <<'PY'
import json
import sys

src, dst, port = sys.argv[1:]
with open(src, encoding="utf-8") as f:
    cfg = json.load(f)

cfg["world_name"] = "dev-smoke"
cfg["port"] = int(port)
cfg["world_chunks_w"] = 4
cfg["world_chunks_h"] = 4
cfg["data_dir"] = "data"
cfg["logging"] = {
    "filter": "openmines_server=info,openmines_server::net=debug,tokio=warn",
    "format": "compact",
    "file": None,
}

with open(dst, "w", encoding="utf-8") as f:
    json.dump(cfg, f, ensure_ascii=False, indent=2)
    f.write("\n")
PY

echo "==> Starting local smoke server"
echo "    workdir:  $WORK_DIR"
echo "    endpoint: 127.0.0.1:$PORT"
echo "    log:      $LOG_FILE"

pushd "$WORK_DIR" >/dev/null
env \
  -u M3R_PORT \
  -u M3R_DATA_DIR \
  -u M3R_REGEN_WORLD \
  -u M3R_USE_CTRL_C \
  -u M3R_ABORT_ON_PANIC \
  -u M3R_LOG \
  -u RUST_LOG \
  M3R_ADMIN_TOKEN=dev-smoke-admin \
  M3R_GRANT_ADMIN=smokeadmin \
  "$BIN" >"$LOG_FILE" 2>&1 &
SERVER_PID="$!"
popd >/dev/null

python3 - "$PORT" "$SERVER_PID" "$LOG_FILE" <<'PY'
import os
import hashlib
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


def prog_payload(prog_id, source, compiled=b""):
    source_bytes = source.encode("utf-8")
    return struct.pack("<ii", len(compiled), prog_id) + compiled + source_bytes


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


def read_until_event(event, timeout=8):
    found = []
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        sock.settimeout(max(0.1, min(0.5, end - time.monotonic())))
        try:
            packet = read_packet()
        except socket.timeout:
            continue
        found.append(packet)
        if packet[1] == event:
            return packet, found
    raise RuntimeError(
        f"missing event {event!r}; observed tail={[p[1] for p in found[-20:]]!r}"
    )


def drain_available(timeout=0.25):
    found = []
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        sock.settimeout(max(0.01, min(0.05, end - time.monotonic())))
        try:
            found.append(read_packet())
        except socket.timeout:
            continue
    return found


def assert_no_event(packets, event, label):
    observed = [p[1] for p in packets]
    if event in observed:
        raise RuntimeError(f"{label}: unexpected {event!r} in {observed!r}")


def foreground_events(packets):
    return [p[1] for p in packets if p[1] != "PI"]


def first_payload(packets, event):
    for packet in packets:
        if packet[1] == event:
            return packet[2]
    raise RuntimeError(f"missing payload for event {event!r}")


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

    nick = "smokeadmin"
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
    user_id_s, user_hash = ah_payload.split("_", 1)
    user_id = int(user_id_s)

    init_packets = wait_for_events(["BD", "GE", "@L", "BI", "sp", "@B", "P$", "LV", "IN", "@T", "#S", "mO", "mU", "#F", "@P"], timeout=12)
    tp_payload = next(p[2] for p in init_packets if p[1] == "@T").decode("utf-8", errors="strict")
    xs, _, ys = tp_payload.partition(":")
    x = int(xs)
    y = int(ys)
finally:
    if sock is not None:
        sock.close()

sock = None
try:
    sock = connect_client()
    sock.settimeout(5)
    sid = expect_handshake("grant-admin-reconnect")
    token = hashlib.md5(f"{user_hash}{sid}".encode("utf-8")).hexdigest()
    write_u("AU", f"smokere_{user_id}_{token}")
    admin_login_packets = wait_for_events(["cf", "Gu", "BD", "GE", "@L", "BI", "sp", "@B", "P$", "LV", "IN", "@T", "#S", "mO", "mU", "#F", "@P"], timeout=12)
    tp_payload = next(p[2] for p in admin_login_packets if p[1] == "@T").decode("utf-8", errors="strict")
    xs, _, ys = tp_payload.partition(":")
    x = int(xs)
    y = int(ys)

    write_u("PO", f"0:{int(time.time() * 1000) & 0x7FFFFFFF}")
    write_ty("Xdig", x, y, "3")
    time.sleep(0.25)
    write_ty("Xmov", x + 1, y, "3", int(time.time() * 1000))
    wait_for_events(["PI"], timeout=8)
    x += 1

    write_ty("Locl", x, y, "/money 10000")
    money_packets = wait_for_events(["P$"], timeout=8)
    money_payload = json.loads(first_payload(money_packets, "P$").decode("utf-8"))
    if money_payload["money"] < 10_000:
        raise RuntimeError(f"admin /money did not grant funds: {money_payload!r}")

    write_ty("Blds", x, y)
    blds_packets = wait_for_events(["GU"], timeout=8)
    blds_payload = first_payload(blds_packets, "GU").decode("utf-8", errors="replace")
    if "Мои здания" not in blds_payload:
        raise RuntimeError(f"Blds did not open owned-buildings window: {blds_payload!r}")

    write_gui("open_buildings")
    buildings_packets = wait_for_events(["GU"], timeout=8)
    buildings_payload = first_payload(buildings_packets, "GU").decode("utf-8", errors="replace")
    if "bld_place:O" not in buildings_payload:
        raise RuntimeError(f"buildings menu missing Spot placement route: {buildings_payload!r}")

    write_gui("bld_place:O")
    placed_packets = wait_for_events(["P$", "HB", "Gu"], timeout=8)
    placed_events = foreground_events(placed_packets)
    if placed_events[0] != "P$" or "HB" not in placed_events or placed_events[-1] != "Gu":
        raise RuntimeError(f"building placement events mismatch: {placed_events!r}")

    for step in range(1, 4):
        expected_event = "GU" if step == 3 else "HB"
        write_ty("Xmov", x + step, y, "3", int(time.time() * 1000))
        wait_for_events([expected_event], timeout=8)
    x += 3

    write_ty("ADMN", x, y)
    admin_packets = wait_for_events(["GU"], timeout=8)
    admin_payload = first_payload(admin_packets, "GU").decode("utf-8", errors="replace")
    if "pack_save:%R%" not in admin_payload:
        raise RuntimeError(f"pack admin GUI missing save route: {admin_payload!r}")

    write_ty("Pope", x, y)
    wait_for_events(["GU"], timeout=8)
    write_gui("createprog:main")
    open_packet, create_packets = read_until_event("#P", timeout=8)
    create_events = [p[1] for p in create_packets]
    if create_events[:2] != ["Gu", "#P"]:
        raise RuntimeError(f"programmator create events mismatch: {create_events!r}")
    opened = json.loads(open_packet[2].decode("utf-8"))
    prog_id = int(opened["id"])
    if prog_id <= 0 or opened["title"] != "main" or opened["source"] != "":
        raise RuntimeError(f"invalid #P payload after create: {opened!r}")
    create_tail = drain_available()
    tail_events = foreground_events(create_tail)
    if tail_events != ["Gu"]:
        raise RuntimeError(f"programmator create tail mismatch: {tail_events!r}")

    write_ty("PREN", x, y, str(prog_id))
    pren_packets = wait_for_events(["GU"], timeout=8)
    pren_payload = first_payload(pren_packets, "GU").decode("utf-8", errors="replace")
    if f"rename:{prog_id}:%I%" not in pren_payload:
        raise RuntimeError(f"programmator rename dialog missing action: {pren_payload!r}")
    write_gui(f"rename:{prog_id}:renamed")
    rename_update, rename_packets = read_until_event("#p", timeout=8)
    rename_packets.extend(drain_available())
    rename_events = foreground_events(rename_packets)
    if rename_events[:2] != ["#p", "Gu"]:
        raise RuntimeError(f"programmator rename events mismatch: {rename_events!r}")
    renamed = json.loads(rename_update[2].decode("utf-8"))
    if int(renamed["id"]) != prog_id or renamed["title"] != "renamed":
        raise RuntimeError(f"invalid #p payload after rename: {renamed!r}")

    write_ty("PCOP", x, y, str(prog_id))
    copy_packets = wait_for_events(["GU"], timeout=8)
    copy_payload = first_payload(copy_packets, "GU").decode("utf-8", errors="replace")
    if "renamed (copy)" not in copy_payload:
        raise RuntimeError(f"programmator copy did not refresh list with copied program: {copy_payload!r}")

    write_gui("createprog:trash")
    trash_open_packet, trash_packets = read_until_event("#P", timeout=8)
    trash_events = foreground_events(trash_packets)
    if trash_events[:2] != ["Gu", "#P"]:
        raise RuntimeError(f"programmator trash create events mismatch: {trash_events!r}")
    trash = json.loads(trash_open_packet[2].decode("utf-8"))
    trash_id = int(trash["id"])
    if trash_id <= 0 or trash["title"] != "trash":
        raise RuntimeError(f"invalid trash #P payload: {trash!r}")
    drain_available()

    write_ty("PDEL", x, y, str(trash_id))
    delete_tail = drain_available()
    delete_events = foreground_events(delete_tail)
    if delete_events:
        raise RuntimeError(f"programmator delete must stay wire-silent, got {delete_events!r}")

    write_ty("Pope", x, y)
    wait_for_events(["GU"], timeout=8)
    write_gui(f"openprog:{prog_id}")
    reopen_packet, reopen_packets = read_until_event("#P", timeout=8)
    reopen_packets.extend(drain_available())
    reopen_events = foreground_events(reopen_packets)
    if reopen_events[:2] != ["Gu", "#P"]:
        raise RuntimeError(f"programmator open events mismatch: {reopen_events!r}")
    reopened = json.loads(reopen_packet[2].decode("utf-8"))
    if int(reopened["id"]) != prog_id or reopened["title"] != "renamed":
        raise RuntimeError(f"invalid #P payload after openprog: {reopened!r}")

    write_ty("TAGR", x, y)
    aggr_packets = wait_for_events(["BA"], timeout=8)
    if first_payload(aggr_packets, "BA") != b"1":
        raise RuntimeError("TAGR did not enable aggression")

    write_ty("Sett", x, y)
    wait_for_events(["GU"], timeout=8)
    write_gui("save:isca:0#mous:0#")
    settings_packets = wait_for_events(["#S", "GU"], timeout=8)
    settings_payload = first_payload(settings_packets, "#S").decode("utf-8", errors="replace")
    if "#isca#0" not in settings_payload or "#mous#0" not in settings_payload:
        raise RuntimeError(f"settings save did not update #S payload: {settings_payload!r}")

    write_ty("PROG", x, y, prog_payload(prog_id, "$z"))
    prog_packets = wait_for_events(["Gu", "@T", "#p", "@P", "BH"], timeout=8)
    prog_packets.extend(drain_available())
    assert_no_event(prog_packets, "#P", "PROG start")
    prog_events = foreground_events(prog_packets)
    expected_prog = ["Gu", "@T", "#p", "@P", "BH"]
    if prog_events[:5] != expected_prog:
        raise RuntimeError(f"PROG start events mismatch: {prog_events!r}")
    prog_update = json.loads(first_payload(prog_packets, "#p").decode("utf-8"))
    if prog_update.get("id") != prog_id or prog_update.get("source") != "$z":
        raise RuntimeError(f"PROG start #p payload mismatch: {prog_update!r}")
    prog_foreground_packets = [p for p in prog_packets if p[1] != "PI"]
    if prog_foreground_packets[3][2] != b"1" or prog_foreground_packets[4][2] != b"0":
        raise RuntimeError("PROG start did not report @P=1 and BH=0")

    write_ty("pRST", x, y)
    stop_packets = wait_for_events(["Gu", "@P", "BH"], timeout=8)
    stop_foreground_packets = [p for p in stop_packets if p[1] != "PI"]
    if stop_foreground_packets[1][2] != b"0" or stop_foreground_packets[2][2] != b"0":
        raise RuntimeError("pRST stop did not report @P=0 and BH=0")
finally:
    if sock is not None:
        sock.close()

sock = None
try:
    sock = connect_client()
    sock.settimeout(5)
    sid = expect_handshake("reconnect")
    token = hashlib.md5(f"{user_hash}{sid}".encode("utf-8")).hexdigest()
    write_u("AU", f"smokere_{user_id}_{token}")
    reconnect_packets = wait_for_events(["cf", "Gu", "BD", "GE", "@L", "BI", "sp", "@B", "P$", "LV", "IN", "@T", "#S", "mO", "mU", "#F", "@P", "BH", "#p"], timeout=12)
    reconnect_events = foreground_events(reconnect_packets)
    assert_no_event(reconnect_packets, "#P", "reconnect init")
    reconnect_update = next(p for p in reconnect_packets if p[1] == "#p")
    reconnect_prog = json.loads(reconnect_update[2].decode("utf-8"))
    if int(reconnect_prog["id"]) != prog_id or reconnect_prog["title"] != "renamed" or reconnect_prog["source"] != "$z":
        raise RuntimeError(f"selected program was not restored on reconnect: {reconnect_prog!r}")
    if first_payload(reconnect_packets, "@P") != b"0" or first_payload(reconnect_packets, "BH") != b"0":
        raise RuntimeError(f"reconnect programmator status mismatch: {reconnect_events!r}")
finally:
    if sock is not None:
        sock.close()

print("OK: local TCP smoke passed")
print("    initial: ST AU PI")
print("    auth-failure: cf BI HB GU")
print("    gui-register: AH cf Gu + init packets")
print("    gameplay: PO/Xdig/Xmov kept session responsive")
print("    building/admin: Blds/open_buildings/bld_place/ADMN HORB wire contract")
print("    programmator: Pope/create/open/rename/copy/delete/PROG/pRST wire contract")
print("    settings: TAGR and settings save wire contract")
print("    reconnect: selected program restored without #P editor open")
PY

echo "==> Stopping smoke server"
kill -TERM "$SERVER_PID"
wait "$SERVER_PID" || true
SERVER_PID=""
