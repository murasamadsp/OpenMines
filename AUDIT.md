# Аудит паритета с C# референсом

Дата: 2026-05-14. Проведён сравнением Rust-кода с `server_reference/` построчно.
Статусы: ✅ подтверждено | ⚠️ расхождение | ❌ не реализовано

---

## Фаза 1: MVP (сеть и мир)

| Пункт | Статус | Комментарий |
|-------|--------|-------------|
| TCP wire-формат 1:1 | ✅ | `Packet::try_decode` корректен, LE, включает len |
| OnConnected: ST→AU→PI | ✅ | `connection.rs:39-42`, порядок совпадает с `Session.cs:40-42` |
| Auth OK: `cf` первым → `Gu("_")` → Init() | ✅ | `player/init.rs` соответствует C# `Player.Init()` |
| Auth FAIL: `cf` → `BI(id=-1)` → `HB` → `GU` | ✅ | Подтверждено |
| MD5 токен hex lowercase | ✅ | `auth/login.rs` |
| Ping/Pong 1:1 | ⚠️ | C# делает `Thread.Sleep(200)` перед ответом `PI` (ROADMAP это зафиксировал, убрано намеренно). Но: C# читает `lastpong = ServerTime.Now` **в самом начале** `Ping()` до sleep. Rust делает то же самое. ОК. |
| World mmap 32x32 | ✅ | `world/mod.rs` |
| TY wrapper decode 1:1 | ✅ | `[4B event][u32 time][u32 x][u32 y][sub...]` |
| HB `M`/`X`/`L` подпакеты | ✅ | `protocol/packets.rs` |
| SQLite схема | ✅ | `db/mod.rs` |

---

## Фаза 2: Копание и стройка

| Пункт | Статус | Комментарий |
| - | - | - |
| `Xdig` 200ms cooldown | ⚠️ | **РАСХОЖДЕНИЕ:** Rust: 333ms (`cd.last_dig.elapsed() < 333`). C# `TryAct` с `player.ServerPause = (OnRoad ? pause*5*0.80 : pause*5) * 1.4 / 1000`. При базовом `pause=10000` → `10000*5*1.4/1000 = 70s`! Это явно не 200ms. Но C# Session.cs:227: `DigHandler => TryAct(..., 200)`. Значит: в C# DigHandler передаёт `200`, а не `ServerPause`. Rust передаёт `333`. **Ошибка: должно быть 200ms, не 333ms.** |
| BOX special case (cell 90) | ✅ | `dig_build.rs:105-131` |
| MilitaryBlock (81) | ✅ | `dig_build.rs:139-155` |
| Crystal accumulator `cb` | ✅ | `dig_build.rs:168-226` — точно соответствует `Player.Mine()` |
| Boulder push every hit | ✅ | `dig_build.rs:230-251` |
| Dig exp only on destroy | ✅ | `dig_build.rs:254-268` |
| MineGeneral exp every hit | ✅ | `dig_build.rs:188-192` |
| Crystal FX (fx=2) broadcast | ✅ | `dig_build.rs:208-221` |
| FX broadcast BEFORE `!diggable` check | ✅ | `dig_build.rs:92-102` — комментарий правильный |
| Build G/R/O/V типы 1:1 | ✅ | `dig_build.rs:385-474` |
| BuildYellow/Red upgrade | ✅ | `dig_build.rs:393-445` |
| `V` (MilitaryBlock) без задержки | ⚠️ | C# ставит `MilitaryBlockFrame` (80) сначала, через 10 тиков → `MilitaryBlock` (81). Rust ставит сразу 81. Есть TODO-комментарий, но это расхождение с C#. |
| Build skill exp | ✅ | `dig_build.rs:477-486` |
| `TADG` toggle | ✅ | Подтверждено |
| Кристаллы/корзина `@B` | ✅ | Отправляется в нужных местах |
| Смерть — crystal box drop | ✅ | `death.rs:42-75` |
| Смерть — hb_bot_del broadcast | ✅ | `death.rs:203-210` |
| Смерть — prog stop | ✅ | `death.rs:158-162` |
| Смерть — Gu close | ✅ | `death.rs:236` |
| Смерть — `@T` телепорт | ✅ | `death.rs:237` |
| Смерть — HP reset | ✅ | `death.rs:238` |
| `Hurt` AntiGun damage reduction | ⚠️ | `hurt_player_pure` не применяет AntiGun (название говорит "Pure"). C# `Player.Hurt(num, DamageType.Pure)` тоже не применяет AntiGun при Pure. Но `gun_firing_system` должен вызывать `Hurt(DamageType.Gun)`. Надо проверить что он не вызывает `hurt_player_pure`. |
| Death — FindEmptyForBox BFS | ⚠️ | C# делает BFS через `FindEmptyForBox`. Rust использует `pick_box_coord` из `db/mod.rs`. Алгоритм может отличаться. Проверить. |
| `Xhea` heal | ✅ | `heal_inventory.rs` — Repair skill effect |
| `INVN` toggle | ✅ | |
| `INUS` all items | ✅ | Подтверждено |
| `INCL` selection | ✅ | |
| Геопаки `Xgeo` | ✅ | |

---

## Фаза 3: Здания

| Пункт | Статус | Комментарий |
|-------|--------|-------------|
| Pack HB 'O' subpacket | ✅ | |
| Resp bind/fill/charge/cost | ✅ | |
| Teleport list/action | ✅ | |
| Up skill GUI | ✅ | |
| Market buy/sell/commission | ✅ | |
| Gun firing radius/damage | ✅ | |
| Gate clan blocking | ✅ | |
| Storage deposit/withdraw | ✅ | |
| Crafter 8 recipes | ✅ | |
| BotSpot ECS | ✅ | |

---

## Фаза 4: Скиллы

| Пункт | Статус | Комментарий |
|-------|--------|-------------|
| 58 типов, дерево зависимостей | ✅ | |
| Формулы effect 1:1 | ✅ | |
| `@S` пакет | ✅ | |
| `LV` пакет | ✅ | |
| `player.pause` — Movement skill | ⚠️ | **РАСХОЖДЕНИЕ ВАЖНОЕ.** C# `Player.pause` (строка 75-92): итерирует все скиллы, ищет `SkillType.Movement`, берёт `c.Effect * 100`. Это `xy_pause`. Затем `ServerPause = (OnRoad ? pause*5*0.80 : pause*5) * 1.4 / 1000`. Этот `ServerPause` передаётся в `TryAct` при движении (`Session.cs:236`). В Rust **это не реализовано** — cooldown при движении убран полностью. Это и есть TD-1 (speed hack). |

---

## Фаза 5: Кланы

| Пункт | Статус | Комментарий |
|-------|--------|-------------|
| Создание (1000 creds) | ✅ | |
| Ранги Member/Officer/Leader | ✅ | |
| invite/request | ✅ | |
| Gun/Gate clan immunity | ✅ | |
| cS/cH transitions | ✅ | |
| AccessGun formula | ✅ | |

---

## Фаза 6: Чат

| Пункт | Статус | Комментарий |
|-------|--------|-------------|
| `mU` wire-формат | ✅ | Корень «FED не работает»: слал `±COLOR…` (6 полей, ведущий `±`) → клиент `int.Parse(array[0])`=`""`→`FormatException`. Fix: `ID±COLOR±CID±TIME±NICK±TEXT±GID`, `id`=rowid. Реф `GCMessage` тоже неверен — клиент важнее. Probe-verified live. Спец: `docs/CLIENT_PROTOCOL_GAPS.md` §1. |
| FED/DNO routing | ✅ | C# `Chat.cs:44` хардкодит wire-`ch="FED"` для ЛЮБОГО global (DNO — реальный канал `World.cs:74-75`) → DNO-зритель не видел DNO, текло в FED («в дно не показываются»). Fix: wire-`ch`=реальный `channel_tag`. Probe: FED→ch=FED, DNO→ch=DNO. Реф-баг. |
| Clan channel | ✅ | Тот же wire-fix; CLAN pseudo-channel из БД. |
| Навигация `Cmen`/`Choo`/`Cset`/`Cpri` | ✅ | Реф ИХ НЕ обрабатывает (`Session.cs` только пустой `Chin`; `TYPacket.cs` декодит, `default://Invalid`). Реализовано по клиенту (источник правды): список каналов / вход-переключение (с гейтом прав) / цвет (`chat_color`+миграция) / ЛС (`_min_max`, рассылка только участникам). Probe-verified. GAPS §3–6. |
| `Chin` ресинк + login | ✅ | Реф `Chin` ПУСТ (неполон — клиент шлёт `getLasts()` для инкремент-догрузки). Итог: login=`mO`-only; `Chin "_"`→полная история, `Chin "1:cur:lasts"`→`id>lastid`. Снят баг дублей на реконнекте. Probe-verified. GAPS §2. |
| Локальный HB bubble | ✅ | |
| Консольные команды | ✅ | |
| История FED/DNO переживает рестарт | ✅ | Грузится из БД в `GameState::new` (реф EF-навигатор не портирован). |
| Представление сообщения live=история | ✅ pass-2 | PROBE-VERIFIED (live VPS 2026-05-17). Миграция прод-БД подтверждена логом; `tools/chat_probe_pass2.py`: live `mU` == in-mem == БД-история-после-рестарта (id/color/time/gid>0/cid идентичны; `gid=2` НЕ 0 → «мелкие после рестарта» закрыт). Реализация: миграция `chat_messages.player_id`+`color`; `add_chat_message`→`(id,color)` снимок `chat_color`(sys=50); `get_recent` `LEFT JOIN players`→clan; единый `dotnet_epoch_minutes` (1:1 `GLine.time`). Спека: GAPS §1. |

---

## Фаза 7: Программатор

| Пункт | Статус | Комментарий |
|-------|--------|-------------|
| PROG/PDEL/PREN dispatch | ✅ | |
| LZMA парсер | ✅ | |
| `@P` статус | ✅ | |
| `tail` в HB bot | ✅ | |
| Пошаговое выполнение — GoTo | ✅ | |
| Пошаговое выполнение — RunSub/RunFunction | ✅ | |
| Пошаговое выполнение — ReturnFunction | ✅ | |
| Пошаговое выполнение — MacrosDig/MacrosHeal | ✅ | |
| Пошаговое выполнение — RunOnRespawn | ✅ | Проверено и реализовано в `death.rs`: программатор продолжает работу, если `is_free_resp` и `goto_death` установлен. |
| `Run()` / `Run(p)` — Drop() перед запуском | ✅ | `ProgrammatorData.Drop()` → сбрасывает все счётчики |
| `Step()` — `delay` check | ✅ | |
| `Stop` action → `Run()` | ✅ | |
| `Flip` action | ✅ | |
| BotSpot programmator execution | ❌ | ROADMAP это отмечает |

---

## Фаза 8: Физика мира

| Пункт | Статус | Комментарий |
|-------|--------|-------------|
| Песок fall + diagonal | ✅ | |
| Boulder | ✅ | |
| BOX pickup | ✅ | |
| Кислота | ✅ | |
| Alive-клетки (7 типов) | ✅ | |
| Sand через Gate (gate pass-through) | ❌ | Отмечено в ROADMAP как "не реализовано". C# `Physics.Sand()` строка 38-41: если ячейка ниже == Gate и через одну пусто — прыгнуть через Gate. |

---

## Фаза 9: Предметы

| Пункт | Статус | Комментарий |
|-------|--------|-------------|
| Инвентарь HashMap + selected | ✅ | |
| Crafter 8 рецептов | ✅ | |
| Market | ✅ | |
| Storage | ✅ | |
| Boom/Протектор/C190/Полимер | ✅ | |

---

## Итог: список расхождений для исправления

### 🔴 Критические (нарушают игровой процесс)

0. **Чат: серия реф-неполнот/багов (репорты юзера).** Полная спека и
   статус — `docs/CLIENT_PROTOCOL_GAPS.md` (авторитетный источник, т.к.
   реф для чата неполон: `/////FIX THIS SH`). Кратко:
   - ✅ `mU` wire (`±` без `id` → `FormatException`) — fix формат
     `ID±COLOR±CID±TIME±NICK±TEXT±GID`, юнит-тест + probe-verified.
   - ✅ FED/DNO routing (хардкод `ch="FED"`) — probe-verified.
   - ✅ Навигация `Cmen/Choo/Cset/Cpri` (реф не реализует) —
     probe-verified.
   - ✅ `Chin`-ресинк + login=mO-only (снят баг дублей на
     реконнекте) — probe-verified.
   - ✅ **pass-2 PROBE-VERIFIED** (live VPS 2026-05-17): схема
     `chat_messages` хранит `player_id`+`color` (миграция прод-БД
     подтверждена логом); `tools/chat_probe_pass2.py` доказал
     live `mU` == in-mem == БД-история-после-`force-recreate`
     (id/color/time/`gid=2`(>0)/cid идентичны → «мелкие после
     рестарта» закрыт). Деплой `deploy-vps.sh` (rsync working
     tree, `up -d --force-recreate`, volume цел). GAPS §1.
   **Урок:** ✅ в аудите без проверки против КЛИЕНТА — недостоверны;
   `server_reference` для чата неполон, клиент — источник правды.

1. **Cooldown `Xdig` и `Xbld`: 333ms вместо 200ms.**
   - Файл: `server/net/session/play/dig_build.rs:21` и `:313`
   - Референс: `Session.cs:227,233` → `TryAct(..., 200)`
   - Fix: заменить `333` на `200` в обоих местах.

2. **Отсутствует серверный cooldown на `Xmov`.**
   - Файл: `server/net/session/play/movement.rs`
   - Референс: `Session.cs:236` → `TryAct(() => Move(...), player.ServerPause)`
   - `player.ServerPause` = `(OnRoad ? pause*5*0.80 : pause*5) * 1.4 / 1000`
   - Fix: добавить `last_move_at: Instant` в `PlayerCooldowns`, проверять `elapsed < server_pause`.
   - `server_pause` берётся из `sp` пакета (xy_pause из Movement skill).

3. **`V` (MilitaryBlock) без задержки.**
   - Файл: `server/net/session/play/dig_build.rs:464-473`
   - Референс: C# ставит `MilitaryBlockFrame` (80), через 10 тиков → `MilitaryBlock` (81).
   - Fix: поставить cell_type 80, добавить таймер в ECS на конвертацию.

### 🟡 Требуют проверки

4. **`RespawnOnProg` при смерти программатора.**
   - Проверить `death.rs` — корректно ли обрабатывается случай `resp.cost==0 && GotoDeath!=null`.

5. **`Hurt(DamageType.Gun)` — AntiGun reduction в `gun_firing_system`.**
   - `hurt_player_pure` пропускает AntiGun. Убедиться что gun_firing_system не вызывает `pure`, а применяет AntiGun.

6. **Формат `mO` пакета (чат при логине).**
   - Проверить: `"TAG:Name"` или `"TAG"`. C#: `new CurrentChatPacket(currentchat.tag, currentchat.Name)`.

7. **Death `FindEmptyForBox` BFS vs `pick_box_coord`.**
   - Сравнить алгоритмы поиска пустой клетки для box drop.

### 🟢 Подтверждено ✅

- Wire-формат, Auth lifecycle, Ping/Pong
- Копание: crystal accumulator, boulder, dig exp, FX
- Здания: все типы
- Скиллы: дерево, формулы, @S/@LV
- Кланы: всё
- Программатор: Step/GoTo/Run/Stop/Flip
- Физика: sand, boulder, acid, alive