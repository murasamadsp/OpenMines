import socket
import struct
import threading
import time

def frame(dt: bytes, ev: bytes, pl: bytes) -> bytes:
    body = dt + ev + pl
    return struct.pack("<i", 4 + len(body)) + body

def ty(ev4: bytes, x: int, y: int, sub: bytes, t: int = 0) -> bytes:
    inner = ev4 + struct.pack("<III", t & 0xFFFFFFFF, x & 0xFFFFFFFF, y & 0xFFFFFFFF) + sub
    return frame(b"B", b"TY", inner)

class OpenMinesClient:
    def __init__(self, host, port, timeout=10.0, read_timeout=0.4, auto_connect=True):
        self.host = host
        self.port = port
        self.timeout = timeout
        self.read_timeout = read_timeout
        self.s = None
        self.buf = bytearray()
        self.sid = None
        self.spawn = None
        self.ah = None
        self.alive = True
        self.log = []      # (t_rel, ev, pl)
        self.mus = []      # raw mU payloads
        self.evlog = []    # (t_rel, ev_str, len, pl_head)
        self.pi = 0
        self.cur = None
        self.respawns = 0
        self.last_recv = time.time()
        self.t0 = time.time()
        self.lock = threading.Lock()
        if auto_connect:
            self.connect()

    def connect(self):
        self.s = socket.create_connection((self.host, self.port), timeout=self.timeout)
        self.s.settimeout(self.read_timeout)
        self.t0 = time.time()
        threading.Thread(target=self._rd, daemon=True).start()

    def _rd(self):
        while self.alive:
            try:
                ch = self.s.recv(65536)
            except socket.timeout:
                continue
            except OSError:
                break
            if not ch:
                break
            with self.lock:
                self.last_recv = time.time()
                self.buf += ch
                self._parse()

    def _parse(self):
        while len(self.buf) >= 4:
            total = struct.unpack("<i", self.buf[:4])[0]
            if total < 7 or total > 1 << 20 or len(self.buf) < total:
                break
            pkt = bytes(self.buf[:total]); del self.buf[:total]
            ev = pkt[5:7]; pl = pkt[7:]
            tr = round(time.time() - self.t0, 3)
            
            self.evlog.append((tr, ev.decode("ascii", "replace"), len(pl), pl[:80]))
            
            if ev == b"PI":
                self.pi += 1
            elif ev == b"AU" and self.sid is None:
                self.sid = pl.decode("utf-8", "replace").strip()
            elif ev == b"AH":
                txt = pl.decode("utf-8", "replace"); pid, _, h = txt.partition("_")
                try: self.ah = (int(pid), h)
                except ValueError: pass
            elif ev == b"@T":
                try:
                    xs, _, ys = pl.decode().partition(":")
                    tx, ty = int(xs), int(ys)
                    if self.spawn is None:
                        self.spawn = (tx, ty)
                        self.cur = (tx, ty)
                    elif self.cur is not None:
                        if abs(tx - self.cur[0]) + abs(ty - self.cur[1]) > 40:
                            self.respawns += 1
                        self.cur = (tx, ty)
                except (ValueError, UnicodeDecodeError):
                    pass
            elif ev == b"mU":
                self.mus.append(pl)
                
            self.log.append((tr, ev, pl))

    def send(self, b):
        try:
            self.s.sendall(b)
        except OSError:
            self.alive = False

    def wait(self, pred, to):
        end = time.time() + to
        while time.time() < end:
            with self.lock:
                if pred(): return True
            time.sleep(0.02)
        return False

    def drain_mu(self):
        with self.lock:
            r = list(self.mus); self.mus.clear()
            return r

    def close(self):
        self.alive = False
        try:
            if self.s:
                self.s.close()
        except OSError:
            pass
