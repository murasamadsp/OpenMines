# OpenMines roadmap

Восстановление Mines 3. Клиент: Unity 2019.4.10f1. Сервер: Rust + SQLite + mmap, TCP-binary.
Все пункты верифицируются по C# референсу (`server_reference/`).

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
  - [ ] `GUI_`: JSON parsing 1:1. Auth-window routing — stub (gui_flow.rs TODO). Generic CallWinAction — не портирован
  - [x] `Xhea`: HP from Repair skill Effect, Repair XP — 1:1
  - [x] `INVN`: toggle инвентаря 1:1
  - [x] `INUS`: все предметы (0-46), Gate(27), Poli(35), geopack mapping с pickup-back — 1:1
  - [x] `INCL`: “_” no-op, id==-1 InvToSend+Close, selection — 1:1
- [x] Кристаллы/корзина 1:1: @B после dig/build/heal/box/death, capacity=1
- [x] Смерть/респавн 1:1: crystal box drop, hb_bot_del broadcast, prog stop, Gu close, resp Y-offset random, HP reset, @T
- [x] Геопаки: Xgeo pick/place stack 1:1, geopack items с correct cell mapping и pickup-back

## 3. Здания

- [x] Pack-система: load из DB, HB 'O' subpacket, auto-open на origin cell. Не реализовано: multi-cell footprint auto-open
- [ ] Resp (1): bind работает, charge decrement + cost deduction при респавне работают. Не реализовано: fill GUI (слайдеры +100/+1000/max), ClanZone radius, правильный Resp GUI (сейчас generic)
- [ ] Teleport (0): **не реализован** — нет TP списка, нет TP action, нет GUI
- [ ] Up (2): **не реализован** — нет skill management GUI
- [ ] Market (3): **не реализован** — нет buy/sell/auction
- [x] Gun (26): ECS firing 1:1 (радиус 20, 60HP, clan immunity). Не реализовано: fill GUI, charge-depleted HB resend, building damage
- [x] Gate (27): clan blocking работает, GUI=null (не открывается) — 1:1
- [ ] Storage (29): только withdrawal, нет deposit/slider transfer
- [ ] Crafter (24): рецепты определены, нет execution/timer/GUI
- [ ] Spot (32): placeholder only, нет BotSpot/programmator интеграции

## 4. Скиллы

- [x] Дерево зависимостей — 1:1 с C# PlayerSkills.skillz (структура совпадает)
- [x] Формулы effect — 1:1 (Health: 100+x*3, Movement, Digging, Packing, MineGeneral, AntiGun, Repair). Exp threshold: flat 1.0
- [ ] Up GUI: прокачка за деньги — **не реализован** (нет UpPage, нет install/delete/slot management)
- [x] @S пакет — отправляется при логине и при level-up
- [x] LV пакет — отправляется при логине и при level-up, sum of levels

## 5. Кланы

- [x] Создание (1000 creds), ранги (Member/Officer/Leader), invite/request — всё работает
- [x] Gun/Gate клановые — clan immunity в gun_firing_system, gate blocking в movement
- [x] cS/cH отображение — все transitions (login, create, leave, kick, accept)
- [x] AccessGun территория — формула 1:1, применяется к build/place/bot

## 6. Чат

- [x] FED/DNO каналы — глобальные, работают
- [x] Каналы: FED, DNO, клан (динамический pseudo-channel)
- [x] Переключение Chat/Chin — dispatch + send_chat_init
- [x] Локальный чат HB bubble — Locl → hb_chat broadcast
- [x] Консоль команды — /give, /money, /moneyall, /tp, /heal, /clan, /pack
- [x] История при входе — mO + mU пакеты

## 7. Программатор

- [ ] PROG/PDEL/PREN: dispatch есть, но **не мутирует ECS** — @P отправляется но running никогда не ставится true
- [ ] Парсер: move/dig/if/loop/fn — **реализован но не подключён к сети** (parse_script существует, никогда не вызывается)
- [ ] Пошаговое выполнение — ECS system зарегистрирован, но If/Loop не исполняются (`_ => {}`)
- [ ] @P статус: формат ok, врёт (не отражает реальное ECS состояние)
- [ ] tail в HB bot: **всегда 0** — нигде не проверяется ProgrammatorState.running

## 8. Физика мира

- [x] Песок: падение вниз + diagonal slide + boulder fall system
- [x] Бокс (90): SQLite upsert/get/delete + подбор при стоянии (combat.rs)
- [ ] Кислота: fall_damage есть, нет active acid physics
- [ ] Alive-клетки: constants + geopack placement есть, **нет physics system** (7 типов поведения из C# отсутствуют)
- [x] Лава: fall_damage based — нет специальной физики нужно, 1:1
- [x] Генератор мира: noise skeleton + BFS sectors + palettes 1:1. Не реализовано: spawn area buildings (Market/Resp/Up)

## 9. Предметы

- [x] Инвентарь полный: HashMap items + selected + minv + miniq, INVN/INUS/INCL handlers
- [ ] Crafter рецепты: 8 рецептов определены, **нет execution system/timer**
- [ ] Market buy/sell: **не реализован**
- [ ] Storage: только withdrawal, **нет deposit**
- [x] Бомба, Защита, Разрушитель, C190, Полимер — все 5 работают

---

Приоритет: аудит каждого пункта по референсу, фикс, галка.
