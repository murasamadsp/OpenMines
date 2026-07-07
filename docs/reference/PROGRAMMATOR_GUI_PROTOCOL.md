# Programmator GUI/Wire Contract

Этот файл фиксирует текущий контракт legacy Unity-клиента и C# reference-сервера
для GUI программатора. Перед правками `server/src/net/session/social/misc.rs`,
`server/src/net/session/social/buildings.rs`, `server/src/net/session/ui/gui_buttons.rs`
и `server/src/game/actors/programmator.rs` сверяться с этим файлом.

## Источники истины

- Unity: `client/Assets/Scripts/UI/GUIManager.cs`
- Unity: `client/Assets/Scripts/UI/ProgPanel.cs`
- Unity: `client/Assets/Scripts/UI/ProgrammerView.cs`
- Unity: `client/Assets/Scripts/Gameplay/ServerController.Handlers.cs`
- C# reference: `references/server_reference/Server/Session.cs`
- C# reference: `references/server_reference/GameShit/Programmator/StaticGUI.cs`
- Wire: `docs/PROTOCOL.md`

Клиент менять нельзя для серверного фикса. Сервер обязан подстроиться под эти
переходы.

## Client -> Server

### `Pope`

Кто шлёт:

- `GUIManager.OnProgButton()` шлёт `Pope` только если programmator UI закрыт и
  `ProgrammerView.opened == false`.
- `ProgPanel.OnPlayStop()` шлёт `Pope` только если `GUIManager.programToSend`
  начинается с `@`.

Payload клиентом не используется как строгий серверный контракт. C# reference
на `Pope` игнорирует payload и вызывает `StaticGUI.OpenGui(player)`.

Сервер должен:

- открыть HORB-список программ через `GU`;
- не запускать программу;
- не менять `ProgrammerView.programId`;
- не слать `@P 1`.

### `PROG`

Кто шлёт:

- `ProgrammerView.SendAndStartProgram()`.

Payload:

```text
[len:i32 LE][program_id:i32 LE][compiled_program:len bytes][source:utf8]
```

Важное:

- `program_id` обязан быть id, ранее полученным клиентом из `#P` или `#p`.
- `program_id <= 0` не является валидной программой.
- Сервер не должен создавать программу с id `0` и именем `program`.

C# reference:

- `Session.PROG` -> `StaticGUI.StartedProg(player, p.prog)` -> `player.ProgStatus()`.
- `StaticGUI.StartedProg` сохраняет source, запускает программу, затем шлёт
  `UpdateProgrammatorPacket` (`#p`).

Текущая server-side девиация от этого участка C#: на успешном `PROG` сервер
сохраняет source и запускает runtime, но не шлёт `#p`. Ручная проверка Unity
показала, что `#p` на старте будит тяжёлый редактор/GUI и может фризить клиент.

Сервер должен:

- проверить владение `program_id`;
- сохранить source только в существующую программу игрока;
- если `program_id` положительный, но такой owned-программы нет — вернуть ошибку,
  не создавать программу с именем `program`;
- выбрать эту программу как selected;
- запустить программу;
- закрыть HORB/window state на сервере;
- отправить статус в порядке, совместимом с Unity:
  `Gu`, optional `@T`, затем `@P`, затем `BH`;
- не отправлять `#P/#p` на успешном старте программы.

Пакеты `@P/#p/#P/Gu` нельзя менять без ручной проверки клиента:

- `@P "1"` в Unity вызывает `GUIManager.ChangeProgTo(true)` и ставит
  `ProgPanel.playing = true`;
- `#p` вызывает `GUIManager.UpdateProgramm(...)`, загружает source, трогает
  редактор и в конце закрывает programmator object (`SetActive(false)`).

Итог после успешного запуска: программа работает, `ProgPanel.playing == true`,
редактор не висит поверх игры.

### `pRST`

Кто шлёт:

- `GUIManager.OnProgButton()` всегда шлёт `pRST` перед `Pope`/локальным открытием
  редактора, если programmator UI был закрыт.
- `GUIManager.OnProgCloseButton()` шлёт `pRST`.
- `ProgPanel.OnPlayStop()` при `ProgPanel.playing || ProgPanel.handMode` вызывает
  `OnProgCloseButton()`, то есть тоже шлёт `pRST`.

C# reference:

```text
if selected != null && !ProgRunning -> OpenProg(selected)  // #P
if ProgRunning -> RunProgramm()                           // stop
ProgStatus()                                              // @P
```

Unity всегда шлёт `pRST` перед `Pope`/локальным открытием редактора в
`GUIManager.OnProgButton()`. Поэтому серверная реализация не может дословно
отправлять `#P` на stopped selected: после login-гидрации selected уже есть, и
такой `#P` конфликтует со следующим клиентским шагом.

Сервер должен:

- если программа запущена: остановить, очистить runtime state, очистить server
  `current_window`, отправить `Gu`, затем `@P 0`;
- если программа не запущена: очистить server `current_window` и отправить
  только `@P 0`;
- не открывать `#P` из stopped `pRST`;
- если selected нет: не открывать фиктивную программу и не создавать `program`.

После stop клиент обязан получить `@P "0"`, иначе:

- `ProgPanel.playing` останется `true`;
- кнопка play/stop продолжит слать stop-ветку;
- UI и ручное движение будут выглядеть сломанными даже если сервер уже
  выставил `running=false`.

### `PDEL`

C# reference удаляет программу без wire-ответа и без `ProgStatus`.

Сервер должен:

- удалить только программу владельца;
- если удалена selected-программа, очистить selected/runtime state;
- не слать `@P` для паритета, если нет отдельной явно проверенной причины.

### `PREN`

C# reference открывает HORB rename dialog. После подтверждения rename:

- сервер сохраняет имя;
- отправляет `#p`, не `#P`;
- закрывает server window state.

## Server -> Client

### `GU`

Используется для HORB/window.

Для программатора:

- `Pope` отвечает `GU` со списком программ;
- `PROG` после запуска должен закрыть HORB/list через `Gu`/close before status;
- `pRST` при остановке running-программы должен закрыть окно через `Gu`.

Server-side `PlayerUI.current_window` должен синхронно очищаться при `Gu`, который
закрывает программаторные окна. Иначе движение будет блокироваться guard-ом
`window_open`.

### `#P`

Открыть редактор программатора.

Unity handler:

- `GUIManager.OpenProgramm(id,title,source)`;
- если `id == -1`: только title и `GUIManager.programToSend = source`;
- если `id != -1`: `ProgrammerView.programId = id`, source загружается,
  programmator object остаётся открытым.

Использовать:

- selected stopped program по `pRST`;
- open/create из HORB списка.

Не использовать:

- rename confirmation;
- background login hydration, если редактор не должен открываться поверх игры.

### `#p`

Обновить редактор/selected program без удержания редактора открытым.

Unity handler:

- `GUIManager.UpdateProgramm(id,title,source)`;
- если `id == -1`: только title и `GUIManager.programToSend = source`;
- если `id != -1`: `ProgrammerView.programId = id`, source загружается,
  затем programmator object закрывается (`SetActive(false)`).

Использовать:

- после rename;
- на login, если selected program есть и нужно восстановить client-side
  `ProgrammerView.programId/source`, но не открывать редактор.

Не использовать:

- после успешного `PROG` save/start.

### `@P`

Статус работы программатора.

Payload:

- `"1"`: running;
- `"0"`: stopped.

Unity `ProgrammatorHandler`:

- `"1"`:
  - `GUIManager.ChangeProgTo(true)`;
  - `robotRenderer.isProgrammator = true`;
  - `clientController.isProgrammator = true`;
  - `ProgPanel.playing = true`;
  - `clientController.stopAutoMove()`.
- `"0"`:
  - `GUIManager.ChangeProgTo(false)`;
  - `robotRenderer.isProgrammator = false`;
  - `clientController.isProgrammator = false`;
  - `clientController.TimeSync()`;
  - `ProgPanel.playing = false`.

Правило:

- любое серверное завершение/stop runtime-программы, видимое игроку, обязано
  доставить `@P 0`;
- любое успешное начало runtime-программы обязано доставить `@P 1`;
- нельзя менять только ECS `running`, не отправляя `@P`, если действие вызвано
  UI или opcode `Stop`.

## Login Flow

В `Player.Init()` после `#F`:

- если selected program есть: сервер должен отправить `#p` с id/name/source;
- затем сервер отправляет `@P 0`, если программа не запущена.

Это нужно, чтобы после входа клиент знал `ProgrammerView.programId` и source.
Если `#p` не отправить, кнопка play может отправить `PROG` с `program_id = 0`
или открыть список вместо запуска.

Сервер не должен выбирать “последнюю” или “единственную” программу неявно.
Selected program должен быть явным persistent состоянием игрока.

## Runtime Actions

Команды программатора (`Dig`, `Build`, `Geo`, `Heal`, macro-действия) идут через
`ProgrammatorQueue` и вызывают те же handlers, что ручные действия, но с
`programmatic=true`.

Правила:

- `programmatic=true` пропускает manual guards: open GUI window, manual-control
  block и ручные cooldown-и;
- коллизии, координаты, стоимость, доступ к гейтам/зданиям и dirty tracking
  остаются общими с ручным действием;
- Dig/Build/Geo/Heal/macro action delay: 3 раза в секунду
  (`333_333 microseconds`) по текущему gameplay contract;
- движение остаётся skill/speed based, если отдельно не решено иначе.

Opcode `Stop` внутри программы:

- должен завершить runtime state;
- должен доставить игроку `@P 0`;
- не должен превращаться в бесконечную паузу.

## Запрещённые решения

- Создавать программу `id=0`, `name="program"` при `PROG program_id=0`.
- Открывать список программ как fallback для валидного `PROG`.
- Молча выбирать другую программу, если selected отсутствует.
- Слать `#P/#p` на успешном `PROG` start без новой ручной проверки Unity.
- Менять порядок `@P/#p/#P/Gu` без сверки с Unity handlers.
- Чистить только клиентский или только серверный state: GUI state, ECS runtime
  state и wire status должны сходиться.
