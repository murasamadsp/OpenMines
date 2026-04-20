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

- [ ] TY-события 1:1 (референс `server_reference/Server/Session.cs`, типовые пакеты в `server_reference/Server/Network/TypicalEvents/*`):
  - [ ] `Xdig`: `sub_payload` = direction (UTF-8 int). Поведение 1:1: `TryAct(..., 200)`, внутри: `Move(x, y, dir)` + `Bz()` (эффекты/разрушение/валуны — как в референсе)
  - [ ] `Xbld`: `sub_payload` = `"{direction}{blockType}"` (dir = `data[..^1]`, blockType = `data[^1..]`). Поведение 1:1: `TryAct(() => Build(blockType), 200)`; поддерживаемые blockType — как в клиенте
  - [ ] `TADG`: toggle авто-копания 1:1: `player.autoDig = !player.autoDig`, ответ сервер→клиент `BD` (`"0"`/`"1"`)
  - [ ] `GUI_`: `sub_payload` = JSON `{"b":"button_name"}` (кнопка берётся из поля `b`). Роутинг 1:1: если auth-окно — `auth.CallAction(button)`, иначе `player.CallWinAction(button)` и `SendWindow()`
  - [ ] `Xhea`: лечение 1:1: `player.Heal()`
  - [ ] `INVN`: toggle инвентаря 1:1: `minv = !minv`, затем `SendInventory()`
  - [ ] `INUS`: use item 1:1: `inventory.Use(player)`
  - [ ] `INCL`: выбор слота 1:1: payload `"_"` = служебный пакет без selection; иначе UTF-8 int selection (включая `-1`), поведение выбора как в референсе
- [ ] Кристаллы/корзина 1:1: сервер→клиент `@B` `"G:R:B:V:W:C:Capacity"` (в т.ч. изменения после dig/build/box/use)
- [ ] Смерть/респавн 1:1: `RESP` (TY) вызывает `Death()`; drop кристаллов/позиция/снятие deadlock — строго по референсу
- [ ] “Геопаки” в контексте TY/мира: определить и реализовать строго по референсу (пока не специфицировано в `docs/PROTOCOL.md`, нельзя “галочку” без ссылки на C#)

## 3. Здания

- [ ] Pack-система: load, HB 'O', GU-пакет при входе
- [ ] Resp (1)
- [ ] Teleport (0)
- [ ] Up (2)
- [ ] Market (3)
- [ ] Gun (26) -- ECS система есть, не верифицирована
- [ ] Gate (27)
- [ ] Storage (29)
- [ ] Crafter (24)
- [ ] Spot (32)

## 4. Скиллы

- [ ] Дерево зависимостей (код есть, не верифицирован)
- [ ] Формулы effect/cost/exp (код есть, не верифицирован)
- [ ] Up GUI: прокачка за деньги
- [ ] SK пакет
- [ ] LV пакет

## 5. Кланы

- [ ] Создание, ранги, приглашения
- [ ] Gun/Gate клановые
- [ ] CS/CH отображение
- [ ] AccessGun территория

## 6. Чат

- [ ] FED чат CC/CM
- [ ] Каналы: FED, DNO, клан
- [ ] Переключение Chat/Chin
- [ ] Локальный чат HB bubble
- [ ] Консоль команды
- [ ] История при входе

## 7. Программатор

- [ ] PROG/PDEL/PREN
- [ ] Парсер: move/dig/if/loop/fn
- [ ] Пошаговое выполнение
- [ ] @P статус
- [ ] tail в HB bot

## 8. Физика мира

- [ ] Песок: падение вниз
- [ ] Бокс (90): SQLite + подбор
- [ ] Кислота
- [ ] Alive-клетки
- [ ] Лава
- [ ] Генератор мира

## 9. Предметы

- [ ] Инвентарь полный
- [ ] Crafter рецепты
- [ ] Market buy/sell
- [ ] Storage
- [ ] Бомба, Защита, Разрушитель, C190, Полимер

---

Приоритет: аудит каждого пункта по референсу, фикс, галка.
