# CLAUDE.md

Руководство для Claude Code при работе с этим репозиторием.

## Обязательные ограничения

- **НЕ ВОЗВРАЩАТЬ УДАЛЁННОЕ** — если код/файл был удалён, не восстанавливать без явной просьбы.
- **НЕ ТРОГАТЬ ЛИНТЕРЫ** — не менять настройки clippy/rustfmt, не подавлять warnings.
- **НЕ ОБХОДИТЬ ХУКИ** — никогда не использовать `--no-verify`.
- **ЛОКАЛЬНОСТЬ** — Rust код только в `server/`, C# только в `client/`.
- **НЕ ТРОГАТЬ** `target`, `bin`, `obj`.
- **СПРАШИВАТЬ ПЕРЕД УДАЛЕНИЕМ** — не удалять файлы массово без подтверждения, особенно untracked (невосстановимы).
- Код должен проходить strict clippy и rustfmt перед коммитом.
- Изменения мира должны помечать чанки dirty для flush-цикла.
- При работе с протоколом — всегда сверяться с `server_reference/` (C# референс).

## Lessons Learned

Ошибки фиксируются здесь, чтобы следующая сессия не повторяла их. Если совершил ошибку — **допиши сюда**.

- Записывай конкретно: что сделал, почему неправильно, что делать вместо этого.
- Не записывай очевидности. Только реальные ошибки с последствиями.

### Урок 1: Не удалять файлы без понимания их назначения

Получив задачу "найди AI-мусор", массово удалил 40 файлов CLAUDE.md (бывш. AGENTS.md) через `find -delete`. 22 файла в `client/` были untracked и потеряны безвозвратно. Эти файлы — контекстные описания модулей, а не мусор.

**Правило:** перед массовым удалением — спросить. Перед удалением untracked — предупредить. Не удалять то, что не понимаешь.

## Контекстные CLAUDE.md

В подпапках `server/` лежат локальные CLAUDE.md с описанием конкретных модулей. **Читай их перед работой с модулем** — это быстрее чем разбирать код целиком.

## Документация

- **`docs/PROTOCOL.md`** — полная спецификация сетевого протокола (актуальная)
- **`ROADMAP.md`** — что реализовано и что нет (чекбоксы). Сверяйся перед тем как утверждать что что-то работает.
- **`server_reference/`** — C# исходники оригинального сервера, источник правды по поведению

## Обзор проекта

OpenMines — MMORPG sandbox-майнинг игра. Rust-сервер реализует бинарный TCP-протокол, совместимый с legacy Unity-клиентом (C#). Клиент **неизменяем** — сервер подстраивается под него.

## Build & Run

```bash
cargo build --release
cargo run --release
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery
cargo fmt --all
```

Регенерация мира: `cargo run --release -- --regen` или `M3R_REGEN_WORLD=1`

Конфиг: `config.json` (создаётся автоматически с дефолтами). Порт по умолчанию: 8090.

---

## Сетевой протокол

### Wire-формат

```
[4B length i32 LE (включая эти 4B)] [1B data_type] [2B event_name] [payload...]
```

- `data_type`: `U` (строка/UTF-8), `B` (бинарный/hub), `J` (JSON)
- `event_name`: 2 ASCII-байта, **CASE-SENSITIVE** (`cf` ≠ `CF`, `Gu` ≠ `GU`)
- Минимальный размер пакета: 7 байт

### Жизненный цикл соединения

**1. OnConnected (server→client):**

- `ST` (статус строка) → `AU` (session ID, 5 символов) → `PI` (`"0:0:"`)

**2. Auth (client→server):**

- `AU` пакет: `uniq` / `uniq_NO` / `uniq_userid_token`
- Токен: `MD5(player.hash + sid)` hex lowercase; также принимается SHA256

**3. Auth OK (server→client):**

- `cf` (WorldInfo JSON — **обязан быть первым**) → `Gu` (`"_"`) → Player.Init()

**4. Auth FAIL (server→client):**

- `cf` → `BI (id=-1)` → `HB` → `GU`

**5. Ping/Pong:**

- `PI` шлётся при подключении и в ответ на `PO` (таймерного нет)
- Клиент шлёт `PO` `"response:current_time"`, сервер отвечает `PI`
- Таймаут: >40.5s без `PI` → клиент отключается; сервер: >30s без `PO` → disconnect

### Player.Init() — порядок пакетов (1:1 с C# референсом)

| # | Event | Payload | Описание |
|---|-------|---------|----------|
| 1 | `BD` | `"0"`/`"1"` | Авто-копание |
| 2 | `GE` | строка/`""` | Геолокация (имя региона) |
| 3 | `@L` | `"hp:max_hp"` | Здоровье |
| 4 | `BI` | JSON `{x,y,id,name}` | Инфо о боте |
| 5 | `sp` | `"xy_pause:road_pause:100000"` | Скорость |
| 6 | `@B` | `"G:R:B:V:W:C:Capacity"` | Кристаллы/корзина |
| 7 | `P$` | JSON `{money,creds}` | Деньги |
| 8 | `LV` | число | Уровень |
| 9 | `IN` | `"show:total:selected:k#v#..."` | Инвентарь |
| 10 | `HB` | binary | Чанки карты |
| 11 | `@T` | `"x:y"` | Телепорт на позицию |
| 12 | `#S` | строка | Настройки |
| 13 | `cH`/`cS` | пусто/`"clan_id"` | Клан |
| 14 | `mO`+`mU` | строка+JSON | Текущий чат + история |
| 15 | `#F` | `"oldprogramformat+"` | Конфиг клиента |
| 16 | `@P` | `"0"`/`"1"` | Статус программатора |

### Все пакеты Server→Client

| Event | Payload | Описание |
|-------|---------|----------|
| `ST` | UTF-8 строка | Статус |
| `AU` | 5 символов | Session ID |
| `AH` | `"user_id_hash"` | Auth hash (реконнект) |
| `AE` | строка | Auth error |
| `PI` | `"resp:time:text"` | Ping |
| `cf` | JSON `{width,height,name,v,version}` | WorldInfo |
| `RC` | строка | Reconnect notification |
| `GU` | строка (HORB JSON) | GUI окно |
| `BI` | JSON `{x,y,id,name}` | Bot/Player info |
| `@T` | `"x:y"` | Телепорт |
| `@t` | `"x:y"` | Плавный телепорт |
| `sp` | `"xy_pause:road_pause:depth"` | Скорость |
| `@L` | `"hp:max_hp"` | Здоровье |
| `@S` | `"code:pct#code:pct#..."` | Скиллы |
| `@B` | `"G:R:B:V:W:C:Capacity"` | Кристаллы |
| `NL` | `"id:name,id:name,..."` | Список ников |
| `ON` | `"count:max"` | Онлайн |
| `LV` | число | Уровень |
| `Gu` | `"_"` | Закрыть окно |
| `GR` | строка | Открыть URL |
| `cS` | `"clan_id"` | Показать клан |
| `cH` | пусто | Скрыть клан |
| `$$` | строка | Покупка |
| `P$` | JSON `{money,creds}` | Деньги |
| `PM` | строка | Модули |
| `@P` | `"0"`/`"1"` | Статус программатора |
| `#P` | данные | Открыть программатор |
| `#p` | данные | Обновить программатор |
| `OK` | `"title#message"` | Модальное сообщение |
| `IN` | `"show:total:selected:k#v#..."`/`"close:"` | Инвентарь |
| `BC` | данные | Плохие ячейки |
| `BA` | данные | Агрессия |
| `BD` | `"0"`/`"1"` | Авто-копание |
| `SP` | данные | Панель состояния |
| `GE` | строка (имя региона!) | Геолокация |
| `SU` | данные | Бан-молот |
| `BB` | пусто | Бибика (звук) |
| `@R` | данные | Точка респавна |
| `GO` | данные | Стрелка навигации |
| `DR` | данные | Ежедневная награда |
| `#F` | строка | Клиентский конфиг |
| `#S` | данные | Настройки |
| `MM` | данные | Панель миссий |
| `MP` | данные | Прогресс миссии |

**Чат:**

| Event | Payload | Описание |
|-------|---------|----------|
| `mO` | `"TAG:Name"` | Текущий канал |
| `mL` | `"TAG±notif±title±preview#..."` | Список каналов |
| `mU` | JSON `{ch,h:[...]}` | Сообщения |
| `mN` | число | Уведомления |
| `mC` | данные | Цвет чата |

### HB (Hub Bundle) — бинарный пакет (тип B, event `HB`)

Контейнер подпакетов: `[1B tag][data...]`, склеены последовательно.

| Tag | Описание | Layout (LE) | Размер |
|-----|----------|-------------|--------|
| `M` | Карта | `[w:1][h:1][x:u16][y:u16][cells:w*h]` | 5+w*h |
| `X` | Бот | `[dir:1][skin:1][tail:1][id:u16][x:u16][y:u16][clan:u16]` | 11 |
| `L` | Удалить бота | `[id:u16]` | 2 |
| `S` | Бот покинул блок | `[id:u16][block_pos:i32]` | 6 |
| `O` | Строения | `[block_pos:i32][count:u16][entries...]` | 6+count*8 |
| `F` | Эффект (FX) | `[fx_type:1][x:u16][y:u16]` | 5 |
| `D` | Направл. FX | `[fx:1][dir:1][color:1][x:u16][y:u16][bot_id:u16]` | 9 |
| `C` | Чат-баббл | `[bot_id:u16][x:u16][y:u16][strlen:u16][text:utf8]` | 8+strlen |
| `B` | Список ботов | `[count:u16][bot entries...]` | varies |
| `Z` | Выстрел | `[from_id:u16][to_id:u16][fx:1]` | 5 |

### Client→Server пакеты

| Event | Тип | Описание |
|-------|-----|----------|
| `AU` | U | Авторизация: `"uniq_userid_token"` |
| `PO` | U | Pong: `"response:current_time"` |
| `TY` | B | Обёртка для игровых действий |

### TY-события (внутри TY пакета)

Структура: `[event:4B][time:u32 LE][x:u32 LE][y:u32 LE][sub_payload...]`

| Event | Описание | Sub-payload |
|-------|----------|-------------|
| `Xmov` | Движение | direction (число) |
| `Xdig` | Копание | direction (число) |
| `Xbld` | Строительство | `"{direction}{blockType}"` |
| `Xgeo` | Геология | — |
| `Xhea` | Лечение | — |
| `GUI_` | Кнопка GUI | JSON `{"b":"button_name"}` |
| `Locl` | Локальный чат | `"length:message"` |
| `Chat` | Глобальный чат | сообщение |
| `Chin` | Chat init | `"_"` или `"1:TAG:lasts"` |
| `Whoi` | Запрос ников | `"id,id,id,..."` |
| `TADG` | Авто-копание toggle | — |
| `INCL` | Выбор инвентаря | selection index |
| `INUS` | Использовать предмет | — |
| `INVN` | Toggle инвентарь | — |
| `DPBX` | Открыть ящик | — |
| `Sett` | Настройки | — |
| `ADMN` | Кнопка админа | — |
| `RESP` | Респавн | — |
| `Clan` | Открыть клан | — |
| `Pope` | Открыть программатор | — |
| `Blds` | Мои постройки | — |
| `PROG` | Программатор | данные программы |
| `PDEL` | Удалить программу | ID |
| `pRST` | Перезапуск программы | — |
| `PREN` | Переименовать | ID |
| `TAGR` | Сменить агрессию | — |
| `Miss` | Mission init | `"0"`/`"1"` |

---

## Архитектура Rust-сервера

Единый бинарник `openmines-server`, entry point — `server/main.rs`.

### Ключевые модули (`server/`)

- **`config.rs`** — загрузка `config.json`, `cells.json`, `buildings.json`
- **`world/`** — мир на mmap-слоях (`.mapb`): cells, road, durability. Чанки 32×32. Dirty-tracking + atomic backup
- **`db/`** — SQLite (WAL mode). Таблицы: players, buildings, clans, chats, chat_messages, boxes, programs
- **`game/`** — игровая логика на Bevy ECS
  - `mod.rs` — `GameState` (центральный Arc-объект), ECS-системы, очереди broadcast/programmator
  - `player.rs` — ECS-компоненты игрока (12+: Position, Stats, Inventory, Skills, Cooldowns, Connection и др.)
  - `buildings.rs` — ECS-компоненты зданий, `PackType` enum (15 вариантов)
  - `combat.rs` — `standing_cell_hazard_system`, `gun_firing_system`
  - `sand.rs` — `sand_physics_system` (гравитация песка)
  - `programmator.rs` — `programmator_system` (парсер + исполнение)
  - `skills.rs` — 58 типов навыков
  - `crafting.rs` — 8 рецептов (определены, не подключены к сессии)
- **`net/`** — TCP-сервер + сессии
  - `lifecycle.rs` — фоновые циклы: world flush (60s), player save (10s), building save (45s), game tick (1s)
  - `session/connection.rs` — главный цикл сессии
  - `session/dispatch/ty.rs` — диспетчер TY-событий (25+)
  - `session/auth/` → `play/` → `outbound/` → `social/` → `ui/` → `player/`
- **`protocol/`** — бинарный кодек пакетов
- **`cron.rs`** — планировщик фоновых задач
- **`metrics.rs`** — Prometheus через Axum HTTP
- **`logging.rs`** — tracing с ротацией файлов

### GameState — центральный объект

```
GameState {
    world: Arc<World>                         // mmap-слои мира
    db: Arc<Database>                         // SQLite
    config: Config
    active_players: DashMap<PlayerId, ActivePlayer>    // онлайн игроки
    chunk_players: DashMap<(u32,u32), Vec<PlayerId>>   // пространственный индекс
    building_index: DashMap<(i32,i32), Entity>         // здания по координатам
    chat_channels: RwLock<Vec<ChatChannel>>            // FED, DNO, LOC
    ecs: RwLock<EcsWorld>                              // Bevy ECS
    schedule: RwLock<Schedule>                         // ECS-системы
    auth_failures: DashMap<IpAddr, (u32, Instant)>     // rate limiting
}
```

### Критические паттерны

- **Wire протокол неизменяем** — клиент legacy, менять нельзя.
- **ECS-системы не модифицируют мир напрямую** — используют `BroadcastQueue` и `ProgrammatorQueue` чтобы избежать deadlock.
- **Все здания загружаются в ECS при старте** из SQLite.
- **Dirty flag tracking** — периодические flush-циклы сохраняют помеченных игроков/здания в БД.
- **mmap мир** — zero-copy доступ, изменения через dirty chunk marking.

### Игровая механика

**Движение:** direction 0-3, проверка дистанции ≤1.2, доступ через Gate (клан), чанк-переключение.

**Копание:** 120ms cooldown, удар = `dig_power/500 * dig_mult`, кристаллы умножаются Mining скиллом. XGreen 4x, XBlue 3x, XRed/XViolet/XCyan 2x. Валуны толкаются.

**Строительство:** типы G (Green→Yellow→Red цепочка), R (Road), O (Support), V (Military). Стоимость в кристаллах.

**Бой:** Gun стреляет в радиусе 20 клеток, 60 HP урон (снижается AntiGun скиллом), не стреляет по своему клану и владельцу. Protection item блокирует на 30s.

**Смерть:** кристаллы выпадают как Box (ячейка 90), TP на respawn, reset HP.

**Предметы:** boom (AoE 3x3, 50HP), protector (30s неуязвимость), razryadka (разрядка ганов), C190 (лазер 10 клеток).

**Кристаллы:** 6 типов (Green, Blue, Red, Violet, White, Cyan). Basket с capacity.

**Кланы:** создание (1000 creds), ранги (Member/Officer/Owner), invite/request, Gate/Gun доступ.

**Чат:** FED, DNO глобальные + клановый + локальный (HB bubble). Admin команды: `/give`, `/money`, `/tp`, `/heal`, `/clan`, `/pack`.

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `M3R_DATA_DIR` | Override директории состояния (default: `data/`) |
| `M3R_REGEN_WORLD` | Форсировать регенерацию мира |
| `M3R_GRANT_ADMIN` | Comma-separated имена игроков для роли admin |
| `M3R_USE_CTRL_C` | Ctrl+C для shutdown (false в Docker) |
| `RUST_LOG` / `M3R_LOG` | Override фильтра tracing |

---

## Статус реализации (аудит)

### ✅ Полностью работает

| Компонент | Файлы | Детали |
|-----------|-------|--------|
| Движение | `play/movement.rs` | Валидация, гейты, чанки, broadcast |
| Копание/Стройка | `play/dig_build.rs` | Все типы блоков, скиллы, валуны |
| Аутентификация | `auth/login.rs` | MD5/SHA256 токены, rate limiting |
| Чат | `social/misc.rs` | Глобальный + клановый + локальный, DB, admin cmds |
| Кланы | `social/clans.rs` | CRUD, ранги, invite/request |
| Инвентарь | `ui/heal_inventory.rs` | Все предметы, boom/protector/C190 |
| Бой | `game/combat.rs` | Guns, hazards, fall damage, box pickup |
| Смерть/Респавн | `social/misc.rs` | Crystal box, TP, HP reset |
| Спавн игрока | `player/init.rs` | Порядок пакетов 1:1 с C# |
| Чанки/Видимость | `play/chunks.rs` | HB refresh, deadlock safety |
| Мир/Ячейки | `world/` | Layers, properties, physics |
| БД | `db/` | Полная схема с миграциями |
| Протокол | `protocol/` | Все билдеры и декодеры |
| Синхронизация | `outbound/` | Чат, инвентарь, скиллы, здоровье |

### ⚠️ Частично реализовано

| Компонент | Статус | Что не хватает |
|-----------|--------|----------------|
| Здания | ~70% | Размещение/удаление работает. Не работает: телепорт-меню, маркет, крафтер, хранилище |
| Скиллы | ~60% | Типы + эффекты есть. Нет: прокачка за деньги, GUI, полные формулы exp |
| Настройки | stub | Упрощённый GUI, полный RichList не портирован |
| GUI регистрация | stub | `handle_gui_auth_flow()` — TODO, регистрация через GUI не работает |

### ❌ Не реализовано

| Компонент | Описание | Что нужно сделать |
|-----------|----------|-------------------|
| Программатор | Парсер есть, исполнение заглушено | Подключить execution loop, GUI создания программ, сохранение в БД |
| Крафтинг | 8 рецептов определены | Подключить к зданию Crafter, очередь крафта, GUI |
| Здания — внутренности | PackType enum есть | Телепорт: список + TP. Market: buy/sell. Storage: доп. инвентарь. Crafter: очередь |
| Skill GUI | Нет | Окно прокачки, Up за деньги, формулы cost/exp |
| Пакеты | Определены но не отправляются | `AE`, `RC`, `#P`, `#p`, `BC`, `BA`, `SP`, `SU`, `BB`, `@R`, `GO`, `DR`, `MM`, `MP` |
| Физика мира | Частично | Кислота, alive-клетки, лава — не реализованы |
| Missions | Нет | `Miss` TY-событие — no-op |

---

## Клиент (Unity) — источник правды

Клиент **не может быть изменён**. Ключевые файлы:

| Файл | Назначение |
|------|------------|
| `ServerController.cs` | Главный диспатчер (~45 обработчиков) |
| `ConnectionManager.cs` | TCP соединение, статус, реконнект |
| `AuthManager.cs` | Аутентификация, хранение credentials |
| `ServerTime.cs` | Ping/Pong, синхронизация времени |
| `WorldInitScript.cs` | Инициализация мира по `cf` пакету |
| `Obvyazka.cs` / `Obvyazka3/` | Сетевой фасад, Send/On обёртки |
| `ChatManager.cs` | Обработчики чата (mO/mL/mU/mN/mC) |
| `UnknownClass2.cs` | RSA шифрование имён пакетов |

**Важно:** Имена пакетов в клиенте зашифрованы RSA через `UnknownClass2.smethod_16()`. Расшифрованные имена совпадают с таблицей Server→Client выше.

**Auth на клиенте:** `SHA256(userHash + sessionUnique)`, hash хранится в зашифрованных PlayerPrefs (XOR с device hash).

---

## C# Референс-сервер (`server_reference/`)

Ключевые файлы:

| Файл | Что содержит |
|------|-------------|
| `Server/Session.cs` | Основной обработчик пакетов, TY роутинг |
| `Server/Auth.cs` | Аутентификация, создание аккаунта, MD5 токен |
| `GameShit/Entities/PlayerStaff/Player.cs` | CreatePlayer(), Init(), Move(), Bz(), Death() |
| `GameShit/Entities/PlayerStaff/pSenders.cs` | SendGeo/SendWindow/SendMoney/SendClan и др. |
| `GameShit/Entities/PlayerStaff/Inventory.cs` | Инвентарь, предметы, Use() |
| `GameShit/Entities/PlayerStaff/Basket.cs` | Кристаллы, Mine(), корзина |
| `GameShit/Entities/PlayerStaff/Settings.cs` | Настройки (cc, snd, mus, isca, tsca и др.) |
| `GameShit/Buildings/Pack.cs` | Базовый класс зданий |
| `GameShit/Buildings/Teleport.cs` | Телепорт: список + TP, charge 1000/10000 |
| `GameShit/Buildings/Resp.cs` | Респавн: bind, fill, cost, clan zone |
| `GameShit/Skills/Skill.cs` | Скилл: lvl, exp, Effect, Cost, SkillType |
| `GameShit/Sys_Clan/Clan.cs` | Кланы: создание, ранги, kick |
| `GameShit/GChat/Chat.cs` | Чат: каналы, сообщения, broadcast |
| `GameShit/Programmator/Program.cs` | Программы: парсер, ActionType enum (28+ действий) |
| `GameShit/WorldSystem/World.cs` | Мир: 260×420 чанков, 32×32 ячеек/чанк |

### Ключевые механики из референса (не портированы в Rust)

**Скиллы — полная система:**

- SkillEffectType: OnMove, OnDig, OnDigCrys, OnBld, OnExp, OnHealth, OnHurt, OnRepair
- Up(player): level up если exp ≥ Expiriense
- AddExp(player, amount): накопление опыта
- Cost: стоимость прокачки (кристаллы)

**Программатор — полная система:**

- ActionType enum: 28+ действий (Move, Dig, Rotate, Build, Heal, If, Loop, Call, Return)
- Условия: CheckUp/Down/Left/Right, IsEmpty, IsCrystal, IsFalling
- Операторы: Or, And
- RunProgramm(prog) → programsData.Step() → execute action

**Здания — внутренности:**

- Teleport: GUIWin показывает список TP в радиусе 1000, canvas карта, кнопка TP
- Resp: Fill(crystals→charge), OnRespawn(cost→moneyinside), ClanZone radius
- Market: buy/sell items
- Crafter: recipe queue, completion timer

**Настройки — ключи:**

- `cc` (char size), `snd` (sound), `mus` (music), `isca` (interface scale)
- `tsca` (territory scale), `mous` (mouse), `pot` (graphics), `frc` (force updates)
- `ctrl` (CTRL speed), `mof` (mute nearby)
