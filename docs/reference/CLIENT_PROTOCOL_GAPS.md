# Расхождения клиент ↔ референс (что есть в клиенте, но не/неверно в `docs/reference/server_reference/`)

> **Зачем этот документ.** `docs/reference/server_reference/` — НЕПОЛНАЯ реконструкция
> оригинального C#-сервера. Для ряда фич он либо вообще не реализует логику
> (только структуры пакетов, с литеральным комментарием `/////FIX THIS SH`
> в `Server/Session.cs`), либо реализует её НЕВЕРНО относительно реального
> Unity-клиента. Клиент (`client/`) **неизменяем** и потому является
> **единственным источником правды** по wire-протоколу и ожидаемому
> поведению (CLAUDE.md: «Клиент неизменяем — сервер подстраивается под
> него»). Здесь подробно зафиксировано КАЖДОЕ такое место: что шлёт/ждёт
> клиент (с `file:line`), что делает (или не делает) референс, точный
> wire-формат и реализованное поведение сервера.
>
> Прошлый удалённый audit-файл и `ROADMAP.md` ставили ✅ сверяясь с неполным
> референсом и НЕ проверяя клиент — отсюда «много чего помечено сделано,
> но не работает». Этот файл — противоядие: правится по мере проверки
> каждой фичи **против клиента**, а не против референса.
>
> В строках evidence короткий путь `server_reference/...` означает локальный
> `docs/reference/server_reference/...`.

Статусы: ✅ реализовано+верифицировано · 🔧 реализовано, ждёт реал-теста ·
❌ не реализовано (известная дыра).

---

## 1. `mU` — поле `id` в `GCMessage` (КОРЕНЬ «FED-чат не работает») — ✅

**Клиент** (`client/Assets/Scripts/ChatManager.cs` `muHandler`, ~стр. 339–387;
структура `client/Assets/Scripts/GCMessage.cs`):

```csharp
string[] array = MuPacket.h[i].Split('±');
if (array.Length == 7) {
    GCMessage.id    = int.Parse(array[0]);  // ← поле id ОБЯЗАТЕЛЬНО первым
    GCMessage.color = int.Parse(array[1]);
    GCMessage.cid   = int.Parse(array[2]);
    GCMessage.time  = int.Parse(array[3]);
    GCMessage.nick  =           array[4];
    GCMessage.text  =           array[5];
    GCMessage.gid   = int.Parse(array[6]);
}
// дедуп истории: if (GCMessage.id > this.LastIDs[ch]) History[ch].Add(...)
```

**Референс** (`server_reference/.../Chat/GCMessage.cs`): `Encode` =
`±{Color}±{ClanId}±{Time}±{Nick}±{Text}±{UserId}` — ведущий `±`, **нет
поля `id`**, 6 значимых полей. Split('±') → `["",Color,...]` →
`int.Parse(array[0])` = `int.Parse("")` → **`FormatException` в Unity** →
НИ ОДНО сообщение чата не отображалось.

**Wire (правильный, по клиенту):**
`{"ch":"TAG","h":["ID±COLOR±CID±TIME±NICK±TEXT±GID", ...]}` — каждый
элемент ровно 7 `±`-частей, `ID` первым (целое, int32).

**Сервер:** `protocol/packets.rs::chat_messages` шлёт
`ID±COLOR±CID±TIME±NICK±TEXT±GID`. `ChatMessage.id` = rowid строки
`chat_messages` (монотонный → дедуп клиента `LastIDs` работает). FED/DNO
история грузится из БД в `GameState::new` (переживает рестарт;
референсный EF-навигатор `Chat.messages` не портирован — Rust держит
in-mem `ChatChannel.messages`). Юнит-тест контракта:
`protocol::packets::tests::chat_messages_matches_client_gcmessage_parse_contract`.

> **pass 2 (репорт юзера 2026-05-17, real-client) — ✅ PROBE-VERIFIED
> (live VPS, 2026-05-17).** Прежняя формулировка «color 10 live / 1
> история (1:1 `Chat.cs:44`)» была НЕВЕРНА — это и есть баг (реф
> `GetMessages` color=1, `Chat.cs:44` live color=10 → один message
> разного цвета). Канон — `server_reference` `GLine` (`playerid` —
> реальная EF-колонка; `player`/`time` `[NotMapped]`,
> `time=(int)(DateTime.Now.Ticks/10000L/60000L)`). По клиенту
> (источник правды) реализовано:
> - ✅ **`color`**: `ChatManager.colorFromCode` =
>   `HSVToRGB(code/20,…)` (`code 50` = системный серый, спец-кейс
>   `ChatManager.cs:264`), `AddLine`/`AddMiniLine` красят строку
>   `colorFromCode(message.color)`. Реализация: `chat_messages.color`
>   (миграция) = **снимок `players.chat_color` автора на момент
>   отправки** (`add_chat_message` резолвит и возвращает `(id,color)`;
>   `player_id<=0` → 50). live-рассылка, in-mem-копия и БД-история
>   несут ОДИН `color`. **Snapshot-семантика (НЕ баг, не «чинить»):**
>   игрок меняет цвет ПОСЛЕ отправки → старые сообщения сохраняют
>   цвет на момент отправки (как и должно: история = что было
>   показано live); новые берут новый цвет.
> - ✅ **`time`**: `ChatLineInfo.cs:17` `(long)time*60000L*10000L`
>   тиков → `new DateTime(...)` — клиент ждёт **минуты с .NET-эпохи**
>   (НЕ unix-минуты). Был рассинхрон: live `as_secs()` (unix-сек),
>   история `ts/60` (unix-мин) → разное время одного сообщения.
>   Реализация: единый `game::chat::dotnet_epoch_minutes(unix_secs)`
>   = `(621_355_968_000_000_000 + s*10_000_000)/10_000/60_000`
>   (двухшаговое усечение 1:1 `GLine.time`), вызывается И в live, И в
>   истории (startup `game/mod.rs`, `load_db_history`). Юнит-тесты:
>   `game::chat::tests` (epoch, +1мин, i32-range).
> - ✅ **`gid`/`cid`**: `ChatLineInfo.cs:15,25` время/id ТОЛЬКО при
>   `gid>0`; `:54 if(gid<=0) fontSize=10` → мелкий; `:43 cid` →
>   клан-иконка `ClanSpriteScript.sprites[cid-1]`. Реализация:
>   `chat_messages.player_id` (миграция; = C# `GLine.playerid`),
>   `get_recent_chat_messages` `LEFT JOIN players` → `user_id`
>   (=`gid`) = `player_id`, `cid` = текущий клан автора (динамический,
>   1:1 C# `line.player.cid`). live берёт `clan_id` из ECS (снимок).
>   **Легаси-строки** (отправлены ДО миграции, `player_id=0` →
>   `gid=0` → клиент `fontSize=10`, репорт юзера «всё равно мелкий
>   шрифт», probe показал 18/22 FED-истории) — БЭКФИЛЛ-миграция:
>   `UPDATE chat_messages SET player_id=(SELECT id FROM players
>   WHERE name=player_name) WHERE player_id=0` (`players.name`
>   UNIQUE; идемпотентно). Прод-лог: `backfilled … 23 legacy rows`;
>   probe после: **26/26 `gid>0`, 0 мелких**. Легаси `color`
>   остаётся 10 (снимок утрачен; мелкость = `gid`, не `color` —
>   10 не мелкий). Автор удалён → строка остаётся `gid=0`
>   (истинно невосстановимо).
>
> **Мигание ползунка чата — ЧИСТО КЛИЕНТСКИЙ Unity-баг, НЕ сервер
> (закрыто, deferred 2026-05-17).** РЕШАЮЩАЯ улика: юзер **выключил
> интернет (нет связи с сервером вообще) — мигание ОСТАЛОСЬ.** Значит
> ни один серверный пакет ни при чём (PI/@S/@B/mO/mU/реконнект — всё
> эмпирически исключено, в т.ч. 40с-гистограмма: 0 mO/mU в игре).
> Корень: `Assets/Scenes/m1client.unity` чат-`ScrollRect`
> `m_VerticalScrollbarVisibility: 2` = Unity **AutoHideAndExpandViewport**
> — петля (полоса→viewport↓→контент влез→полоса прячется→viewport↑→…),
> срабатывает на любом layout-проходе без пакетов. Пре-существующий
> конфиг сцены; серверного wire-фикса нет без клиентской layout-правки.
> Решение юзера: **забить** (косметика; «пишем сервер»). Тупиковые версии в
> истории этого файла (mixed-heights/PI-шторм/реконнект) — ОПРОВЕРГНУТЫ
> тестом «выключил интернет»; не реанимировать.
>
> **PROBE-РЕЗУЛЬТАТ (live VPS, 2026-05-17, `tools/chat_probe_pass2.py`,
> bot id=2).** Деплой `deploy-vps.sh` → лог подтвердил миграцию прод-БД
> (`added chat_messages.player_id column`, `…color column`). Фаза
> **send**: FED-сообщение `id=24` → live `mU` = `gid=2`(>0), `color=6`
> (снимок), `cid=0`, `time=1065244006` → `2026-05-17 18:46 UTC`
> (верная единица); `Chin "_"` (in-mem) — те же поля → **live ==
> in-mem история** (half-a: рассинхрон color/time устранён). Фаза
> **verify**: `up -d --force-recreate` (in-mem очищен, FED перегружен
> ИЗ БД) → `Chin "_"` → `id=24` всё ещё `gid=2`(НЕ 0!), `color=6`,
> `time` тот же = **БД-история после рестарта == live-эталон**
> (half-b: «старые мелкие после рестарта» закрыт; `player_id`/`color`
> персистнули). Оба корня ✅ на реальном wire-протоколе.
> - ✅ **`Chat.cs:44` хардкод `ch="FED"` для global** (DNO — реальный
>   канал, `World.cs:74-75`) → DNO не виден, тёк в FED. ИСПРАВЛЕНО
>   (pass 1): wire-`ch` = реальный `channel_tag`. Probe-verified
>   (FED→ch=FED, DNO→ch=DNO). Референс-баг; клиент важнее.
> - ⚠️ **Дубли на реконнекте**: старая фиксация “login sends only `mO`” устарела.
>   Текущий `player/init.rs` шлёт `mO` + bounded `mU`; `Chin` остаётся
>   ресинком по `getLasts()` (§2). Любое утверждение про отсутствие дублей
>   требует повторной live-проверки именно текущего init-порядка.

---

## 2. `Chin` — механизм РЕСИНКА чата (реф неполон) — ✅

**Клиент** шлёт `Chin` на world-init
(`client/Assets/Scripts/WorldInitScript.cs:101,109`):

```csharp
SendTypicalMessage(-1, "Chin", 0, 0, "_");                       // первый вход
SendTypicalMessage(-1, "Chin", 0, 0, "1:" + getCurrentChat()     // реконнект
                    + ":" + getLasts());                          // lasts = TAG#lastid#...
```

`getLasts()` = `TAG#lastid#TAG#lastid…` (наибольший виденный `id` по
каждому каналу). На реконнекте клиент **НЕ сбрасывает** `History`/
`LastIDs` (поля инстанса `ChatManager`; `WorldInitScript` ветка
«уже inited» НЕ перестраивает сцену). Т.е. `Chin "1:cur:lasts"` —
это контракт инкрементального ресинка: «у меня есть вот эти id, пришли
только новее».

**Референс** (`server_reference/Server/Session.cs:132-135`): `Chin()` —
**пустое тело**. Реф НЕ реализовал ресинк → реф неполон (клиент шлёт
`lasts`, реф его не использует). Это НЕ «1:1 = no-op» — это дыра.

**Почему неконтролируемая полная пере-отправка ломается:** клиент `muHandler` вызывает
`AddLine` (визуальный рендер) для КАЖДОГО сообщения пакета при
`MuPacket.ch == currentChat`, ДО и НЕЗАВИСИМО от дедупа `id >
LastIDs[ch]` (тот дедупит лишь словарь `History`, НЕ визуал). На
исторически login слал полную историю `mU` → `muHandler` `AddLine`'ил
всё повторно поверх уже отрендеренной (через `moHandler`) History →
**визуальные дубли** (репорт юзера «сообщения дублируются, прошлые
не затёрлись»). Прежняя итерация (Rust `handle_chat_init_ty` →
`send_chat_init` → слал `mL`) ломала вход в чат (`mlHandler` гасил
ChatInput, «СПИСОК ЧАТОВ» поверх FED); фикс «`Chin` = no-op» убрал
ту поломку, но оставил дубли латентными (login всё ещё слал полную
историю).

**Сервер (текущая реализация):**
- `init_player` шлёт `mO` + bounded `mU` текущего канала. Это делает первый
  вход независимым от timing-а `Chin`; риск дублей на реконнекте должен
  проверяться живым клиентом именно на текущем init-порядке.
- `handle_chat_resync` (TY `Chin`, `social/chat.rs`):
  - `"_"` (первый вход, History клиента пуста) → полная история
    текущего канала (`mU`). `mO` уже от login.
  - `"1:cur:lasts"` (реконнект) → `current_chat=cur`, `mO` + `mU`
    ТОЛЬКО с `id > lastid[cur]` (нет в `lasts` → −1 → полная).
    Доступ к `cur` гейтится `chat_access` (нет прав, напр. `CLAN`
    после выхода из клана — drop, без `mU`).
  - НИКОГДА не слать `mL` (ломает вход — см. выше).
- Пустой `mU` (`{"ch":"cur","h":[]}`) безопасен: `muHandler` цикл
  пуст, `AddLine` не зовётся, дублей нет.

**Замечание:** уже задублированные в клиенте сообщения из прошлого
сломанного состояния не дедупятся ретроактивно (нет пакета «очистить
History») — уйдут после полного рестарта клиента (перезагрузка сцены).

---

## 3. `Cmen` — открыть список чатов → `mL` — ✅ verified (probe)

**Клиент** (`ChatManager.cs:67-72` `OnMenu`, кнопка «список чатов»):
`SendTypicalMessage(time, "Cmen", 0, 0, "_")`. Ждёт `mL`
(`ChatListPacket`), клиент `mlHandler` рисует «СПИСОК ЧАТОВ» с кнопками
каналов; клик по каналу → `Choo` (см. §4).

**Референс:** `CmenPacket.Decode` требует payload ровно `"_"`
(`TypicalEvents/CmenPacket.cs`). Логики обработки **нет** — `Session.TY`
не имеет `case CmenPacket` (`Session.cs:86-119`, `default: //Invalid`).

**Wire `mL` (`ChatListPacket`, `GCChatEntry`):** записи через `#`, каждая
запись = `TAG±NOTIF±TITLE±NICK: TEXT` (ровно 4 `±`-части; `NOTIF`∈{0,1};
4-я часть — превью «Ник: Текст»). Источник: `Chat/GCChatEntry.cs`
`Encode => "{Tag}±{Notif?1:0}±{Title}±{Nickname}: {Text}"`; клиент
`mlHandler` берёт `subs[0..3]`, для `_`-тегов (ЛС) обрезает превью до
первого `:`.

**Сервер:** `Cmen` → `send_channel_list` шлёт `mL` со списком: FED, DNO
(глобальные) + `CLAN` (если `player.clan_id.is_some()`) + активные
приватные каналы игрока (DB: `chat_messages` с tag вида `_<id>_<id>`,
где участвует наш `id`). `mO`/`mU` НЕ слать (конфликт с входом в канал).

---

## 4. `Choo <tag>` — войти/переключить канал → `mO`+`mU` — ✅ verified (probe)

**Клиент** (`ChatManager.cs:173-177`, кнопка канала в списке):
`SendTypicalMessage(time, "Choo", 0, 0, subs[0])`, где `subs[0]` — тег
канала из `mL`. Ждёт `mO` (`CurrentChatPacket` `TAG:Name`) → `moHandler`
активирует `ChatInput`, ставит `currentChat=tag`, входит в чат; затем
`mU` — история.

**Референс:** `ChooPacket.Decode` = UTF-8 строка `tag`
(`TypicalEvents/ChooPacket.cs`). Логики **нет** (нет `case ChooPacket`).

**⚠ Граница безопасности.** Клиент может прислать ЛЮБОЙ tag (`Choo
"CLAN"`, `Choo "_1_999"`) независимо от прав. Сервер ОБЯЗАН валидировать:
- глобальные (`FED`/`DNO`/`LOC`) — всегда можно;
- `CLAN` — только если `player.clan_id.is_some()`;
- `_a_b` (приват) — только если `player.id ∈ {a, b}`;
- иначе — drop + `warn!`, без `mO`.

**Сервер:** `Choo` → валидация тега (выше) → `send_enter_channel`: ставит
`ui.current_chat = tag`, шлёт ТОЛЬКО `mO` + `mU` (история канала). `mL`
НЕ слать.

---

## 5. `Cset` / `mC` — цвет чата — ✅ verified (probe)

**Клиент** (`ChatManager.cs:60-65` `OnSettings`, кнопка настроек чата):
`SendTypicalMessage(time, "Cset", 0, 0, "_")`. Обработчик ответа —
`mcHandler` (`ChatManager.cs:333-337`):

```csharp
int num = (int)short.Parse(msg);
ChatInput...color = Color.HSVToRGB(num/20f, 0.3f, (num%2==0)?1f:0.86f);
```

**Референс:** `CsetPacket.Decode` требует `"_"`
(`TypicalEvents/CsetPacket.cs`); `ChatColorPacket` (mC) =
`short Color`, валид `[0,20)` (`Chat/ChatColorPacket.cs`). Логики **нет**.
`GlobalChatManager` (упоминался в старых грепах) в клиенте **отсутствует**.

**Wire `mC`:** строка-число `Color` (short, `0..=19`).

**Сервер:** `Cset` → циклический инкремент `player.chat_color =
(c + 1) % 20`, персист в БД (новая колонка `players.chat_color INTEGER
NOT NULL DEFAULT 0`, миграция в стиле `db/mod.rs::migrate` через
`pragma_table_info`-guard), эхо `mC <new>`. Косметика (тинт поля ввода).

---

## 6. `Cpri <userId>` — приватный чат (ЛС) — ✅ verified (probe)

**Клиент** (`ChatManager.cs:304-308`, клик по строке любого сообщения):
`SendTypicalMessage(time, "Cpri", 0, 0, message.gid.ToString())` —
`gid` = userId автора сообщения. Открыть ЛС с этим игроком. Ответ —
как `Choo`: `mO` (`_tag:OtherName`) + `mU`. `moHandler`
(`ChatManager.cs:187-194`): `if (tag.StartsWith("_")) TitleTF =
"ЛС – " + name` — **тег приватного канала ОБЯЗАН начинаться с `_`**, а
`Name` в `mO` — ник СОБЕСЕДНИКА (не тег).

**Референс:** `CpriPacket.Decode` = `int UserId`
(`TypicalEvents/CpriPacket.cs`). Логики/модели приватных каналов **нет
вообще** — новая подсистема, проектируется по клиенту.

**Модель приватного канала (новое, нет в референсе):**
- Тег: `_{min(me,uid)}_{max(me,uid)}` — стабилен для пары независимо
  от инициатора.
- Имя в `mO` — ник собеседника (`db.get_player(uid)` / `active_players`).
- Персистенция: те же `chat_messages` с `chat_tag` = приватный тег.
- История: грузится из БД по требованию (как `CLAN`).
- **Рассылка:** сообщение в `_a_b` шлётся ТОЛЬКО онлайн-участникам
  `{a,b}` (НЕ всем активным игрокам — это утечка ЛС). Хелпер
  `send_to_users(state, &[a,b], pkt)`.
- **Безопасность:** `Cpri uid`: `uid` должен существовать в `players`;
  `uid != self` (нет ЛС с собой); тег материализуется только для
  валидного `uid`. Отправка в `_a_b` через `handle_channel_chat`
  гейтится тем же `me ∈ {a,b}` (иначе forge через подменённый
  `current_chat`).

---

## 7. Прочие клиентские события

`server_reference/.../TYPacket.cs` их ДЕКОДИРУЕТ, но `Session.TY` НЕ
обрабатывает (`default: //Invalid`). Сервер молча игнорит. Разобрано
по клиенту 2026-05-17; классификация по рабочему методу (есть в
референсе → 1:1; нет → доку + сам):

### 7.1 `THID` — прогресс/скрытие туториала — ✅ known no-op

Клиент `client/.../TutorialNavigation.cs:132` `CheckHide(marker)` и `:141`
(таймаут) → `SendTypicalMessage(-1,"THID",0,0, marker | "TIMEOVER")`.
**Клиент НЕ ждёт ответа** — сразу вызывает `this.hide()` локально.
Реф: `THIDPacket(string Marker)`, обработки нет. Контракт: сервер
принимает и (опц.) персистит прогресс туториала; ответа НЕ слать.
Минимум 1:1-корректно = явный no-op arm в dispatch (не молчаливый
fallthrough). Низкий приоритет, поведение клиента не меняется.
Статус Rust: заведён явный known no-op/debug-log.

### 7.2 `PCOP <programId>` — копия программы программатора — ✅

Клиент `client/.../ProgrammatorManager.cs:106` `OnCopyProgramm` →
`SendTypicalMessage(-1,"PCOP",0,0, programId)`. UI: «создать копию
программы, появится в общем списке». Реф: `PCOPPacket(int Id)`,
обработки НЕТ (ни `Session`, ни `Program.cs`). Контракт: сервер
дублирует строку программы (новый id, имя «… (копия)»), затем
рефреш списка программ клиенту (как открытие программатора). Привязка
к существующей Rust-инфре `db/programs.rs` + программаторным пакетам.
Статус Rust: реализовано. `PCOP` добавлен в dispatch, копирует только
owned-программу и затем обновляет список программатора.

### 7.3 `GDon <method>` / `Help "_"` — донат / помощь — ✅/product

Клиент `client/.../GUIManager.cs:162` (`GDon`, payload =
`ConnectionManager.METHOD`) и `:117` (`Help "_"`). Реф:
`GDonPacket(string Method)`, `HelpPacket` — обработки нет. Вероятный
контракт — ответ `GR` (открыть URL) или `OK`/`GU` окно с внешней
ссылкой (донат-платёж / справка). **URL зависят от среды и могут быть
неактуальны для приватного воскрешения сервера.** Нужно решение юзера:
нужен ли донат/помощь вообще и какие URL — это конфиг/продукт, не
чистый порт. Текущий Rust-контракт: `GDon` перепрофилирован в ежедневный
бонус; `Help` возвращает `OK`, чтобы кнопка не была молчаливой.

### 7.4 `Miso "0"` — миссии — ⚠ минимальный UI-контракт есть, full port открыт

Клиент `client/.../MissionPad.cs:12` → `Miso "0"`. **В отличие от
остальных, в референсе ЕСТЬ подсистема миссий**:
`server_reference/GameShit/Sys_Miss/*`, `Network/Tutorial/
MissionPanelPacket.cs` (`MM`), `MissionProgressPacket.cs` (`MP`),
`NaviArrowPacket.cs` (`GO`). По рабочему методу это «ЕСТЬ в референсе
→ портировать 1:1», а не доку-дыра. TY-входы подсистемы: `Miss`
(сейчас dispatch → пустой no-op!), `Miso`, `Rndm`, `TAUR`.
Текущий Rust-контракт: `Miso` шлёт `MM` с пустым text, что штатно скрывает
mission panel в клиенте. Отдельная крупная задача 1:1-порта `Sys_Miss` остаётся
открытой (НЕ реверс по клиенту).

### Сводка

| Событие | Триггер клиента (file:line) | Payload | Класс | Статус |
|---|---|---|---|---|
| `THID` | TutorialNavigation.cs:132/141 | marker/"TIMEOVER" | fire-and-forget | ✅ known no-op/log |
| `PCOP` | ProgrammatorManager.cs:106 | programId | доку+сам | ✅ copy + refresh list |
| `GDon` | GUIManager.cs:162 | METHOD | product | ✅ daily bonus |
| `Help` | GUIManager.cs:117 | "_" | product | ✅ explicit OK |
| `Miso` | MissionPad.cs:12 | "0" | **1:1 порт Sys_Miss** | ⚠ minimal hide; full port open |

(Дифф «шлёт клиент vs диспатчит сервер»:
`grep -rhoE 'SendTypicalMessage\([^,]+,\s*"[^"]+"' client/Assets/Scripts/*.cs`
против `match`-веток `crates/openmines-server/src/net/session/dispatch/ty.rs`.)

---

## 8. Combat — Gun (`gun_firing_system` ↔ `Gun.Update`)

Источник правды — `server_reference/GameShit/Buildings/Gun.cs:122-167` (боевое
поведение, не протокол-с-клиентом).

- **Мульти-таргет — ✅ портировано 2026-05-30 (pending live verify).** C#
  `Gun.Update` в `foreach` бьёт КАЖДОГО игрока в радиусе 20, списывая charge
  per-hit, и НЕ прерывает цикл при обнулении charge (оставшиеся жертвы всё равно
  получают урон в этот тик). Rust бил одного (`break` после первого). Исправлено:
  итерация всех игроков, урон/charge per-victim, без `break`; top-guard
  `charge <= 0` лишь пропускает пушку на следующем тике. Charge-формула
  (`0.5 * Induction.Effect/100`) уже была 1:1 (`skill_effect(Induction,0)=100` →
  0.5 для игрока без Induction, как C# default).
- **Protector-скип — ⚠ девиация Rust сверх референса.** `gun_firing_system`
  пропускает игрока с активным `protection_until`. В C# `Player.Hurt` И
  `Gun.Update` проверки неуязвимости НЕТ (`Player.cs:827-869` — только
  Health/Induction/AntiGun exp + AntiGun-снижение урона). Protector-механика в
  C# идёт иным путём (не найден в Hurt). Оставлено как есть — соответствует
  документированному «protector = 30s неуязвимость»; помечено для ревью.
- **FX — ⚠ форма пакета отличается (pending).** C# шлёт per-victim направленный
  FX `SendDFToBots(7, x, y, player.id, 1)` (подпакет `D`: тип 7, от пушки к
  боту). Rust шлёт позиционный `hb_fx` (подпакет `F`) у пушки. Сейчас FX
  внутри per-victim цикла, но shape другой (нет направления/bot_id). Точный 1:1
  требует `D`-подпакета с `player.bot_id` — отложено.

Клиент НЕ доверенный. Любой `Choo`/`Cpri`/`Chat` может нести подделанный
tag/uid. Инварианты, проверяемые на сервере ДО смены `current_chat` и ДО
чтения/записи истории:

1. `Choo tag`: tag ∈ разрешённых для игрока (глобал | CLAN-если-в-клане |
   `_a_b`-если-участник). Иначе drop+warn.
2. `Cpri uid`: `uid` существует, `uid != self`.
3. `handle_channel_chat`: запись в `CLAN`/`_a_b` гейтится членством/
   участием по АКТУАЛЬНОМУ состоянию игрока, не по присланному.
4. Рассылка приватных — только участникам `{a,b}` онлайн.

---

## История правок

- 2026-05-17: создан. Зафиксированы §1 (mU id, ✅), §2 изначально ошибочно
  трактовал `Chin` как no-op; ниже в истории это исправлено на resync-контракт,
  §3–6 (Cmen/Choo/Cset/Cpri — спроектированы по клиенту), §7 (прочие
  дыры). Источник правды — `client/`, т.к. `docs/reference/server_reference/` для чата
  неполон (`/////FIX THIS SH`).
- 2026-05-17: §3–6 РЕАЛИЗОВАНЫ и верифицированы прямым protocol-пробом
  на live VPS (`tools/chat_probe.py`): `Cmen`→`mL`(4-частные записи)+`mN`;
  `Choo "DNO"`→`mO DNO:..`+`mU`, без `mL`; `Cset`×2→`mC 1`→`mC 2`
  (цикл+персист, колонка `chat_color` через миграцию); `Cpri 1`→
  `mO _1_2:Murasama`+`mU` (тег `_min_max`, имя собеседника, рассылка
  только участникам). Security-гейт `handle_channel_chat` активен.
  Открытая product-заметка: получатель ЛС, который онлайн и НЕ открыл
  `Cmen`, не получит уведомление о новом приватном сообщении до
  открытия списка (`mN` зашит в 0) — UX-пробел, не «чат сломан».
  Прочее (§7): GDon/Help/Miso/PCOP/THID уже заведены как Rust-контракт;
  полный порт `Sys_Miss` остаётся отдельной задачей.
- 2026-05-17 (репорты юзера с real-client, исправлено+probe-verified):
  - **DNO routing**: `Chat.cs:44` хардкод `ch="FED"` для ЛЮБОГО global
    (DNO — реальный канал) → DNO не виден, тёк в FED. Fix: wire-`ch`=
    реальный `channel_tag`. Probe: FED→ch=FED, DNO→ch=DNO. (§1)
  - **Дубли на реконнекте**: §2 переписан — `Chin` НЕ no-op, а
    механизм ресинка по `getLasts()` (реф `Chin` ПУСТ = неполон).
    Текущий login=`mO+mU`; `Chin` остаётся full/incremental resync
    (`"_"`→полная, `"1:cur:lasts"`→`id>lastid`). Старые выводы про
    `mO`-only не использовать без повторной проверки текущего кода.
  - **«старые мелкие» диагностирован и закрыт pass-2**: НЕТ пакета размера —
    `ChatLineInfo.cs:54` `if(gid<=0) fontSize=10`. Тот же корень что
    «цвета/кэш»: история шлёт `user_id=0`/`color`-рассинхрон/`time`
    sec≠min. → §1 pass-2: добавлены `chat_messages.player_id`
    +`color`, `time` минуты везде.
  - Юзер чистит чат вручную → backfill-scar старых строк (player_id=0)
    после pass-2-миграции неактуален (свежие строки будут верные).
