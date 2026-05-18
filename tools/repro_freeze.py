#!/usr/bin/env python3
"""Детерминированный repro-харнес фриза OpenMines.

Не коммитится (tools/ исключён из rsync). Реализует задокументированный
бинарный протокол: connect -> handshake -> auth (GUI-регистрация на 1-м
запуске, далее быстрый Regular) -> keepalive PO -> туннелирование (Xdig+Xmov)
через границы чанков. Детектит "фриз" = сервер замолкает, пока мы шлём.

Запуск (через SSH-туннель на VPS):
    ssh -fN -L 18090:127.0.0.1:8090 vps
    python3 tools/repro_freeze.py --host 127.0.0.1 --port 18090 --secs 60
"""
import argparse, hashlib, json, os, socket, struct, sys, threading, time

CREDS = os.path.join(os.path.dirname(__file__), ".repro_creds.json")


def frame(data_type: bytes, event: bytes, payload: bytes) -> bytes:
    # [i32 LE total (incl 4)] [1B type] [2B event] [payload]
    body = data_type + event + payload
    return struct.pack("<i", 4 + len(body)) + body


def ty(event4: bytes, x: int, y: int, sub: bytes, t: int = 0) -> bytes:
    # outer 'B' "TY"; inner [4B ev][u32 time][u32 x][u32 y][sub]
    inner = event4 + struct.pack("<III", t & 0xFFFFFFFF, x & 0xFFFFFFFF, y & 0xFFFFFFFF) + sub
    return frame(b"B", b"TY", inner)


class Client:
    def __init__(self, host, port):
        self.s = socket.create_connection((host, port), timeout=10)
        self.s.settimeout(0.4)
        self.buf = bytearray()
        self.last_recv = time.time()
        self.sid = None
        self.spawn = None          # (x, y) from @T
        self.ah = None             # (id, hash) from AH
        self.pi_count = 0          # PI replies received (PO->PI health)
        self.tp_rollback = 0       # small @T after spawn = move rejected (rubber-band proxy)
        self.cur = None            # believed (x,y)
        self.alive = True
        self.lock = threading.Lock()
        threading.Thread(target=self._reader, daemon=True).start()

    def _reader(self):
        while self.alive:
            try:
                chunk = self.s.recv(65536)
            except socket.timeout:
                continue
            except OSError:
                break
            if not chunk:
                break
            with self.lock:
                self.last_recv = time.time()
                self.buf += chunk
                self._parse()

    def _parse(self):
        while len(self.buf) >= 4:
            total = struct.unpack("<i", self.buf[:4])[0]
            if total < 7 or total > 1 << 20 or len(self.buf) < total:
                break
            pkt = bytes(self.buf[:total]); del self.buf[:total]
            ev = pkt[5:7]; payload = pkt[7:]
            if ev == b"PI":
                self.pi_count += 1
            if ev == b"AU" and self.sid is None:
                self.sid = payload.decode("utf-8", "replace").strip()
            elif ev == b"AH":
                txt = payload.decode("utf-8", "replace")
                pid, _, h = txt.partition("_")
                try:
                    self.ah = (int(pid), h)
                except ValueError:
                    pass
            elif ev == b"@T":
                try:
                    xs, _, ys = payload.decode().partition(":")
                    txc, tyc = int(xs), int(ys)
                except (ValueError, UnicodeDecodeError):
                    continue
                if self.spawn is None:
                    self.spawn = (txc, tyc)
                    self.cur = (txc, tyc)
                elif self.cur is not None:
                    d = abs(txc - self.cur[0]) + abs(tyc - self.cur[1])
                    # маленький @T (1..4 клетки) = сервер откатил ход =
                    # rubber-band proxy; большой = респаун (игнор).
                    if 0 < d <= 4:
                        self.tp_rollback += 1
                    self.cur = (txc, tyc)

    def send(self, b):
        try:
            self.s.sendall(b)
        except OSError:
            self.alive = False

    def wait(self, pred, timeout):
        end = time.time() + timeout
        while time.time() < end:
            with self.lock:
                if pred():
                    return True
            time.sleep(0.02)
        return False


def md5_token(h, sid):
    return hashlib.md5((h + sid).encode()).hexdigest()


def authenticate(c):
    if not c.wait(lambda: c.sid is not None, 5):
        print("FAIL: no sid handshake"); return False
    print(f"sid={c.sid}")
    creds = None
    if os.path.exists(CREDS):
        creds = json.load(open(CREDS))
    if creds:
        tok = md5_token(creds["hash"], c.sid)
        c.send(frame(b"U", b"AU", f"bot_{creds['id']}_{tok}".encode()))
        print(f"AU Regular id={creds['id']}")
    else:
        # GUI registration: AU(NoAuth fail) -> newakk -> newnick -> passwd
        c.send(frame(b"U", b"AU", b"bot_NO_x"))
        time.sleep(0.4)
        c.send(ty(b"GUI_", 0, 0, b'{"b":"newakk"}')); time.sleep(0.4)
        name = f"reprobot{int(time.time()) % 100000}"
        c.send(ty(b"GUI_", 0, 0, json.dumps({"b": f"newnick:{name}"}).encode())); time.sleep(0.4)
        c.send(ty(b"GUI_", 0, 0, b'{"b":"passwd:pw"}'))
        print(f"GUI register nick={name}")
    if not c.wait(lambda: c.spawn is not None, 8):
        print("FAIL: no @T spawn (auth/init failed)"); return False
    if c.ah:
        json.dump({"id": c.ah[0], "hash": c.ah[1]}, open(CREDS, "w"))
        print(f"saved creds id={c.ah[0]}")
    print(f"spawn={c.spawn}")
    return True


def run(host, port, secs):
    c = Client(host, port)
    if not authenticate(c):
        return 2
    x, y = c.spawn
    t0 = time.time()
    last_po = 0.0
    direction, dirvec = 3, (1, 0)   # Xmov dir 3 = +x (per server delta logic)
    moves = 0
    max_silence = 0.0
    froze_at = None
    def keepalive(n):
        nonlocal last_po
        if n - last_po > 3:
            c.send(frame(b"U", b"PO", f"0:{int(n * 1000) & 0x7FFFFFFF}".encode()))
            last_po = n

    def track(nx_, ny_):
        nonlocal x, y, moves, max_silence, froze_at
        x, y = nx_, ny_
        moves += 1
        with c.lock:
            c.cur = (x, y)
            sil = time.time() - c.last_recv
        max_silence = max(max_silence, sil)
        if sil > 3.0 and froze_at is None:
            froze_at = round(time.time() - t0, 2)
            print(f"*** FREEZE: silent {sil:.1f}s at T+{froze_at}s moves={moves} ***")

    # Phase 1: прокопать прямой коридор ~45 клеток (dig-gated ~5/с).
    print("--- phase1: tunneling corridor ---")
    corridor_x0 = x
    while time.time() - t0 < secs * 0.45 and c.alive and (x - corridor_x0) < 45:
        keepalive(time.time())
        nx, ny = x + 1, y
        c.send(ty(b"Xdig", nx, ny, b"3"))
        time.sleep(0.205)
        c.send(ty(b"Xmov", nx, ny, b"3", int(time.time() * 1000)))
        time.sleep(0.05)
        track(nx, ny)
    # Phase 2: БЫСТРО осциллировать в открытом коридоре ~12/с (темп реального
    # клиента) — это и есть сценарий rubber-band: при cooldown сервер дропал
    # ходы → @T-откат. Пост-фикс (cooldown убран) tp_rollback должен быть ~0.
    print(f"--- phase2: rapid oscillation in open corridor (~12/s, "
          f"x={corridor_x0}..{x}) ---")
    lo, hi = corridor_x0 + 1, x
    cx2, d2 = x, -1
    while time.time() - t0 < secs and c.alive:
        keepalive(time.time())
        cx2 += d2
        if cx2 <= lo:
            cx2, d2 = lo, 1
        elif cx2 >= hi:
            cx2, d2 = hi, -1
        c.send(ty(b"Xmov", cx2, y, b"3" if d2 > 0 else b"1", int(time.time() * 1000)))
        time.sleep(0.083)  # ~12/s = real client SpeedPacket pace
        track(cx2, y)
        if moves % 40 == 0:
            with c.lock:
                sil = time.time() - c.last_recv
            print(f"T+{time.time()-t0:5.1f}s moves={moves} x~{cx2} "
                  f"rollback={c.tp_rollback} silence={sil:.2f}s")
    c.alive = False
    rb = c.tp_rollback
    print(f"--- done: moves={moves} max_silence={max_silence:.2f}s "
          f"froze_at={froze_at} pi_replies={c.pi_count} "
          f"tp_rollback={rb} ({100.0 * rb / max(moves, 1):.1f}% of moves) ---")
    return 0 if froze_at is None else 1


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=18090)
    ap.add_argument("--secs", type=int, default=60)
    sys.exit(run(*vars(ap.parse_args()).values()))
