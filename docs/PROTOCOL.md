# Сетевой протокол

> Это внутренний протокол Mines legacy-клиента и **не может меняться на стороне клиента**. Сервер обязан подстроиться под легаси-клиент.
> Имена событий **чувствительны к регистру** (`"CF"` ≠ `"cf"`, `"Gu"` ≠ `"GU"`).

## Базовый wire-формат

Сетевая шина построена вокруг структуры `server/protocol/mod.rs::Packet`:

```text
[4B length LE][1B data_type][2B event_name][payload...]
```

- `length: i32 LE` — общий размер кадра в байтах (включая этот 4-байтный префикс). Минимум 7.
- `data_type: u8` — тип пакета: `U` (string/JSON), `B` (binary/hub), `J` (JSON).
- `event_name: [u8; 2]` — двухсимвольный event (case-sensitive!).
- `payload: Vec<u8>` — данные пакета.

---

## Жизненный цикл подключения

### 1. OnConnected (сразу после TCP-соединения)

Сервер → клиент (референс `Session.OnConnected`):

| # | Event | Payload                        | Описание                   |
|---|-------|--------------------------------|----------------------------|
| 1 | `ST`  | UTF-8 строка                   | Статусное сообщение        |
| 2 | `AU`  | UTF-8 строка (sid, 5 символов) | Session ID для авторизации |
| 3 | `PI`  | `"0:0:"`                       | Начальный ping             |

### 2. Авторизация (клиент → сервер)

Клиент отправляет `AU` пакет. Форматы:

1. `uniq` — серверная авторизация (только имя).
2. `uniq_NO` / `uniq_NOAUTH` — без авторизации.
3. `uniq_userid_token` — стандартная авторизация. В `server_reference/Server/Auth.cs` токен = **MD5**(`player.hash + sid`), hex lowercase, байты строки как в UTF-8. Сервер OpenMines принимает и **MD5**, и **SHA256** (на случай других сборок клиента).

### 3. Ответ на авторизацию (сервер → клиент)

**Успешная авторизация** (референс `Auth.TryToAuth`):

| # | Event           | Описание                                                                                                                                                      |
|---|-----------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------|
| 1 | `cf`            | WorldInfo — JSON с размерами мира. **Обязан быть первым** — клиент в `OnWorldConfig` вызывает `ServerController.Init()`, без которого все обработчики мертвы. |
| 2 | `Gu`            | Закрыть окно авторизации (payload: `_`)                                                                                                                       |
| 3 | `Player.Init()` | Серия init-пакетов (см. ниже)                                                                                                                                 |

**Неудачная авторизация**:

| # | Event | Описание                         |
|---|-------|----------------------------------|
| 1 | `cf`  | WorldInfo                        |
| 2 | `BI`  | BotInfo с `id=-1` (гостевой бот) |
| 3 | `HB`  | Чанк карты для отображения       |
| 4 | `GU`  | GUI окно авторизации             |

### 4. Player.Init() — инициализация игрока

Порядок 1:1 с референсом `Player.cs:597-652`:

| #   | Event     | Тип | Payload                         | Ref метод                    |
|-----|-----------|-----|---------------------------------|------------------------------|
| 1   | `BD`      | U   | `"0"` / `"1"`                   | `SendAutoDigg()`             |
| 2   | `GE`      | U   | имя региона (строка) или `""`   | `SendGeo()`                  |
| 3   | `@L`      | U   | `"hp:max_hp"`                   | `SendHealth()`               |
| 4   | `BI`      | U   | JSON `{"x","y","id","name"}`    | `SendBotInfo()`              |
| 5   | `sp`      | U   | `"xy_pause:road_pause:depth"`   | `SendSpeed()`                |
| 6   | `@B`      | U   | `"G:R:B:V:W:C:Capacity"`        | `SendCrys()`                 |
| 7   | `P$`      | U   | JSON `{"money":M,"creds":C}`    | `SendMoney()`                |
| 8   | `LV`      | U   | число (уровень)                 | `SendLvl()`                  |
| 9   | `IN`      | U   | `"show:total:selected:k#v#..."` | `SendInventory()`            |
| 10  | `HB`      | B   | Binary чанки карты              | `CheckChunkChanged(true)`    |
| 11  | `@T`      | U   | `"x:y"`                         | `tp(x, y)`                   |
| 12  | —         | —   | консольные строки               | (пропущено)                  |
| 13  | `#S`      | U   | настройки                       | `SendSettings()`             |
| 14  | `cH`/`cS` | U   | пустой / `"clan_id"`            | `SendClan()`                 |
| 15  | `mO`      | U   | `"TAG:Name"`                    | `SendChat()` — текущий канал |
| 15b | `mU`      | U   | JSON сообщений                  | `SendChat()` — история       |
| 16  | `#F`      | U   | `"oldprogramformat+"`           | `ConfigPacket`               |
| 17  | `@P`      | U   | `"0"` / `"1"`                   | `ProgStatus()`               |

---

## Все пакеты Server → Client (тип U)

### Plaintext (не зашифрованы в клиенте)

| Event | Описание                   | Payload формат                                                            | Файл клиента              |
|-------|----------------------------|---------------------------------------------------------------------------|---------------------------|
| `ST`  | Статус                     | UTF-8 строка                                                              | `ConnectionManager.cs:52` |
| `AU`  | Session ID                 | UTF-8 строка (5 символов)                                                 | `AuthManager.cs:24`       |
| `AH`  | Auth hash (для реконнекта) | `"user_id_hash"`                                                          | `AuthManager.cs:45`       |
| `AE`  | Auth error                 | строка                                                                    | `AuthManager.cs:23`       |
| `PI`  | Ping                       | `"pong_resp:client_time:text"`                                            | `ServerTime.cs:10`        |
| `cf`  | WorldInfo                  | JSON `{"width","height","name","v","version","update_url","update_desc"}` | `WorldInitScript.cs:39`   |
| `RC`  | Reconnect notification     | строка                                                                    | `ConnectionManager.cs:53` |
| `GU`  | GUI окно (HORB)            | строка (часто `"horb:{json}"`)                                            | `ServerController.cs:256` |

### Зашифрованы в клиенте (RSA через `UnknownClass2.smethod_16`)

| Event | Описание              | Payload формат                               | Ref packet                 |
|-------|-----------------------|----------------------------------------------|----------------------------|
| `BI`  | Bot/Player info       | JSON `{"x","y","id","name"}`                 | `BotInfoPacket`            |
| `@T`  | Телепорт              | `"x:y"`                                      | `TPPacket`                 |
| `@t`  | Плавный телепорт      | `"x:y"`                                      | `SmoothTPPacket`           |
| `sp`  | Скорость              | `"xy_pause:road_pause:depth"` (int, ms)      | `SpeedPacket`              |
| `@L`  | Здоровье              | `"hp:max_hp"`                                | `LivePacket`               |
| `@S`  | Скиллы                | `"code:pct#code:pct#..."` (trailing `#`)     | `SkillsPacket`             |
| `@B`  | Кристаллы/корзина     | `"G:R:B:V:W:C:Capacity"`                     | `BasketPacket`             |
| `NL`  | Список ников          | `"id:name,id:name,..."`                      | `NickListPacket`           |
| `ON`  | Онлайн                | `"count:max"`                                | `OnlinePacket`             |
| `LV`  | Уровень               | число (строка)                               | `LevelPacket`              |
| `Gu`  | Закрыть окно          | `"_"` (1 байт)                               | `GuPacket`                 |
| `GU`  | GUI popup             | строка (HORB JSON)                           | `GUIPacket`                |
| `GR`  | Открыть URL           | строка                                       | `OpenURLPacket`            |
| `cS`  | Показать клан         | `"clan_id"`                                  | `ClanShowPacket`           |
| `cH`  | Скрыть клан           | пустой                                       | `ClanHidePacket`           |
| `$$`  | Покупка               | строка                                       | `PurchasePacket`           |
| `P$`  | Деньги                | JSON `{"money":M,"creds":C}`                 | `MoneyPacket`              |
| `PM`  | Модули                | строка                                       | `ModulesPacket`            |
| `@P`  | Программатор статус   | `"0"` / `"1"`                                | `ProgrammatorPacket`       |
| `#P`  | Открыть программатор  | данные программы                             | `OpenProgrammatorPacket`   |
| `#p`  | Обновить программатор | данные программы                             | `UpdateProgrammatorPacket` |
| `OK`  | Модальное сообщение   | `"title#message"`                            | `OKPacket`                 |
| `IN`  | Инвентарь             | `"show:total:selected:k#v#..."` / `"close:"` | `InventoryPacket`          |
| `BC`  | Плохие ячейки         | данные                                       | `BadCellsPacket`           |
| `BA`  | Агрессия              | данные                                       | `AgressionPacket`          |
| `BD`  | Авто-копание          | `"0"` / `"1"`                                | `AutoDiggPacket`           |
| `SP`  | Панель состояния      | данные                                       | `StatePanelPacket`         |
| `GE`  | Геолокация            | **строка** (имя региона, НЕ координаты!)     | `GeoPacket`                |
| `SU`  | Бан-молот             | данные                                       | `BanHammerPacket`          |
| `BB`  | Бибика (звук)         | пустой                                       | `BibikaPacket`             |
| `@R`  | Точка респавна        | данные                                       | `RespPacket`               |
| `GO`  | Стрелка навигации     | данные                                       | `NaviArrowPacket`          |
| `DR`  | Ежедневная награда    | данные                                       | `DailyRewardPacket`        |
| `#F`  | Клиентский конфиг     | строка (напр. `"oldprogramformat+"`)         | `ConfigPacket`             |
| `#S`  | Настройки             | данные                                       | `SettingsPacket`           |
| `MM`  | Панель миссий         | данные                                       | `MissionPanelPacket`       |
| `MP`  | Прогресс миссии       | данные                                       | `MissionProgressPacket`    |

### Чат (тип U)

| Event | Описание         | Payload формат                                                   | Ref packet               |
|-------|------------------|------------------------------------------------------------------|--------------------------|
| `mO`  | Текущий канал    | `"TAG:Name"`                                                     | `CurrentChatPacket`      |
| `mL`  | Список каналов   | `"TAG±notif±title±preview#..."`                                  | `ChatListPacket`         |
| `mU`  | Сообщения канала | JSON `{"ch":"TAG","h":["±color±clanid±time±nick±text±uid",...]}` | `ChatMessagesPacket`     |
| `mN`  | Уведомления чата | число (строка)                                                   | `ChatNotificationPacket` |
| `mC`  | Цвет чата        | данные                                                           | `ChatColorPacket`        |

---

## Пакет HB (тип B, событие `HB`)

Hub Bundle — бинарный контейнер с подпакетами обновлений мира.

Структура: `[1B tag][sub-packet data...]`, подпакеты склеены последовательно.

### Подпакеты HB

| Tag | Описание         | Layout (LE)                                               | Размер      |
|-----|------------------|-----------------------------------------------------------|-------------|
| `M` | Карта            | `[w:1][h:1][x:u16][y:u16][cells:w*h]`                     | 5 + w*h     |
| `X` | Бот              | `[dir:1][skin:1][tail:1][id:u16][x:u16][y:u16][clan:u16]` | 11          |
| `L` | Удалить бота     | `[id:u16]`                                                | 2           |
| `S` | Бот покинул блок | `[id:u16][block_pos:i32]`                                 | 6           |
| `O` | Строения (packs) | `[block_pos:i32][count:u16][entries...]`                  | 6 + count*8 |
| `F` | Эффект (FX)      | `[fx_type:1][x:u16][y:u16]`                               | 5           |
| `D` | Направленный FX  | `[fx:1][dir:1][color:1][x:u16][y:u16][bot_id:u16]`        | 9           |
| `C` | Чат-баббл        | `[bot_id:u16][x:u16][y:u16][strlen:u16][text:utf8]`       | 8 + strlen  |
| `B` | Список ботов     | `[count:u16][bot entries...]`                             | varies      |
| `Z` | Выстрел          | `[from_id:u16][to_id:u16][fx:1]`                          | 5           |

### HB `O` — layout одной записи (8 байт)

Референс `HBPack.cs` — `sizeof(char)=2` в C#, поэтому `Length = 2 + 2*2 + 2*1 = 8`:

```text
[0]   code    (u8)  — тип строения
[1-2] x       (u16 LE)
[3-4] y       (u16 LE)
[5]   padding (u8, 0) — sizeof(char)=2 в C#, Encode пишет code в [0], X в [1..], оставляя gap
[6]   clan_id (u8)
[7]   off     (u8)
```

Encode пишет clan в `[5]`, но Decode читает из `[6]` — **асимметрия в референсе**. На проводе ориентируемся на **Decode** (то, что читает клиент).

---

## Клиентские пакеты (Client → Server)

### Верхний уровень

| Event | Тип | Описание                           |
|-------|-----|------------------------------------|
| `AU`  | U   | Авторизация: `"uniq_userid_token"` |
| `PO`  | U   | Pong: `"response:current_time"`    |
| `TY`  | B   | Обёртка для игровых действий       |

### TY-события (внутри TY)

Структура TY: `[event:4B][time:u32 LE][x:u32 LE][y:u32 LE][sub_payload...]`

| Event (4B) | Описание                | Sub-payload                |
|------------|-------------------------|----------------------------|
| `Xmov`     | Движение                | direction (число, текст)   |
| `Xdig`     | Копание                 | direction (число, текст)   |
| `Xbld`     | Строительство           | `"{direction}{blockType}"` |
| `GUI_`     | Кнопка GUI              | имя кнопки (UTF-8)         |
| `Locl`     | Локальный чат           | `"length:message"`         |
| `Whoi`     | Запрос ников            | `"id,id,id,..."`           |
| `TADG`     | Авто-копание toggle     | —                          |
| `INCL`     | Выбор инвентаря         | selection index            |
| `INUS`     | Использовать предмет    | —                          |
| `DPBX`     | Открыть ящик            | —                          |
| `Sett`     | Настройки               | —                          |
| `ADMN`     | Кнопка админа           | —                          |
| `RESP`     | Респавн                 | —                          |
| `Clan`     | Открыть клан            | —                          |
| `Pope`     | Открыть GUI             | —                          |
| `PROG`     | Программатор            | данные программы           |
| `PDEL`     | Удалить программу       | ID                         |
| `pRST`     | Перезапуск программы    | —                          |
| `PREN`     | Переименовать программу | ID                         |
| `Chat`     | Глобальный чат          | сообщение                  |
| `INVN`     | Toggle инвентарь        | —                          |
| `Xhea`     | Лечение                 | —                          |
| `Chin`     | Chat init               | `"_"` или `"1:TAG:lasts"`  |
| `Blds`     | Мои постройки           | —                          |
| `TAGR`     | Сменить агрессию        | —                          |
| `Miss`     | Mission init            | `"0"` или `"1"`            |
| `TAUR`     | (зарезервировано)       | —                          |

---

## Heartbeat (Ping/Pong)

1. Сервер шлёт `PI` с `"0:0:"` при подключении.
2. Клиент отвечает `PO` с `"response:current_time"`.
3. Сервер через 200ms отвечает `PI` с `"52:{time+1}:{time-(expected-201)} "`.
4. Клиент отключается если `lastPITime < NowTime() - 40500` (40.5 секунд без PI).

**Важно**: `PI` отправляется ТОЛЬКО при подключении и в ответ на `PO`. НЕ отправляется в `Player.Init()`.

---

## Статус реализации (Rust сервер)

Источник правды по поведению: локальный **`server_reference/`** (C#, `Session`, `Auth`, `Player`, `SettingsPacket`, …).

### Реализовано и работает

Wire и пакеты как у референса: `ST`, `AU`, `PI`, `PO`/`PI` ping, `cf`, `Gu`, `GU`, `BI`, `@T`, `sp`, `@L`, `@S`, `@B`, `NL`, `ON`, `LV`, `P$`, `OK`, `BD`, `IN`, `cS`, `cH`, `#S` (строка как `SettingsPacket` с дефолтным словарём из `Settings.cs`), `mO`/`mU` после логина как `Player.SendChat()` (без обязательных `mL`/`mN` в Init), `#F`, `@P`, `GE`, `HB`, неуспешный `AU` (`cf`→`BI`→`HB`→`GU`).

**TY** (по `Session.cs`): движение/копание/стройка, чат, инвентарь, `RESP`→`Death`, `Clan`, **`Pope`→программатор** (`StaticGUI.OpenGui`), **`Blds`→«мои здания»** (список из SQLite), **`Sett`→окно настроек** (упрощённый GU), **`DPBX`→окно бокса** (кристаллы, без слайдеров), `PROG`/`PDEL`/`pRST`/`PREN`→`@P` как в референсе (без серверного `#p`).

Токен `uniq_userid_token`: **MD5** как в `Auth.CalculateMD5Hash`, плюс приём **SHA256** для других клиентов.

### Не реализовано

Полный порт GUI/БД из референса: RichList настроек, `CrystalSliders` для бокса, программы в БД/`#P`/`#p`, `Miss`/`TAGR`/`TAUR`, `AE`/`RC`, и т.д.

### Шифрование

В **`server_reference/Server/Session.cs`** исходящие пакеты кодируются через `Packet.Encode` и уходят **в открытом виде** — отдельного RSA-слоя на сервере в этом референсе нет. Клиентский RSA из таблицы выше — сторона Unity-клиента при приёме.

### Известные баги

- **`GE`**: нет карты «регион → имя» в `World`; до появления стека гео шлётся пустая строка (не координаты `x:y`).
