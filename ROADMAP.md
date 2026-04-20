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
- [x] Up (2): skill tree GUI (upgrade/install/delete/buyslot), 58 типов скиллов, зав��симости, total_slots persistence — 1:1 с C#
- [x] Market (3): buy/sell кристаллов (цены 1:1), 10% комиссия владе��ьцу, admin page, Sell/Buy/Auc tabs — 1:1 с C#
- [x] Gun (26): ECS firing 1:1 (радиус 20, 60HP, clan immunity). Не реализовано: fill GUI, charge-depleted HB resend, building damage
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

- [x] FED/DNO каналы — глобальные, работают
- [x] Каналы: FED, DNO, клан (динамический pseudo-channel)
- [x] Переключение Chat/Chin — dispatch + send_chat_init
- [x] Локальный чат HB bubble — Locl → hb_chat broadcast
- [x] Консоль команды — /give, /money, /moneyall, /tp, /heal, /clan, /pack
- [x] История при входе — mO + mU пакеты

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
