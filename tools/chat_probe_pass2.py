#!/usr/bin/env python3
"""Chat pass-2 probe: live mU == история mU для ОДНОГО сообщения.

Проверяет фикс CLIENT_PROTOCOL_GAPS §1 pass-2 на ЖИВОМ сервере (не unit).
Поля `mU.h[i]` = `ID±COLOR±CID±TIME±NICK±TEXT±GID` (клиент-контракт).

Фазы:
  --phase send   : послать уникальное FED-сообщение, поймать live `mU`,
                   затем `Chin "_"` (in-mem история) — сверить ОДИН id:
                   color/time/gid/cid идентичны, gid>0 (НЕ легаси-0).
                   Сохранить эталон в tools/.p2_ref.json.
  (между фазами оператор делает up -d --force-recreate → DB-reload)
  --phase verify : `Chin "_"` (история ИЗ БД после рестарта) — найти тот
                   же id, сверить с .p2_ref.json. Это и есть баг
                   «старые становятся мелкими после рестарта».

Тоннель: ssh -fN -L 18090:127.0.0.1:8090 vps
"""
import argparse, hashlib, json, os, sys, time
from datetime import datetime, timedelta
sys.path.insert(0, os.path.dirname(__file__))
from om_net import OpenMinesClient, frame, ty

CREDS = os.path.join(os.path.dirname(__file__), ".repro_creds.json")
REF = os.path.join(os.path.dirname(__file__), ".p2_ref.json")
DOTNET_UNIX_EPOCH_MIN = 1_035_593_280  # (621_355_968_000_000_000/10000/60000)


def parse_mu(payload: bytes):
    """`{"ch","h":[...]}` → list[dict] из 7-польных ±-записей."""
    obj = json.loads(payload.decode("utf-8"))
    out = []
    for raw in obj.get("h", []):
        p = raw.split("±")
        if len(p) != 7:
            out.append({"_bad": raw})
            continue
        out.append({
            "id": int(p[0]), "color": int(p[1]), "cid": int(p[2]),
            "time": int(p[3]), "nick": p[4], "text": p[5], "gid": int(p[6]),
            "ch": obj.get("ch"),
        })
    return out


def auth(c) -> int:
    if not c.wait(lambda: c.sid is not None, 6):
        print("FAIL: no sid"); sys.exit(2)
    creds = json.load(open(CREDS))
    tok = hashlib.md5((creds["hash"] + c.sid).encode()).hexdigest()
    c.send(frame(b"U", b"AU", f"bot_{creds['id']}_{tok}".encode()))
    if not c.wait(lambda: c.spawn is not None, 10):
        print("FAIL: no @T spawn (auth failed?)"); sys.exit(2)
    return int(creds["id"])


def render_time(mins: int) -> str:
    # Клиент: new DateTime((long)time*60000*10000 тиков). .NET tick-эпоха
    # = 0001-01-01; minutes-with-epoch → вычитаем сдвиг до Unix.
    unix_min = mins - DOTNET_UNIX_EPOCH_MIN
    try:
        return (datetime(1970, 1, 1) + timedelta(minutes=unix_min)).strftime("%Y-%m-%d %H:%M UTC")
    except OverflowError:
        return f"<overflow {mins}>"


def cmp_fields(a: dict, b: dict, keys=("id", "color", "cid", "time", "nick", "text", "gid")):
    return [k for k in keys if a.get(k) != b.get(k)]


def phase_send(c, bot_id):
    tag = b"FED"
    c.send(ty(b"Choo", 0, 0, tag, int(time.time() * 1000)))
    time.sleep(1.2)
    c.drain_mu()
    token = f"p2_{int(time.time())}"
    print(f"=== SEND FED {token!r} (bot id={bot_id}) ===")
    c.send(ty(b"Chat", 0, 0, token.encode(), int(time.time() * 1000)))
    time.sleep(2.5)
    live_raw = c.drain_mu()
    live = None
    for pl in live_raw:
        for m in parse_mu(pl):
            if m.get("text") == token:
                live = m
    if not live:
        print(f"  ✗ live mU с text={token!r} не пришёл. raw={live_raw[:2]}")
        return 1
    print(f"  live: {live}")
    print(f"  live.time → {render_time(live['time'])}")

    fails = []
    if live["gid"] <= 0:
        fails.append(f"live gid={live['gid']} (<=0 → клиент fontSize=10, баг НЕ закрыт)")
    if live["gid"] != bot_id:
        fails.append(f"live gid={live['gid']} != bot id={bot_id}")
    if live["text"] != token:
        fails.append(f"live text={live['text']!r} != {token!r}")
    now_min = int(time.time()) // 60 + DOTNET_UNIX_EPOCH_MIN
    if abs(live["time"] - now_min) > 1440:  # ±1 сутки
        fails.append(f"live time={live['time']} далеко от now≈{now_min} "
                     f"(→ {render_time(live['time'])}; неверная единица?)")

    # half-a: in-mem история (Chin "_") == live для того же id
    c.send(ty(b"Chin", 0, 0, b"_", int(time.time() * 1000)))
    time.sleep(1.8)
    hist_raw = c.drain_mu()
    hist = None
    for pl in hist_raw:
        for m in parse_mu(pl):
            if m.get("id") == live["id"]:
                hist = m
    if not hist:
        fails.append(f'Chin "_" (in-mem): нет записи id={live["id"]} '
                     f'(история не отдалась)')
    else:
        print(f"  in-mem история: {hist}")
        diff = cmp_fields(live, hist)
        if diff:
            fails.append(f"live ≠ in-mem история по {diff} "
                         f"(live={ {k:live[k] for k in diff} }, "
                         f"hist={ {k:hist[k] for k in diff} })")
        else:
            print("  ✓ live == in-mem история (id/color/time/gid/cid идентичны)")

    json.dump(live, open(REF, "w"))
    print(f"  эталон → {REF}")
    return _verdict(fails, "SEND/half-a (live vs in-mem)")


def phase_verify(c, bot_id):
    if not os.path.exists(REF):
        print(f"FAIL: нет {REF} (сначала --phase send)"); return 2
    ref = json.load(open(REF))
    print(f"=== VERIFY: история ИЗ БД после рестарта vs эталон id={ref['id']} ===")
    c.send(ty(b"Choo", 0, 0, b"FED", int(time.time() * 1000)))
    time.sleep(1.2)
    c.drain_mu()
    c.send(ty(b"Chin", 0, 0, b"_", int(time.time() * 1000)))
    time.sleep(2.0)
    hist = None
    for pl in c.drain_mu():
        for m in parse_mu(pl):
            if m.get("id") == ref["id"]:
                hist = m
    fails = []
    if not hist:
        fails.append(f'id={ref["id"]} НЕ в истории из БД после рестарта '
                     f'(сообщение не персистнулось?)')
    else:
        print(f"  из БД: {hist}")
        print(f"  из БД time → {render_time(hist['time'])}")
        if hist["gid"] <= 0:
            fails.append(f'gid={hist["gid"]} после рестарта (<=0 → мелкий '
                         f'шрифт; player_id НЕ персистнулся — РОВНО баг юзера)')
        diff = cmp_fields(ref, hist)
        if diff:
            fails.append(f"БД-история ≠ эталон по {diff} "
                         f"(ref={ {k:ref[k] for k in diff} }, "
                         f"db={ {k:hist[k] for k in diff} })")
        else:
            print("  ✓ БД-история после рестарта == live-эталон "
                  "(player_id/color/time персистнули; gid>0)")
    return _verdict(fails, "VERIFY (post-restart DB history)")


def phase_diag(c, bot_id):
    """Улики для двух репортов юзера: (A) что прилетает idle-клиенту
    периодически (~0.5с мигание ползунка); (B) распределение gid в
    FED-истории (мелкий шрифт = сколько записей gid<=0)."""
    print("=== A. IDLE 9s — ВСЕ пакеты с таймингом (ищем ~0.5с период) ===")
    with c.lock:
        c.evlog.clear()
    time.sleep(9.0)
    with c.lock:
        ev = list(c.evlog)
    for tr, e, ln, head in ev:
        print(f"  T+{tr:6.3f} {e:3} len={ln:5} {head[:60]!r}")
    # период по событию: дельты T между одинаковыми ev
    from collections import defaultdict
    times = defaultdict(list)
    for tr, e, _, _ in ev:
        times[e].append(tr)
    print("  -- межпакетные интервалы по типу --")
    for e, ts in sorted(times.items()):
        if len(ts) >= 2:
            d = [round(ts[i + 1] - ts[i], 3) for i in range(len(ts) - 1)]
            print(f"     {e}: n={len(ts)} Δ={d}")
        else:
            print(f"     {e}: n={len(ts)} (one-shot)")
    if not ev:
        print("  (idle-клиент НЕ получает ничего — мигание чисто клиентское/Unity)")

    print('\n=== B. FED Chin "_" — распределение gid (мелкий шрифт) ===')
    c.send(ty(b"Choo", 0, 0, b"FED", int(time.time() * 1000)))
    time.sleep(1.2)
    c.drain_mu()
    c.send(ty(b"Chin", 0, 0, b"_", int(time.time() * 1000)))
    time.sleep(2.0)
    rows = []
    for pl in c.drain_mu():
        rows += parse_mu(pl)
    if not rows:
        print("  (история пуста)")
        return 0
    small = [m for m in rows if m.get("gid", 0) <= 0]
    ok = [m for m in rows if m.get("gid", 0) > 0]
    print(f"  всего={len(rows)}  gid>0(норм)={len(ok)}  "
          f"gid<=0(МЕЛКИЙ шрифт)={len(small)}")
    for m in rows[-12:]:
        flag = "  <-- МЕЛКИЙ" if m.get("gid", 0) <= 0 else ""
        print(f"   id={m.get('id')} gid={m.get('gid')} color={m.get('color')} "
              f"cid={m.get('cid')} t={m.get('time')} "
              f"nick={m.get('nick')!r} txt={str(m.get('text'))[:18]!r}{flag}")
    if small:
        print(f"  ⚠ {len(small)}/{len(rows)} ЛЕГАСИ-строк (player_id=0 до "
              f"миграции) → клиент рисует их fontSize=10. Это и есть "
              f"«всё равно мелкий шрифт» — старая история до фикса.")
    return 0


def phase_watch(c, bot_id):
    """Реал-клиент-подобное репро: после spawn клиент шлёт Chin "_"
    (WorldInitScript). Затем 25с пишем ВЕСЬ поток + ищем периодику
    (~0.5с мигание ползунка = что-то релэйаутит чат-панель)."""
    print("=== пост-spawn: Chin \"_\" (как реал-клиент WorldInit) ===")
    c.send(ty(b"Chin", 0, 0, b"_", int(time.time() * 1000)))
    time.sleep(1.5)
    with c.lock:
        c.evlog.clear()
    WIN = 25
    print(f"=== WATCH {WIN}s — ВЕСЬ поток (бот в мире, не двигается) ===")
    time.sleep(WIN)
    with c.lock:
        ev = list(c.evlog)
    for tr, e, ln, head in ev:
        print(f"  T+{tr:6.3f} {e:3} len={ln:5} {head[:54]!r}")
    from collections import defaultdict
    t_by = defaultdict(list)
    for tr, e, _, _ in ev:
        t_by[e].append(tr)
    print(f"  -- всего {len(ev)} пакетов за {WIN}s; интервалы по типу --")
    for e, ts in sorted(t_by.items()):
        if len(ts) >= 2:
            d = [round(ts[i + 1] - ts[i], 2) for i in range(len(ts) - 1)]
            avg = round(sum(d) / len(d), 2)
            print(f"     {e}: n={len(ts)} avg_Δ={avg}s Δ={d[:18]}")
        else:
            print(f"     {e}: n={len(ts)} (one-shot)")
    if not ev:
        print("  (0 пакетов — бот-сессия отличается от реал-клиента; "
              "симптом не воспроизвёлся этим репро)")
    return 0


def _verdict(fails, label):
    print(f"\n=== ИТОГ {label} ===")
    if fails:
        for f in fails:
            print(f"  ✗ {f}")
        print(f"  FAIL ({len(fails)})")
        return 1
    print("  ✓ PASS")
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=18090)
    ap.add_argument("--phase", choices=("send", "verify", "diag", "watch"), required=True)
    a = ap.parse_args()
    c = OpenMinesClient(a.host, a.port)
    bot_id = auth(c)
    print(f"sid={c.sid} spawn={c.spawn} bot_id={bot_id}")
    rc = ({"send": phase_send, "verify": phase_verify,
           "diag": phase_diag, "watch": phase_watch}[a.phase])(c, bot_id)
    c.alive = False
    return rc


if __name__ == "__main__":
    sys.exit(main())
