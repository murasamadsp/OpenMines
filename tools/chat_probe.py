#!/usr/bin/env python3
"""FED-чат probe: что сервер РЕАЛЬНО шлёт на login и на Chat-сообщение.

Не коммитится (tools/ исключён). Авторизуется как repro-бот, логирует ВСЕ
пакеты (особо mO/mU/mL/mN), затем шлёт TY "Chat" и логирует ответ.
Запуск через тоннель:
    ssh -fN -L 18090:127.0.0.1:8090 vps
    python3 tools/chat_probe.py --host 127.0.0.1 --port 18090
"""
import argparse, hashlib, json, os, sys, time
sys.path.insert(0, os.path.dirname(__file__))
from om_net import OpenMinesClient, frame, ty

CREDS = os.path.join(os.path.dirname(__file__), ".repro_creds.json")
WATCH = {b"mO", b"mU", b"mL", b"mN", b"mC", b"cf", b"Gu", b"@T", b"AU", b"AH", b"BI"}


def _json_loads_h(mu_payload: bytes):
    """`h` array из mU `{"ch":..,"h":[..]}`."""
    import json as _j
    return _j.loads(mu_payload.decode("utf-8")).get("h")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=18090)
    a = ap.parse_args()
    c = OpenMinesClient(a.host, a.port)
    if not c.wait(lambda: c.sid is not None, 5):
        print("FAIL: no sid"); return 2
    print(f"sid={c.sid}")
    creds = json.load(open(CREDS)) if os.path.exists(CREDS) else None
    if not creds:
        print("FAIL: no .repro_creds.json (run repro_freeze once to register)"); return 2
    tok = hashlib.md5((creds["hash"] + c.sid).encode()).hexdigest()
    c.send(frame(b"U", b"AU", f"bot_{creds['id']}_{tok}".encode()))
    print(f"AU id={creds['id']}")
    if not c.wait(lambda: c.spawn is not None, 8):
        print("FAIL: no @T spawn"); return 2
    print(f"spawn={c.spawn}")
    time.sleep(2.0)  # collect full login burst

    print("\n=== LOGIN PACKETS (chat-relevant) ===")
    with c.lock:
        login_log = [x for x in c.log if x[1] in WATCH or x[1] in (b"mU", b"mO")]
    for tr, ev, pl in login_log:
        print(f"  T+{tr:6.3f} {ev.decode('ascii','replace'):3} "
              f"len={len(pl):4} {pl[:300]!r}")

    fails = []

    # Контракт §2: login шлёт ТОЛЬКО mO (история — через Chin, иначе
    # дубли на реконнекте). Проверяем: в login-пакетах есть mO, НЕТ mU.
    login_evs = [ev for _, ev, _ in login_log]
    if b"mO" not in login_evs:
        fails.append("login: нет mO")
    if b"mU" in login_evs:
        fails.append("login: пришёл mU (login должен быть mO-only — иначе "
                      "дубли на реконнекте; история через Chin)")
    else:
        print("  ✓ login = mO без mU (история отдана Chin'у)")

    # Chin "_" (первый вход) → ПОЛНАЯ история текущего канала (mU),
    # без mL, без mO. Реальный клиент шлёт это в WorldInitScript:101.
    with c.lock:
        c.log.clear()
    print('\n=== SEND TY Chin "_" → ждём ПОЛНЫЙ mU (без mL/mO) ===')
    c.send(ty(b"Chin", 0, 0, b"_", int(time.time() * 1000)))
    time.sleep(1.8)
    with c.lock:
        chin_resp = list(c.log)
    for tr, ev, pl in chin_resp:
        print(f"  {ev.decode('ascii','replace'):3} {pl[:160]!r}")
    chin_evs = [ev for _, ev, _ in chin_resp]
    if b"mU" not in chin_evs:
        fails.append('Chin "_": нет mU (история не пришла)')
    if b"mL" in chin_evs:
        fails.append('Chin "_": пришёл mL (ломает вход в чат)')
    if b"mU" in chin_evs and b"mL" not in chin_evs:
        print('  ✓ Chin "_" → полный mU, без mL')

    # Chin "1:FED:FED#<огромный id>" (реконнект, клиент уже всё видел) →
    # mO + mU с ПУСТЫМ h (ничего новее) — НЕ полная пере-отправка (дубль).
    with c.lock:
        c.log.clear()
    print('\n=== SEND TY Chin "1:FED:FED#999999" → ждём mU h=[] (без дублей) ===')
    c.send(ty(b"Chin", 0, 0, b"1:FED:FED#999999", int(time.time() * 1000)))
    time.sleep(1.8)
    with c.lock:
        rc = list(c.log)
    for tr, ev, pl in rc:
        print(f"  {ev.decode('ascii','replace'):3} {pl[:160]!r}")
    mu = next((pl for _, ev, pl in rc if ev == b"mU"), None)
    if mu is None:
        fails.append('Chin reconnect: нет mU')
    else:
        try:
            h = _json_loads_h(mu)
            if h == []:
                print("  ✓ реконнект с актуальным lastid → mU h=[] (нет дублей)")
            else:
                fails.append(f'Chin reconnect: h не пуст ({len(h)}) — '
                             f'сервер шлёт уже виденное → дубли')
        except Exception as e:
            fails.append(f'Chin reconnect: mU не парсится: {e}')

    with c.lock:
        c.log.clear()
    msg = f"probe{int(time.time())%100000}".encode()
    print(f"\n=== SEND TY Chat sub={msg!r} ===")
    c.send(ty(b"Chat", 0, 0, msg, int(time.time() * 1000)))
    time.sleep(2.5)
    print("=== PACKETS AFTER Chat ===")
    with c.lock:
        after = list(c.log)
    if not after:
        print("  (NOTHING received after Chat — server did not broadcast mU)")
    for tr, ev, pl in after:
        print(f"  T+{tr:6.3f} {ev.decode('ascii','replace'):3} "
              f"len={len(pl):4} {pl[:400]!r}")

    # ── Навигация чата: Cmen / Choo / Cset / Cpri ──────────────────────
    # (fails определён выше — НЕ переинициализировать)

    def burst(ev4: bytes, sub: bytes, wait=1.8):
        with c.lock:
            c.log.clear()
        c.send(ty(ev4, 0, 0, sub, int(time.time() * 1000)))
        time.sleep(wait)
        with c.lock:
            return list(c.log)

    print('\n=== Cmen "_" → ждём mL (записи: TAG±NOTIF±TITLE±NICK: TEXT, 4 части) ===')
    r = burst(b"Cmen", b"_")
    ml = [pl for _, ev, pl in r if ev == b"mL"]
    for tr, ev, pl in r:
        print(f"  {ev.decode('ascii','replace'):3} {pl[:300]!r}")
    if not ml:
        fails.append("Cmen: нет mL")
    else:
        for entry in ml[0].decode("utf-8", "replace").split("#"):
            if entry and len(entry.split("±")) != 4:
                fails.append(f"Cmen: запись mL не 4 части: {entry!r}")
                break
        else:
            print("  ✓ mL получен, все записи по 4 ±-части")

    print('\n=== Choo "DNO" → ждём mO потом mU, БЕЗ mL ===')
    r = burst(b"Choo", b"DNO")
    evs = [ev for _, ev, _ in r]
    for tr, ev, pl in r:
        print(f"  {ev.decode('ascii','replace'):3} {pl[:200]!r}")
    if b"mO" not in evs:
        fails.append("Choo: нет mO")
    if b"mU" not in evs:
        fails.append("Choo: нет mU")
    if b"mL" in evs:
        fails.append("Choo: пришёл mL (не должен — режим списка)")
    mo = next((pl for _, ev, pl in r if ev == b"mO"), b"")
    if not mo.startswith(b"DNO:"):
        fails.append(f"Choo: mO не 'DNO:...': {mo[:40]!r}")
    if not [f for f in fails if f.startswith("Choo")]:
        print("  ✓ mO(DNO:...) + mU, mL отсутствует")

    print('\n=== Cset "_" ×2 → ждём два mC с соседними кодами (цикл+персист) ===')
    r1 = burst(b"Cset", b"_")
    r2 = burst(b"Cset", b"_")
    c1 = next((pl for _, ev, pl in r1 if ev == b"mC"), None)
    c2 = next((pl for _, ev, pl in r2 if ev == b"mC"), None)
    print(f"  mC#1={c1!r} mC#2={c2!r}")
    if c1 is None or c2 is None:
        fails.append("Cset: нет mC")
    else:
        try:
            n1, n2 = int(c1), int(c2)
            if n2 == (n1 + 1) % 20:
                print(f"  ✓ цвет циклится: {n1} → {n2}")
            else:
                fails.append(f"Cset: код не +1 mod20: {n1}→{n2}")
        except ValueError:
            fails.append(f"Cset: mC не число: {c1!r},{c2!r}")

    print('\n=== Cpri "1" → ждём mO начинающийся с "_" (ЛС) ===')
    r = burst(b"Cpri", b"1")
    for tr, ev, pl in r:
        print(f"  {ev.decode('ascii','replace'):3} {pl[:200]!r}")
    mo = next((pl for _, ev, pl in r if ev == b"mO"), None)
    if mo is None:
        print("  (нет mO — игрок id=1 не существует на этом сервере? инфо, не fail)")
    elif mo.startswith(b"_"):
        print(f"  ✓ приватный mO: {mo[:40]!r}")
    else:
        fails.append(f"Cpri: mO не приватный (нет ведущего '_'): {mo[:40]!r}")

    # Маршрутизация каналов (баг «в дно не показываются»): сообщение в
    # FED должно прийти с ch="FED", в DNO — с ch="DNO" (НЕ хардкод "FED").
    import json as _json

    def chat_in(tag: bytes, body: bytes):
        burst(b"Choo", tag, wait=1.2)
        r = burst(b"Chat", body, wait=2.0)
        for _, ev, pl in r:
            if ev == b"mU":
                try:
                    return _json.loads(pl.decode("utf-8")).get("ch")
                except Exception:
                    return f"<bad json {pl[:60]!r}>"
        return None

    print('\n=== Маршрутизация: Chat в FED → ch=FED, в DNO → ch=DNO ===')
    fed_ch = chat_in(b"FED", b"routefed")
    dno_ch = chat_in(b"DNO", b"routedno")
    print(f"  FED-сообщение пришло с ch={fed_ch!r}; DNO-сообщение ch={dno_ch!r}")
    if fed_ch != "FED":
        fails.append(f"routing: FED msg ch={fed_ch!r} (ждали 'FED')")
    if dno_ch != "DNO":
        fails.append(f"routing: DNO msg ch={dno_ch!r} (ждали 'DNO'; "
                     f"баг 'в дно не показываются' НЕ закрыт)")
    if fed_ch == "FED" and dno_ch == "DNO":
        print("  ✓ каналы маршрутизируются раздельно (DNO-фикс работает)")

    print("\n=== ИТОГ ===")
    if fails:
        for f in fails:
            print(f"  ✗ {f}")
        print(f"  FAIL ({len(fails)})")
    else:
        print("  ✓ ВСЁ ОК: Cmen/Choo/Cset/Cpri по контракту клиента")
    c.alive = False
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
