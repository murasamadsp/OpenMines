# OpenMines roadmap

Восстановление Mines 3. Клиент: Unity 2019.4.10f1. Сервер: Rust + SQLite + mmap, TCP-binary.
Все пункты верифицируются по C# референсу (`server_reference/`).

---

## STATUS 2026-06-22 — паритет-аудит 10 подсистем + 13 фиксов (автономная сессия)

Аудит (10 субагентов: программатор, GUI, dig, combat, death, skills, market,
inventory, clans, chat — читали клиент+C#+Rust) подтвердил: **программатор 1:1**
и **все Building-GUI функциональны** — это были НЕ «большие рефакторинги», а доделки.

**⚠️ ТОП-ИЗВЕСТНЫЙ БАГ (НЕ исправлен — нужно решение):** clans — HB bot шлёт
`clan_id` вместо `icon`; клиент рендерит `sprites[value-1]` → неверные иконки в
мире + краш (IndexOutOfRange) при clan_id>218. C# имеет clan.id==icon, Rust
разделил (fdeacbf). Фикс широкий+wire+дизайн-вопрос. Детали — `.remember/remember.md`.

Найдено и **исправлено** (13 коммитов, каждый прошёл pre-commit; выделенные требуют
live-проверки в клиенте):
- market: dirty-флаг при сделках (теряли деньги при краше); chat: Locl весь payload
  = сообщение (эвристика резала «5:hi»); death P$/@B порядок; BuildWar 2-й exp; +ниже.

- programmator: MacrosBuild→no-op (был лишний build блок) — `9c85949`
- resp: персист clanzone в admin (терялся, хардкод 0) — `8121235`
- **combat: выстрел пушки стал ВИДИМ** — directed FX7 вместо немого FX1 — `6be45c8`
- dig/build: @S при dig-destroy/build (полосы скиллов не обновлялись) + бабл «+N»
  при подборе бокса — `726675f`
- **death: экономика респавна 1:1** (публичный респ бесплатен, нет ухода в минус,
  cost==0 не →10$) + self-broadcast (воскресший видим соседям) — `63d8b1b`
- skills: @S pct без клампа (состояние полосы >=200) + AntiGun усечение урона — `73f9744`
- dig: Movement exp при копании (1:1 C# Move→Bz) — `75aca7d`

**Намеренно НЕ применено — спорные кварки C# (решение пользователя):** dig D1 (списание
кристаллов при стройке в стену), D4 (бесплатные дороги на высоком lvl), D5 (durability
дорог=0). Детали — `.remember/remember.md`.

**Два реально больших трека впереди (оба требуют решения, НЕ автономно):**
ECS-миграция (Раздел 10, гейтнута паритетом, этапы 2-6 = точка невозврата) и
конфиг/балансировка (north-star, не в ROADMAP).

---

## STATUS 2026-05-17 — аудит фриза + 1:1 (автономная сессия)

Прежние сплошные `[x]` ниже были ПЕРЕОЦЕНЕНЫ: аудит (3 субагента +
инструментирование + детерминированные repro/sim тулзы `tools/`) нашёл
реальные расхождения и КОРЕНЬ МНОГОМЕСЯЧНОГО ФРИЗА. Этот раздел —
авторитетная истина; пункты ниже правятся по мере подтверждения.

### Корень фриза (НЕ был в роадмапе) — НАЙДЕН, ИСПРАВЛЕН, ВЕРИФИЦИРОВАН

Класс «медленная операция под локом / на tick-пути»:

- `Layer::flush` делал `fs::copy` ~3ГБ (durability 2ГБ) ПОД write-локом
  слоя каждые 60с → весь сервер вис секундами. **Fix:** msync(µs) под
  локом, `.bak` копия вне лока + раз в ~30мин. **Verified:** соло 95с/8
  чанков, froze=None (было: фриз на T+2.7с).
- C-1 `combat.rs` `standing_cell_hazard_system`: `db.get_box_at/
  delete_box_at` (SQLite) ПОД `ecs.write()` каждые 10ms при игроке на
  BOX. C-2 `death.rs` `upsert_box` под удержанным `ecs.write()`. H-1
  `dig_build.rs` то же на dig. **Fix:** in-mem `box_index` +
  отложенная персистенция (`box_persist_q`). **Verified:** 12 ботов,
  1295 смертей/боксов за 120с — сервер-тик 0 OVER-BUDGET (до фикса было
  бы тысячи стопоров).
- C-4 `on_disconnect`: `save_player` под `ecs.read()` (readers блокируют
  writer-тик) → фриз на каждый disconnect (объясняло «реконнект→снова
  фриз»). **Fix:** save вне лока.

### Исправлено в этой сессии + верифицировано

- [x] **PO→PI**: рефактор УДАЛИЛ ответ PI на PO (стр.19 ниже была ЛОЖНА)
  → клиент в HUD показывал «фриз». Восстановлено 1:1 `Session.Ping`.
  Verified: pi_replies=32/95с.
- [x] **Xdig/Xbld cooldown 333→200ms** (стр.33-34 были ЛОЖНЫ: код 333).
  1:1 `Session.cs:230,233 TryAct(...,200)`.
- [x] **@S при move/dig**: `add_skill_exp` всегда возвращал `false` →
  @S НИКОГДА не слался при exp за движение/копание (стр.63 ниже —
  формулировка неверна; C# `Skill.AddExp` шлёт @S всегда). Fix: 1:1.
- [x] **mU тег "FED"** жёстко 1:1 `Player.cs:665` (был фактический tag).
- [x] **Death FX2** безусловно 1:1 `Player.cs:912` (был под `if crys>0`).
- [x] **mU wire-формат — КОРЕНЬ «FED-чат не работает»** (репорт юзера).
  `protocol/packets.rs chat_messages` слал `±COLOR±…` (6 полей, ведущий
  `±`). Клиент `ChatManager.cs muHandler` (источник правды, неизменяем):
  `array=h[i].Split('±'); if(array.Length==7) GCMessage.id=int.Parse(array[0])`
  → `array[0]==""` → `FormatException` в Unity → НИ ОДНО сообщение чата
  (FED/DNO/CLAN/история) не отображалось. `server_reference GCMessage.Encode`
  ТОЖЕ неверен (ведущий `±`, нет `id`) — клиент важнее референса (CLAUDE.md).
  Fix: формат `ID±COLOR±CID±TIME±NICK±TEXT±GID`; `ChatMessage.id` = rowid
  `chat_messages` (дедуп клиента `LastIDs`); color 10 live / 1 история
  (1:1 `Chat.cs:44`/`Chat.GetMessages`); FED/DNO история грузится из БД при
  старте (переживает рестарт). Verified: unit-тест + probe на live VPS.
- [x] **`Chin` = РЕСИНК чата (НЕ no-op) + login=mO-only — корень «меню
  открывается / дубли на реконнекте»**. Эволюция (честно): (1) прежний
  `handle_chat_init_ty`→`send_chat_init` слал `mL` → клиент в «СПИСОК
  ЧАТОВ» поверх FED → «нельзя зайти». (2) Промежуточно: `Chin`=no-op
  (убрало `mL`-поломку, но login слал полную историю `mU` → на
  реконнекте клиент `muHandler` `AddLine`'ил всё повторно → ДУБЛИ,
  репорт юзера). (3) ИТОГ: login шлёт только `mO`; история — через
  `Chin`-resync по `getLasts()` клиента (`WorldInitScript.cs:109`):
  `"_"`→полная, `"1:cur:lasts"`→инкремент `id>lastid` (доступ
  гейтится). Реф `Session.Chin` ПУСТ — реф неполон, контракт по
  клиенту. Probe-verified: login=mO-only, `Chin "_"`→полный mU,
  реконнект с актуальным lastid→`mU h=[]` (без дублей). Спец:
  `docs/CLIENT_PROTOCOL_GAPS.md` §2.
- [x] **DNO routing — «в дно не показываются»** (репорт юзера). C#
  `Chat.cs:44` хардкодит wire-`ch="FED"` для ЛЮБОГО global (DNO —
  реальный канал) → DNO-зритель не видел DNO, оно текло в FED.
  Fix: wire-`ch` = реальный `channel_tag`. Probe-verified
  (FED→ch=FED, DNO→ch=DNO). Реф-баг; клиент важнее. GAPS §1.
- [x] **pass-2 РЕШЕНО + probe-verified на live VPS (2026-05-17)**:
  миграция `chat_messages` +`player_id`+`color`; `add_chat_message`
  резолвит/хранит снимок `chat_color` автора (sys=50), возвращает
  `(id,color)`; `get_recent_chat_messages` `LEFT JOIN players`→clan;
  единый `game::chat::dotnet_epoch_minutes` (1:1 `GLine.time`) в live
  И истории (startup+`load_db_history`); 3 места конструкции
  `ChatMessage` сведены. Легаси-строки (`player_id=0` до миграции →
  мелкий шрифт) — миграция-бэкфилл `player_id` по `player_name`→
  `players.name`; прод-лог `backfilled … 23 rows`; probe после:
  26/26 `gid>0`, 0 мелких. `tools/chat_probe_pass2.py` (send/verify/
  diag/watch). Спец — `docs/CLIENT_PROTOCOL_GAPS.md` §1.

### Известные открытые (НЕ серверный фикс — честно, 2026-05-17)

- [ ] **Мигание ползунка чата — КЛИЕНТСКИЙ Unity-баг, НЕ сервер,
  deferred.** Доказано: интернет ВЫКЛ → мигание есть. Корень —
  `m1client.unity` чат-`ScrollRect` `AutoHideAndExpandViewport`
  (петля layout). Клиент неизменяем → серверного рычага нет. Решение
  юзера: забить (косметика). `docs/CLIENT_PROTOCOL_GAPS.md` §1.
- [x] **PI-флуд ИСПРАВЛЕН умно (2026-05-17, deployed + user-verified
  в реальном клиенте под нагрузкой).** Был ~17/с (673/40с): клиент
  шлёт 1 PO/PI на след. кадре, сервер отвечал PI мгновенно →
  tight-loop PO↔PI на RTT. Реф-костыль `Thread.Sleep(200)` пейсил,
  но клиент показывает `text` как пинг (= период петли) → 200мс
  UX-регресс (откачено). **Финальный фикс (`connection.rs`):** PI
  шлёт heartbeat **раз в 400мс** (НЕ на каждый PO) → частота PI =
  частота тика, шторм невозможен; PO лишь меряет РЕАЛЬНЫЙ RTT
  (`now − момент отправки PI`) → в `text` (HUD = настоящие ~50-80мс);
  `num2` = **экстраполированные часы клиента** (`last_pong_ct + мс с
  того PO`) — иначе клиент пишет `FREEZE` при `NowTime−lastPITime
  >1500мс`(`ServerTime.cs:155`); 400мс держит запас под 1500мс при
  джиттере тика;`next_expected` удалён. Деваиация от рефа (клиент
  неизменяем, реф тут говнокод). Эмпирика: **PI 673→54/40с**, payload
  `52:<t>:<54-80> `. Промежуточные версии (800мс / stale num2) давали
  FREEZE — НЕ воспроизводить.`@S` упал 142→55 (НЕ атрибутирую — код
  не трогал; отдельно при необходимости). Память:
  [[ping-tick-refactor-open]].

### Открытые верифицированные 1:1 расхождения (НЕ закрыто — честно)

- [x] **Gun single-target → мульти-таргет ПОРТИРОВАНО 2026-05-30** (не
  верифицировано в живом клиенте). C# `Gun.cs:122-167` бьёт ВСЕХ в радиусе
  20, charge per-hit, без break при обнулении. `combat.rs gun_firing_system`
  переписан: итерация всех игроков, урон/charge per-victim. Charge-формула
  уже была 1:1. Расхождения protector-скип/FX — `CLIENT_PROTOCOL_GAPS §8`.
- [x] **Move dist** — уже 1:1: `movement.rs:108` `dist >= 1.2` (стр.112
  устарела, там было «1.5»); серверный cooldown убран (rubber-band-фикс).
- [x] **Move dir** — уже 1:1: `movement.rs:119-131` считает dir из дельты
  при `dir==-1`/смене позиции (1:1 `Player.cs:416-418`). Стр.115 устарела.
- [ ] **Rubber-band** («назад отбрасывает», репорт юзера) — DEFER (нужен
  живой замер, нельзя автономно). Server move cooldown vs client pace +
  очередь TY. Вероятно уже снят: (1) PO→PI фикс, (2) move-cooldown убран
  (`movement.rs` — нет server-side cooldown в Move). Проверка:
  `tools/repro_freeze.py tp_rollback` на живом сервере. НЕ менять вслепую —
  см. связь с «Единый Delay» ниже (унификация ВЕРНЁТ rubber-band).
- [x] **Auto-dig `dir==-1`** ПОРТИРОВАНО 2026-05-30 (не верифицировано в
  живом клиенте — клиент шлёт `Xmov dir=-1` при автокопе-в-стену; имена
  пакетов RSA-зашифрованы, граф проверить нельзя). `movement.rs`: при
  непустой целевой клетке + `dir==-1` + `auto_dig` → tp назад + `handle_dig`
  в направлении из дельты (1:1 `Player.cs:429-437`). Вызов `handle_dig` —
  ПОСЛЕ закрытия `modify_player` (реентрантность лока). Девиация: C# `Bz()`
  идёт мимо 200ms dig-cooldown, Rust `handle_dig` его применяет (≈ пейс
  ServerPause, безопаснее). Стр.165 («мёртвый код») вводила в заблуждение.
- [ ] **Единый `Delay`** — DEFER, НАМЕРЕННАЯ ДЕВИАЦИЯ (1:1 регрессирует).
  C# `Player.cs:150` ОДИН `Delay`; `TryAct` (214-220) блокирует ВСЕ действия
  одним таймером: dig/build/geo `TryAct(...,200)`, move `TryAct(...,ServerPause)`.
  Rust: раздельные `last_dig`/`last_build`/`last_geo` + move БЕЗ cooldown.
  Унификация 1:1 = добавить move в общий cooldown = ВЕРНУТЬ rubber-band
  (тот самый баг, что чинили — см. выше). Клиент пейсит сам (SpeedPacket).
  Вывод: НЕ унифицировать. Поведение «после dig 200ms нельзя move» —
  жертвуем ради отсутствия rubber-band (клиент важнее реф-говнокода).
- [ ] **HB init-order** — DEFER (риск десинка, нельзя верифицировать без
  клиента). C# `Player.Init` (597-630): `MoveToChunk`(616, только
  spatial-индекс) рано → sync-пакеты (BD/GE/@L/BI/sp/@B/P$/LV/IN) →
  `CheckChunkChanged(true)`(629, HB карты клиенту) **ПОСЛЕДНИМ** → `tp`(630).
  Rust `init.rs:200` зовёт `check_chunk_changed` (spatial+HB вместе) ПЕРВЫМ,
  до sync-пакетов. Для 1:1 надо РАЗДЕЛИТЬ: spatial-регистрация рано + HB
  после `send_inventory`. Реордер Init = классический «1:1-критичный»
  порядок, ломающий клиент при ошибке. Текущий порядок РАБОТАЕТ (сервер
  живой). Менять только с проверкой на живом клиенте.
- [ ] **BotSpot programmator** не реализован — АНАЛИЗ 2026-05-30,
  `docs/BOTSPOT_PROGRAMMATOR.md`. ДВА гэпа: (1) система исполнения для бота
  (большая, additive — обобщить player-coupled `programmator_system`); (2)
  ПРИВЯЗКА программы — БЛОКЕР: `Spot.cs` заглушка (`selected` не
  присваивается, `GUIWin` пуст), 1:1-эталона НЕТ → нужен контракт клиента
  (RSA-имена, анализ `client/`). Не реализовывать вслепую.
- [x] **Chat-навигация `Cmen`/`Choo`/`Cset`/`Cpri` РЕАЛИЗОВАНА** по
  контракту клиента (референс их не обрабатывает — `Session.cs` только
  пустой `Chin`; полная спека — `docs/CLIENT_PROTOCOL_GAPS.md` §3–6).
  `Cmen`→`mL`+`mN` (список каналов: глобал+клан+приваты); `Choo tag`→
  `mO`+`mU` (вход, с валидацией прав); `Cset`→цикл `chat_color` (новая
  колонка+миграция)→`mC`; `Cpri uid`→ЛС `_min_max` (валидация: цель
  есть, не сам с собой; рассылка только участникам). Security-гейт в
  `handle_channel_chat` (клан/приват по актуальному состоянию). Мёртвые
  `handle_chat_init_ty`/`handle_chat_switch`/`send_chat_init` (ловушка
  бага §2) УДАЛЕНЫ. Verified: `tools/chat_probe.py` на live VPS — все 4
  по контракту. Открытая UX-заметка: онлайн-получатель ЛС без открытого
  `Cmen` не получает уведомление (`mN`=0) — не «сломано», отдельный UX.

### Ограничения/безопасность сессии

Без commit/push (Уроки 3/5). Деплой изолирован: проект `openmines`,
volume `openmines_server_data`; `mines3_server_data` НЕ тронут; только
`up -d`/`restart`. `client/` не коммитить. Незакоммиченный рефактор не
откатывался. Замечание масштабируемости: при 12 ботах через 1 ssh-туннель
~5с тишина (broadcast O(N²)/сатурация туннеля, сервер-тик здоров) — НЕ
lock-freeze, юзер играет соло; отдельная заметка.

---

## 1. MVP (сеть и мир)

- [x] TCP tokio 8090, wire-формат 1:1: `[4B length i32 LE (включая эти 4B)][1B data_type ('U'/'B'/'J')][2B event (case-sensitive)][payload...]`
- [x] Жизненный цикл соединения 1:1 (порядок критичен):
  - [x] OnConnected (server→client): `ST` (строка) → `AU` (sid, 5 символов) → `PI` (`"0:0:"`)
  - [x] Auth (client→server): `AU` в одном из форматов: `uniq` / `uniq_NO(AUTH)` / `uniq_userid_token`
  - [x] Auth OK (server→client): **`cf` первым** (WorldInfo JSON) → `Gu` (`"_"`) → `Player.Init()` пакеты (минимум из `docs/PROTOCOL.md`: `BD`, `GE`, `@L`, `BI`, `sp`, `@B`, `P$`, `LV`, `IN`, `HB`, `@T`, `#S`, `cH/cS`, `mO/mU`, `#F`, `@P`)
  - [x] Auth FAIL (server→client): `cf` → `BI (id=-1)` → `HB` → `GU`
- [x] Токен авторизации 1:1 с референсом: **MD5**(`player.hash + sid`), hex lowercase; дополнительно допустим приём SHA256 для других сборок клиента
- [x] Ping/Pong 1:1:
  - [x] `PI` шлётся при подключении и в ответ на `PO` (таймерного `PI` нет)
  - [x] Клиент шлёт `PO` `"response:current_time"`, сервер отвечает `PI` немедленно (референс делает 200ms delay, убран — пинг 50ms вместо 250ms)
  - [x] Клиентский таймаут: ~40.5s без `PI` ⇒ разрыв (сервер должен выдерживать этот контракт)
- [x] World state 1:1: mmap слои `cells/road/durability`, чанки **32x32**, чтение чанка отдаёт 32*32 байт (как в HB `M`)
- [x] TY wrapper 1:1 (client→server): `[4B event][u32 time LE][u32 x LE][u32 y LE][sub_payload...]`
  - [x] `Xmov`: координаты берутся из wrapper `x/y`, `sub_payload` содержит `direction` (текстовое int)
  - [x] Серверная валидация движения: дистанция 1.2, coord validity, cell emptiness, gate blocking, prog guard, direction compute, pack auto-open — 1:1. Отклонение: server-side cooldown убран намеренно (клиент пейсит через SpeedPacket). `dir==-1` ветка в Move() — мёртвый код в C# DigHandler (передаёт свою позицию + реальный dir, не -1)
- [x] HB (server→client, тип `B`, event `HB`) 1:1: подпакеты как в `docs/PROTOCOL.md` (минимум для MVP: `M` карта + `X/L` боты); отправка только клиентам в зоне видимости 1:1
- [x] SQLite минимум для MVP (схема/поля 1:1 с референсом): `players`, `chats`, `buildings`, `boxes` (без этого нельзя считать “регистрацию/авторизацию/инициализацию” завершёнными)
- [x] Сохранение 1:1: periodic flush “грязных” изменений + сохранение при disconnect (включая корректный обработчик закрытия TCP/таймаута ping)
- [x] `cells.json`: **126** типов, загрузка/индексы/дыры 1:1 как в референсе (клиентские ожидания по id типов не ломаем)

## 2. Копание и стройка

- [x] TY-события 1:1 (референс `server_reference/Server/Session.cs`, типовые пакеты в `server_reference/Server/Network/TypicalEvents/*`):
  - [x] `Xdig`: 200ms cooldown, BOX/MilitaryBlock special cases, crystal FX(fx=2), cb accumulator, boulder push every hit, exp on destroy only — 1:1
  - [x] `Xbld`: 200ms cooldown, crystal cost from skill.Effect, AccessGun/PackPart checks, build exp, durability from skill — 1:1
  - [x] `TADG`: toggle авто-копания 1:1
  - [x] `GUI_`: JSON parsing 1:1. Auth-window routing (регистрация/логин GUI state machine) — 1:1 с C#
  - [x] `Xhea`: HP from Repair skill Effect, Repair XP — 1:1
  - [x] `INVN`: toggle инвентаря 1:1
  - [x] `INUS`: все предметы (0-46), Gate(27), Poli(35), geopack mapping с pickup-back — 1:1
  - [x] `INCL`: “_” no-op, id==-1 InvToSend+Close, selection — 1:1
- [x] Кристаллы/корзина 1:1: @B после dig/build/heal/box/death, capacity=1
- [x] Смерть/респавн 1:1: crystal box drop, hb_bot_del broadcast, prog stop, Gu close, resp Y-offset random, HP reset, @T
- [x] Геопаки: Xgeo pick/place stack 1:1, geopack items с correct cell mapping и pickup-back

## 3. Здания

- [x] Pack-система: load из DB, HB 'O' subpacket, auto-open на origin cell. Не реализовано: multi-cell footprint auto-open
- [x] Resp (1): bind, charge decrement + cost deduction, fill GUI (слайдеры +100/+1000/max), admin page (cost/clan toggle/profit), 1:1 с C#. Не реализовано: ClanZone radius persistence
- [x] Teleport (0): GUI со списком TP (distance filter 1000), TP action (offset y+3), charge check — 1:1 с C#
- [x] Up (2): skill tree GUI (upgrade/install/delete/buyslot), 58 skill types, dependencies, total_slots persistence -- 1:1 with C#
- [x] Market (3): buy/sell crystals (prices 1:1), 10% owner commission, admin page, Sell/Buy/Auc tabs -- 1:1 with C#
- [x] Gun (26): ECS firing 1:1 (радиус 20, 60HP, clan immunity), fill GUI, charge-depleted HB resend, building damage — 1:1
- [x] Gate (27): clan blocking работает, GUI=null (не открывается) — 1:1
- [x] Storage (29): двунаправленные слайдеры (deposit + withdrawal), transfer через GUI — 1:1 с C#
- [x] Crafter (24): 8 рецептов, GUI с выбором/стартом/прогрессом/claim, таймер на Unix epoch — 1:1 с C#
- [x] Spot (32): BotSpot ECS entity (skin=3, tail=1, id=-owner_id), HB rendering, owner GUI, lifecycle spawn/despawn — 1:1 с C#. Не реализовано: programmator execution для BotSpot

## 4. Скиллы

- [x] Дерево зависимостей — 1:1 с C# PlayerSkills.skillz (структура совпадает)
- [x] Формулы effect — 1:1 (Health: 100+x*3, Movement, Digging, Packing, MineGeneral, AntiGun, Repair). Exp threshold: flat 1.0
- [x] Up GUI: skill tree (upgrade/install/delete/buyslot), зависимости, 1000 creds per slot — 1:1 с C#
- [x] @S пакет — отправляется при логине и при level-up
- [x] LV пакет — отправляется при логине и при level-up, sum of levels

## 5. Кланы

- [x] Создание (1000 creds), ранги (Member/Officer/Leader), invite/request — всё работает
- [x] Gun/Gate клановые — clan immunity в gun_firing_system, gate blocking в movement
- [x] cS/cH отображение — все transitions (login, create, leave, kick, accept)
- [x] AccessGun территория — формула 1:1, применяется к build/place/bot

## 6. Чат

> Авторитетный источник по чату — `docs/CLIENT_PROTOCOL_GAPS.md`
> (реф неполон: `/////FIX THIS SH`; контракт по клиенту). Прежние `[x]`
> здесь были ЛОЖЬЮ (сверка с неполным референсом, без проверки клиента).

- [x] `mU` wire-формат (`ID±COLOR±CID±TIME±NICK±TEXT±GID`) — probe-verified
- [x] FED/DNO routing (wire-`ch`=реальный канал, не хардкод "FED") —
  probe-verified (FED→ch=FED, DNO→ch=DNO)
- [x] Каналы FED/DNO/CLAN (+ приватные `_min_max`) — probe-verified
- [x] Навигация `Cmen`/`Choo`/`Cset`/`Cpri` (реф НЕ реализует;
  по клиенту, с гейтом прав) — probe-verified
- [x] `Chin`-ресинк (реф `Chin` ПУСТ/неполон — клиент шлёт `getLasts()`):
  login=`mO`-only; `Chin "_"`→полная, `"1:cur:lasts"`→инкремент.
  Снят баг дублей на реконнекте — probe-verified
- [x] Локальный чат HB bubble — Locl → hb_chat broadcast
- [x] Консоль команды — /give, /money, /moneyall, /tp, /heal, /clan, /pack
- [x] История FED/DNO переживает рестарт (грузится из БД в `GameState::new`)
- [ ] **pass-2 ОТКРЫТО** (репорт юзера «мелкие/цвета/кэш плывёт»):
  представление сообщения live≠история (`user_id=0`→`gid=0`→fontSize 10
  - без времени/иконки; `color` 1≠10; `time` сек≠мин). Корень: схема
  `chat_messages` не хранит `player_id`/`color`. Спец — GAPS §1.

## 7. Программатор

- [x] PROG/PDEL/PREN: dispatch → decode binary → save DB → parse LZMA → store ECS → running=true → @P
- [x] Парсер: LZMA decompress + base64 → parse action byte array + labels — 1:1 с C# `Program.parseNormal`
- [x] Пошаговое выполнение: ~90 ActionType (Move/Dig/Build/Rotate/Geo/Heal, cell checks, GoTo/RunSub/RunFunction/If/Return, Macros) — 1:1 с C#
- [x] @P статус: отражает реальное ECS ProgrammatorState.running
- [x] tail в HB bot: queries ProgrammatorState.running (chunks.rs, movement.rs, dig_build.rs)

## 8. Физика мира

- [x] Песок: падение вниз + diagonal slide + boulder fall system
- [x] Бокс (90): SQLite upsert/get/delete + подбор при стоянии (combat.rs)
- [x] Кислота: LivingActiveAcid + CorrosiveActiveAcid, 3s tick, стохастическая коррозия, AcidRock immunity — 1:1 с C#
- [x] Alive-клетки: 7 типов (AliveCyan/Red/Viol/Black/White/Blue/Rainbow), 5s tick, HypnoRock modifier — 1:1 с C# `Physics.cs`
- [x] Лава: fall_damage based — нет специальной физики нужно, 1:1
- [x] Генератор мира: noise skeleton + BFS sectors + palettes 1:1. Не реализовано: spawn area buildings (Market/Resp/Up)

## 9. Предметы

- [x] Инвентарь полный: HashMap items + selected + minv + miniq, INVN/INUS/INCL handlers
- [x] Crafter рецепты: 8 рецептов, GUI, start/timer/progress/claim — 1:1 с C#
- [x] Market buy/sell: цены 1:1, sell/buy, 10% комиссия — 1:1 с C#
- [x] Storage: двунаправленные слайдеры (deposit + withdrawal) — 1:1 с C#
- [x] Бомба, Защита, Разрушитель, C190, Полимер — все 5 работают

---

Приоритет: аудит каждого пункта по референсу, фикс, галка.

---

## 10. Рефакторинг архитектуры

Начинать **только после полного паритета** (фазы 1-9 все ✅. для подтверждения полного паритета должен быть отдельный прогон, в котором ЧИТАЮТСЯ ВСЕ ФАЙЛЫ).

Целевая архитектура: 4 слоя, 3 потока, 0 shared mutable state.

- **Network layer:** N tokio tasks (codec actors) — decode wire → `PlayerCommand`, `GameEvent` → encode wire
- **Game layer:** 1 `std::thread` — владеет ВСЕМ стейтом (ECS World, Map, Buildings). 20 ticks/sec (50ms)
- **Persistence layer:** 1 tokio task — batch writes, game thread никогда не ждёт DB
- **Protocol layer:** чистые функции encode/decode — не меняется

### Этап 0: Чистая логика (~1 сессия)

- [ ] Вынести формулы, валидацию, парсеры в `game/logic/` — функции без зависимости на `GameState`
- [ ] Dig power, crystal calc, skill effects, movement validation — всё как pure `fn`
- [ ] Нулевой риск: только перемещение кода, API не меняется

### Этап 1: Контракты (~1 сессия)

- [ ] Определить `PlayerCommand` enum (замена TY dispatch) — все входящие действия
- [ ] Определить `GameEvent` enum (замена `send_u_packet` + `BroadcastQueue`) — все исходящие
- [ ] Определить `SaveCommand` enum — game → persistence
- [ ] Нулевой риск: только новые файлы, ничего не ломает

### Этап 2: Game thread (~2-3 сессии)

- [ ] `std::thread::spawn` для game loop с `mpsc::Receiver<PlayerCommand>`
- [ ] Сессии шлют команды через `commands_tx` вместо прямых вызовов `GameState`
- [ ] Game thread пока вызывает старые handlers через `Arc<GameState>` (промежуточный шаг)
- [ ] Средний риск: меняется flow, но логика та же

### Этап 3: State → ECS (~3-4 сессии)

- [ ] `active_players: DashMap` → ECS `Query<PlayerComponents>`
- [ ] `chunk_players: DashMap` → ECS `Resource<ChunkIndex>`
- [ ] `building_index: DashMap` → ECS `Resource<BuildingIndex>`
- [ ] `chat_channels: RwLock` → ECS `Resource<ChatState>`
- [ ] По одному DashMap за раз, каждый шаг — компилируемый сервер
- [ ] Высокий риск: каждый шаг ломает внутренний API

### Этап 4: Убить GameState (~2 сессии)

- [ ] Game thread владеет `bevy_ecs::World` напрямую, без Arc/RwLock
- [ ] Убрать `Arc<GameState>` из сессий — только каналы
- [ ] `GameState` struct перестаёт существовать
- [ ] Высокий риск: точка невозврата, но после этого — 0 locks на hot path

### Этап 5: Persistence layer (~1 сессия)

- [ ] Отдельный tokio task получает `SaveCommand` из канала
- [ ] Batch writes: копит `PlayerRow`/`BuildingRow` → одна SQLite транзакция
- [ ] Game thread шлёт `SaveCommand` и не ждёт — fire and forget
- [ ] World flush по таймеру в persistence task
- [ ] Низкий риск: изолированное изменение

### Этап 6: EventBuffer (~2 сессии)

- [ ] `EventBuffer` ECS Resource заменяет `BroadcastQueue` + прямые `send_u_packet`
- [ ] ECS системы пишут в EventBuffer, не шлют пакеты
- [ ] После `schedule.run()` — drain EventBuffer → `broadcast::Sender<GameEvent>`
- [ ] Каждая Session task фильтрует events по chunk/player_id
- [ ] Средний риск: меняется весь output path

### Этап 7: Тесты (~2 сессии)

- [ ] Unit тесты game logic: создать ECS World, заспавнить игрока, применить команду, проверить стейт
- [ ] Integration тесты: mock session шлёт `PlayerCommand`, проверяет `GameEvent`
- [ ] Нет сети, нет DB — чистая логика в тестах
- [ ] Нулевой риск: только новые файлы
