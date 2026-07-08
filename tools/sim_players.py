#!/usr/bin/env python3
"""Мульти-игрок симулятор нагрузки OpenMines.

Не коммитится (tools/ исключён из rsync). N конкурентных ботов: auth
(GUI-регистрация уникальных ников на 1-м запуске → кеш creds → далее
быстрый Regular), keepalive PO, рандом-туннелирование (Xdig+Xmov),
агрегAT статистика, детект фриза и различение глобальный↔per-session.

  ssh -fN -L 18090:127.0.0.1:8090 vps
  python3 tools/sim_players.py --host 127.0.0.1 --port 18090 --players 8 --secs 90
"""
import argparse, hashlib, json, os, random, sys, threading, time
sys.path.insert(0, os.path.dirname(__file__))
from om_net import OpenMinesClient, frame, ty

CREDS = os.path.join(os.path.dirname(__file__), ".sim_creds.json")
_creds_lock = threading.Lock()


def md5_token(h, sid):
    return hashlib.md5((h + sid).encode()).hexdigest()


def load_creds():
    if os.path.exists(CREDS):
        try:
            return json.load(open(CREDS))
        except (json.JSONDecodeError, OSError):
            return {}
    return {}


def save_cred(nick, pid, h):
    with _creds_lock:
        d = load_creds()
        d[nick] = {"id": pid, "hash": h}
        json.dump(d, open(CREDS, "w"))


class Bot(OpenMinesClient):
    def __init__(self, idx, host, port):
        super().__init__(host, port, auto_connect=False)
        self.idx = idx
        self.nick = f"simbot{idx}"
        self.moves = 0
        self.max_silence = 0.0
        self.froze_at = None
        self.err = None

    def register(self):
        """GUI-регистрация (1 NoAuth-failure на IP — учитывать rate-limit
        AUTH_FAILURE_LIMIT=6 / 30s). Сохраняет creds и закрывает соединение."""
        try:
            self.connect()
        except OSError:
            self.err = "no sid (register)"
            return False
        if not self.wait(lambda: self.sid is not None, 6):
            self.err = "no sid (register)"
            return False
        self.send(frame(b"U", b"AU", b"b_NO_x"))
        time.sleep(0.5)
        self.send(ty(b"GUI_", 0, 0, b'{"b":"newakk"}'))
        time.sleep(0.5)
        self.send(ty(b"GUI_", 0, 0,
                     json.dumps({"b": f"newnick:{self.nick}"}).encode()))
        time.sleep(0.5)
        self.send(ty(b"GUI_", 0, 0, b'{"b":"passwd:pw"}'))
        ok = self.wait(lambda: self.ah is not None, 10)
        if ok:
            save_cred(self.nick, self.ah[0], self.ah[1])
        self.close()
        return ok

    def connect_auth(self):
        """Только Regular-auth по кешу creds (0 failures → 0 rate-limit)."""
        cred = load_creds().get(self.nick)
        if not cred:
            self.err = "no cached creds (register first)"
            return False
        try:
            self.connect()
        except OSError:
            self.err = "no sid"
            return False
        if not self.wait(lambda: self.sid is not None, 6):
            self.err = "no sid"
            return False
        self.send(frame(b"U", b"AU",
                        f"b{self.idx}_{cred['id']}_{md5_token(cred['hash'], self.sid)}".encode()))
        if not self.wait(lambda: self.spawn is not None, 10):
            self.err = "no spawn (regular auth failed)"
            return False
        return True

    def run(self, secs, t0):
        if not self.connect_auth():
            self.alive = False
            return
        x, y = self.spawn
        last_po = 0.0
        DIRS = {0: (0, -1), 1: (-1, 0), 2: (0, 1), 3: (1, 0)}
        d = 3
        nextturn = time.time() + random.uniform(2, 6)
        while time.time() - t0 < secs and self.alive:
            now = time.time()
            if now - last_po > 3:
                try:
                    self.send(frame(b"U", b"PO",
                                    f"0:{int(now*1000)&0x7FFFFFFF}".encode()))
                except OSError:
                    break
                last_po = now
            if now > nextturn:
                # биас вниз (dir 2 = +y) — глубже = опаснее (hazard/acid) →
                # органические смерти → боксы → проверка C-1/C-2 под нагрузкой
                d = random.choice([2, 2, 2, 0, 1, 3])
                nextturn = now + random.uniform(2, 6)
            dx, dy = DIRS[d]
            nx, ny = x + dx, y + dy
            try:
                self.send(ty(b"Xdig", nx, ny, str(d).encode()))
                time.sleep(0.26)
                self.send(ty(b"Xmov", nx, ny, str(d).encode(), int(now * 1000)))
            except OSError:
                break
            time.sleep(0.12)
            x, y = nx, ny
            with self.lock:
                self.cur = (x, y)
            self.moves += 1
            with self.lock:
                sil = time.time() - self.last_recv
            self.max_silence = max(self.max_silence, sil)
            if sil > 3.0 and self.froze_at is None:
                self.froze_at = round(now - t0, 2)
        self.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=18090)
    ap.add_argument("--players", type=int, default=8)
    ap.add_argument("--secs", type=int, default=90)
    a = ap.parse_args()

    bots = [Bot(i, a.host, a.port) for i in range(a.players)]

    # --- Warmup: регистрация недостающих аккаунтов ПОСЛЕДОВАТЕЛЬНО ---
    # rate-limit: AUTH_FAILURE_LIMIT=6 / 30s по IP; все боты с одного
    # 127.0.0.1 (ssh-туннель). 1 NoAuth-failure на регистрацию → не более
    # 5 регистраций за 30s, потом пауза окна.
    have = load_creds()
    todo = [b for b in bots if b.nick not in have]
    if todo:
        print(f"warmup: registering {len(todo)} new bots (rate-limited)...")
        for n, b in enumerate(todo):
            if n > 0 and n % 5 == 0:
                print(f"  ...{n} registered, sleeping 31s (auth window reset)")
                time.sleep(31)
            okr = Bot(b.idx, a.host, a.port).register()
            print(f"  reg {b.nick}: {'OK' if okr else 'FAIL'}")
            time.sleep(1.5)

    # --- Load: все боты Regular-auth конкурентно (0 failures → 0 rate-limit) ---
    t0 = time.time()
    threads = []
    for b in bots:
        t = threading.Thread(target=b.run, args=(a.secs, t0), daemon=True)
        t.start()
        threads.append(t)
        time.sleep(0.15)  # лёгкий стаггер сокетов (failures нет)
    for t in threads:
        t.join(a.secs + 30)

    ok = [b for b in bots if b.err is None]
    froze = [b for b in bots if b.froze_at is not None]
    print(f"\n=== SIM SUMMARY: {a.players} bots, {a.secs}s ===")
    for b in bots:
        st = b.err or (f"froze@{b.froze_at}s" if b.froze_at else "ok")
        print(f"  bot{b.idx}: {st} moves={b.moves} "
              f"max_silence={b.max_silence:.2f}s pi={b.pi} respawns={b.respawns}")
    total_resp = sum(b.respawns for b in bots)
    print(f"connected={len(ok)}/{a.players} froze={len(froze)} "
          f"total_moves={sum(b.moves for b in bots)} total_respawns={total_resp} "
          f"(respawns>0 → death/box paths C-1/C-2 exercised)")
    if froze:
        times = sorted(b.froze_at for b in froze)
        spread = times[-1] - times[0]
        kind = ("GLOBAL (all froze ~same time → server-wide stall)"
                if len(froze) == len(ok) and spread < 2.0
                else "PARTIAL/per-session")
        print(f"FREEZE: {len(froze)} bots, times={times}, kind={kind}")
        return 1
    print("NO FREEZE across all bots")
    return 0


if __name__ == "__main__":
    sys.exit(main())
