# Client UI backlog

Статус документа: рабочий backlog по интерфейсным проблемам Unity-клиента при
управлении через Rust-сервер. Клиент можно менять, но только точечно и после
доказательства, что серверная адаптация хуже или не решает проблему. Поэтому
каждый пункт ниже должен быть отнесён к одному из треков: server/wire fix,
client-only fix или product decision.

Для отдельного плана модернизации самого Unity UI под разные разрешения и DPI
см. `docs/CLIENT_UI_MODERNIZATION_PLAN.md`. Этот backlog оставлен шире: здесь
есть и wire-проблемы, и client-only дефекты.

## Критические

### UI-000. Клиентский layout не гарантирован на современных разрешениях

- Симптом: интерфейс "едет" по-разному на разных aspect ratio/DPI; окна и HUD
  могут перекрываться или уходить за экран.
- В клиенте уже есть центральный слой масштабирования:
  `DisplayScale.cs` + `ServerController.CanvasScale.cs`.
- Главный риск не в CanvasScaler как таковом, а в старых hardcoded размерах и
  локальных позициях внутри конкретных окон (`PopupManager`, programmator,
  prefabs/scene anchors).
- Дополнительный риск был в `DisplayScale.DensityBoostFor`: desktop HUD мог
  раздуваться до mobile cap `2.2x` от `Screen.dpi`. После live-обратной связи
  на MacBook M3 с кастомным scaled resolution выбран компромисс: desktop/laptop
  DPI boost сохранён, но ограничен отдельным cap `2.05x`; mobile cap остаётся
  `2.2x`. Если Unity отдаёт `Screen.dpi=0` на macOS при laptop-like render size
  (`short side <= 2240`), fallback выводит effective DPI из предполагаемой
  физической короткой стороны MacBook, чтобы logical и Retina backing sizes не
  давали разный HUD.
- Что делать: вести это отдельным frontend-треком по
  `CLIENT_UI_MODERNIZATION_PLAN.md`; не пытаться лечить layout серверными
  пакетами.

### UI-034. Масштаб HUD зависит от `Screen.dpi` и требует desktop/laptop cap

- Симптом: при одинаковом разрешении интерфейс может выглядеть по-разному на
  разных мониторах/ноутбуках/ретине. Это напрямую похоже на жалобу "на разных
  разрешениях/машинах всё едет", но причина лежит в клиентском scale-layer, а не
  в серверных пакетах.
- Доказательство из клиента:
  - `DisplayScale.DensityBoostFor` берёт `Screen.dpi` и через `DensityBoost`
    влияет и на `UiReferenceResolution`, и на `WorldTilePixels`.
  - Старый единый cap `2.2x` мог делать desktop HUD слишком большим, но полное
    отключение DPI на MacBook Retina делает GUI слишком мелким.
  - На macOS с `Screen.dpi=0` без fallback формула вообще возвращала `1.0`,
    то есть cap не имел значения; Unity не всегда стабильно отдаёт модель
    устройства как `MacBook`.
  - `ServerController.CanvasScale` логирует `dpi`/`density` и применяет
    `DisplayScale.UiReferenceResolution` ко всем root `CanvasScaler`.
- Почему фикс точечный: изменение не трогает scene anchors, prefabs или
  CanvasScaler wiring; оно только ограничивает desktop DPI boost отдельным
  потолком `2.05x` и делает macOS `dpi=0` fallback одинаковым для logical и
  Retina backing sizes MacBook-класса. Ручные `ui_scale`/`world_zoom` hotkeys
  остаются per-device escape hatch.
- Static gate: `python3 tools/ui_layout_audit.py --matrix-only --scale-matrix`.
  После правки desktop matrix может учитывать `DPI`, но должна быть capped at
  `2.05`; рост обратно к `2.2` на desktop считается regression-risk.
  Строка `macbook dpi=0` должна показывать fallback density, а не `1.00`, для
  laptop-like разрешений.
- Static fit counterpoint: `python3 tools/ui_layout_audit.py --matrix-only
  --fit-matrix "GUIWindow|ProgrammatorWindow"` показывает, что корневые fixed
  windows сами по себе не доказывают offscreen-баг: на `1024x768` при
  `dpi=160/220` `ProgrammatorWindow` остаётся примерно `617x468`. Поэтому
  live-дефект надо искать в overlap/dynamic-content, а не автоматически чинить
  root window size.
- Статус: desktop client fix applied; Mac Retina fallback больше не подставляет
  фиксированные `220 dpi`. Static matrix теперь даёт `DensityBoost≈1.56` и для
  logical `1512x982`, и для backing `3024x1964`, вместо скачка `2.05 -> 1.56`.
  Live visual matrix blocked until Unity project lock is released.

### UI-001. Общий GUI слишком хрупкий: Rust вручную собирает клиентские окна

- Симптом: окна не открываются, открываются пустыми, кнопки не работают, вкладки
  не переключаются или клиент падает без понятного серверного лога.
- Причина: `PopupManager.ShowHORB` читает `JsonUtility.FromJson<HORBConfig>`, где
  все коллекции строго плоские `string[]`: `buttons` парами, `tabs` парами,
  `list` тройками, `richList` пятёрками. См.
  `client/Assets/Scripts/Data/HORBConfig.cs` и
  `client/Assets/Scripts/UI/PopupManager.cs`.
- Доказательство: серверный builder уже фиксирует старые проблемы, например
  `tabs:[{object}]` не парсится клиентом как `string[]`
  (`server/net/session/ui/horb.rs`).
- Что делать: запретить ручную сборку HORB JSON в новых/старых хендлерах. Всё
  переводить на typed builder `server/net/session/ui/horb.rs`, добавить
  contract-тесты на чётность `buttons/tabs`, кратность `list/richList`, отсутствие
  объектных массивов.
- Статус: builder уже покрывает `buttons`, `tabs`, `list`, `richList`, `canvas`;
  settings GUI, buildings menu и DPBX crystal box переведены с ручного JSON на
  typed `Horb`; `input_place`/`input_console` тоже поддержаны builder-ом,
  `createprog`, `PREN` и auth GUI flow переведены на typed input dialogs; Spot
  GUI переведён на typed `Horb`; crafter recipe list/progress/detail переведены
  на typed `Horb`; storage crystal slider GUI и resp admin GUI переведены на
  typed `Horb`; market admin GUI и auction GUI переведены на typed `Horb`.
- Static gate: `python3 tools/ui_layout_audit.py --matrix-only --horb-risk`.
  Текущий ожидаемый baseline: `manual_horb_json=1`, и это только централизованный
  emit в `server/net/session/ui/horb.rs`. Любой рост этого счётчика означает
  новый ручной HORB payload и требует ревью.

### UI-035. Dynamic HORB content может ломать layout даже при нормальном root window fit

- Симптом: корневое окно помещается, но внутри ломаются scroll areas, plain
  `text`, canvas, rich rows или кнопки; визуально это выглядит как "интерфейс
  поехал", хотя `GUIWindow`/`ProgrammatorWindow` по sizeDelta не выходят за
  экран.
- Доказательство:
  - `PopupManager.ShowHORB` shrink/clamp применяется к активным `scrollView`,
    big input и `canvasGUI`, но обычный `insideTF` plain `text` не является
    отдельным scroll surface.
  - После runtime shrink/clamp старый `UpdateLayout` rebuild-ил только
    `buttonRow`, `tabsRow` и root `GUIWindow`, оставляя `scrollView`,
    `listContent`, `richContent` и `inputText` на более слабом layout refresh.
  - Static fit показал, что root `GUIWindow`/`ProgrammatorWindow` сами по себе
    помещаются на `1024x768` даже при DPI cap.
  - `--horb-risk` показывает `plain_text_format=0`; новые dynamic plain text
    места должны считаться regression-risk до live-проверки.
- Что делать: для длинного/переменного текста использовать `list`, `richList`
  или `%%` rich text, а не plain centered `text`. Перед client fix сначала
  прогонять `--horb-risk` и live-проверять конкретное окно.
- Статус: static guardrail added, server/wire cleanup done. `DPBX` crystal box
  больше не шлёт динамический многострочный plain `text`: строки кристаллов
  переведены в `list`, то есть в клиентский `ScrollRect`. Auction detail/order
  creation больше не шлют форматируемые plain `text`: имя последнего bidder
  переведено в read-only `list` row, count prompt стал статичным. Клиентский
  `PopupManager.UpdateLayout` расширен на `scrollView`, `listContent`,
  `richContent` и `inputText`, чтобы runtime shrink/clamp не оставлял внутренние
  контейнеры на старых layout-замерах. Live matrix всё ещё нужна для проверки
  overlap/читаемости.

### UI-002. `PCOP` не обработан на сервере

- Симптом: кнопка "создать копию программы" в программаторе ничего не делает.
- Клиент: `ProgrammerManager.OnCopyProgramm` шлёт TY `PCOP <programId>`.
- Статус: закрыто серверной правкой. `PCOP` добавлен в TY dispatch, обработчик
  проверяет ownership через `get_program`, создаёт новую запись с тем же code и
  именем `name (copy)`, затем возвращает список программ. Покрыто targeted
  integration tests, включая отказ на чужой `programId`.

### UI-003. Сохранённая программа может не открыться в редакторе из-за формата

- Симптом: программа сохраняется/запускается, но при повторном открытии интерфейс
  программатора пустой или не загружает исходник.
- Клиент: `SaveToString()` в old-format ветке возвращает чистый LZMA base64,
  `LoadFromString()` читает binary-формат только если `source[0] == 'X'`, а
  текстовый формат только если `source[0] == '$'`.
- Уточнение: `X` здесь не отдельный префикс. Клиент передаёт всю строку в
  `Convert.FromBase64String(source)`, значит `X` — ожидаемый первый символ
  base64 LZMA-payload в старом формате, включённом серверным `#F
  oldprogramformat+`.
- Риск: если в БД попадёт new-format или повреждённая строка, клиент не загрузит
  её, а серверный parser тоже не запустит программу.
- Что делать без правки клиента: не переписывать source "на глаз"; хранить
  строку из `PROG` как есть, но при parse/run failure явно останавливать
  программатор и показывать ошибку.

### UI-004. Кнопка play/stop программатора зависит от рассинхрона глобального состояния

- Симптом: программатор "не запускается", запускается не та программа, кнопка
  stop вместо открытия редактора шлёт reset, состояние панели расходится с
  реальным состоянием бота.
- Клиент: `ProgPanel.OnPlayStop` при `playing || handMode` шлёт `pRST`; при
  `programToSend.StartsWith("@")` шлёт `Pope` с `"@" + programToSend`; иначе
  вызывает `ProgrammerView.THIS.SendAndStartProgram()`.
- Риск: сервер должен очень точно поддерживать `@P`, `#P`, `#p`, `Gu`, `Pope`,
  `pRST` и `programToSend`-сценарии, иначе UI остаётся в неправильной ветке.
- Что делать: описать state machine программатора как контракт и покрыть
  protocol-пробами: открыть список, создать, открыть, сохранить+старт, stop,
  повторно открыть, переименовать, удалить, скопировать.

## Высокий приоритет

### UI-005. Меню программатора с несохранёнными изменениями выходит сразу

- Симптом: подтверждение "несохранённые изменения потеряются" показывается, но
  выход в меню происходит сразу.
- Клиент: `ProgrammerManager.OnMenuButton` показывает `AYSWindowManager`, но
  после `if (ProgrammerView.unsaved)` безусловно вызывает `ExitToMenu()`.
- Ограничение: это клиентская логика; сервер не может отменить уже отправленный
  `Pope "="`.
- Client fix: после показа `AYSWindowManager` нужно выйти из `OnMenuButton`.
  Тогда `ExitToMenu()` вызывается только из OK callback, а cancel оставляет
  редактор открытым.
- Server-side: всё равно не считать `Pope "="` подтверждением сохранения или
  удаления selected program.

### UI-006. `Pope` payload сейчас сервером игнорируется

- Симптом: разные клиентские ветки шлют `Pope` с разным payload (`_`, `"="`,
  `programToSend`, `"@"...`), но Rust открывает один и тот же список программ.
- Клиент: `GUIManager.OnProgButton` шлёт `Pope programToSend`; `ProgrammerManager`
  шлёт `Pope "="`; `ProgPanel` может слать `Pope "@..."`.
- Проверка server_reference: `PopePacket` декодирует `Source`, но
  `Session.Pope` игнорирует payload и вызывает `StaticGUI.OpenGui(player)`.
- Статус: это не баг паритета. Оставить как product-risk до живого сценария, где
  payload действительно должен запускать/выбирать программу.

### UI-007. `UpdateProgramm(#p)` закрывает редактор сразу после загрузки

- Симптом: после сохранения/старта редактор моргает или исчезает; пользователь
  не понимает, сохранено ли.
- Клиент: `GUIManager.UpdateProgramm` загружает source, вызывает `Show()`, затем
  `programmator.SetActive(false)` и `ProgrammerView.active = false`.
- Это похоже на ожидаемый клиентский контракт "сохранить и закрыть", но UX
  опасен при ошибке запуска.
- Что делать: сервер должен отправлять `#p` только в сценариях, где закрытие
  редактора ожидаемо. Для простого открытия использовать `#P`.
- Статус: rename после `PREN` теперь отвечает `#P`, а не `#p`, чтобы после
  переименования редактор оставался открыт.

### UI-008. Chat scrollbar flicker

- Симптом: мигает ползунок чата.
- Уже доказано: баг остаётся без сети, значит не сервер. Причина в Unity
  `ScrollRect` с `AutoHideAndExpandViewport`.
- Client fix: `ChatManager` теперь не вызывает scroll/layout path без реального
  overflow `Content > Viewport`; при нескроллящемся контенте scrollbar скрывается
  через `CanvasGroup`, без отключения GameObject.
- Static status: `ChatManager.ChatScroll` сейчас указывает на ScrollRect
  `1790376288100406984`, где `m_VerticalScrollbarVisibility: 1` (`AutoHide`).
  Значит конкретно этот chat scroll уже не требует той правки; live-проверка
  всё ещё нужна, потому что другие ScrollRect в сцене могут использовать
  `AutoHideAndExpandViewport`.
- Server-side: не посылать лишние chat UI rebuild-пакеты, чтобы не усиливать
  визуальный эффект.

### UI-009. Дубли чата при реконнекте

- Симптом: сообщения визуально дублируются после reconnect.
- Причина: клиент `muHandler` визуально добавляет строки до дедупа History.
- Статус: серверный обход уже описан и реализован через login `mO` без полной
  истории и `Chin` resync.
- Что делать: держать contract-тест/пробу, не возвращать полную историю в login.

### UI-010. `mU` chat payload легко ломает весь чат

- Симптом: FED/DNO чат не отображается, Unity ловит `FormatException`.
- Причина: клиент ждёт ровно 7 частей `ID±COLOR±CID±TIME±NICK±TEXT±GID`.
- Статус: исправлено сервером, но это критический контракт. Дополнительно
  закрыта client-side resilience: `ChatManager` теперь использует `TryParse` и
  length/null guards в `mn`, `mc`, `ml`, `mo` и `mu`, чтобы malformed chat/menu
  packet пропускался, а не валил весь UI. `ServerController` регистрирует chat
  packet callbacks через собственные forwarding handlers, а не через
  `ChatManager.THIS.*`, поэтому init-order `ServerController.Init` больше не
  зависит от того, успел ли `ChatManager.Start`.
- Что делать: оставить golden-тесты на chat wire; не брать формат из неполного
  `server_reference`. Client guards не заменяют wire-тесты, потому что битые
  строки всё равно не должны отправляться сервером.

## Средний приоритет

### UI-011. Необработанные fire-and-forget события создают "мертвые" кнопки

- События: `THID`, `Help`, частично `GDon`, `Miso`, `Miss`, `Rndm`, `TAGR`,
  `TAUR`.
- Симптом: пользователь нажимает кнопку, локальный UI может закрыться или
  показать ожидание, а сервер ничего не делает.
- Что делать: для каждого события добавить явную dispatch-ветку: no-op с логом,
  полноценную реализацию или `OK` с понятным сообщением. Не оставлять silent
  unknown в интерфейсных действиях.
- Статус: закрыто по dispatch-контракту. `GDon` уже реализован как ежедневный
  бонус; `Help` возвращает явный `OK`, пока серверная справка не подключена;
  `Miso` шлёт `MM` с пустым text и скрывает mission panel штатным клиентским
  путём; `THID`, `Miss`, `Rndm`, `TAGR`, `TAUR` заведены как известные no-op с
  debug-логом.

### UI-012. Help/Wiki/Donation завязаны на устаревшие URL/продуктовые решения

- Клиент открывает `http://minesgame.ru/wiki`; `Help` и `GDon` требуют ответа
  сервера или внешней ссылки.
- Что делать: вынести URL/тексты в конфиг сервера. Если фича отключена, отдавать
  короткое `OK`, а не молчание.

### UI-013. Списки могут уходить за экран, если сервер шлёт длинный `text`

- Симптом: окна построек/рецептов/списков становятся неюзабельными.
- Причина: клиент прокручивает `list`, `clanlist`, `%%`-text, но обычный `text`
  центрируется/кладётся в `insideTF`.
- Что делать: длинные коллекции всегда слать через `list` или rich/canvas
  структуры, не через многострочный `text`.

### UI-014. `list` row с пустым subtitle становится некликабельным

- Клиент: если `cfg.list[3n + 1] == ""`, кнопка строки скрывается.
- Симптом: строка отображается, но на неё нельзя нажать.
- Что делать: для кликабельных строк всегда заполнять subtitle/label кнопки.
  Пустой subtitle использовать только для read-only строк.

### UI-015. `buttons` action не может быть пустым/нечётным/слишком свободным

- Клиент берёт action как `buttons[2n + 1]` и вызывает `Substring(0,1)` для
  некоторых веток.
- Симптом: NRE/ArgumentOutOfRange, кнопка не реагирует, back-button становится
  не тем действием.
- Что делать: builder должен запрещать нечётные массивы и пустые action, кроме
  специально описанных случаев. Ручной JSON не использовать.

### UI-016. `tabs` должен быть плоским массивом пар, активная вкладка = пустой action

- Симптом: вкладки не появляются или не переключаются.
- Причина: `HORBConfig.tabs` это `string[]`; объектные вкладки не парсятся.
- Что делать: только `Tab::active(label)` / `Tab::new(label, action)` из builder.

### UI-017. `richList` формат плохо типизирован

- Клиент ожидает 5-кортежи `[label, kind, values, action, value]`; `%R%`
  собирает значения только для `bool`, `drop`, `uint`, `text`.
- Симптом: настройки/админки сохраняют пустые или неверные значения.
- Что делать: все формы настроек, админки и building controls переводить на
  `RichRow`; добавить тесты на `%R%` action strings.
- Статус: settings GUI переведён на typed `Horb` builder; `RichRow::dropdown`
  добавлен для client `drop` rows.

### UI-018. `input_console` автоматически фокусит поле и блокирует игровые хоткеи

- Симптом: после открытия окна не работают WASD/горячие клавиши или ввод уходит
  в поле.
- Клиент: при `input_console` вызывает `SetSelectedGameObject(inputText)`.
- Что делать: использовать `input_console` только там, где действительно нужно
  немедленно печатать. Для справочных окон не включать.

### UI-019. HORB `css` парсится небезопасно

- Клиент парсит `css` через `Split('=')` и `float.Parse` без culture guard.
- Симптом: окно может сломаться при неправильном `css` или локали/десятичном
  формате.
- Статус: закрыто точечной client-правкой в `PopupManager.ShowHORB`: HORB
  `css` float-значения парсятся через `TryParse` с `InvariantCulture`, плохие
  значения пропускаются с warning вместо падения окна.
- Server-side: всё равно генерировать только заранее известные css-токены и
  числа с точкой; не принимать пользовательский css.

### UI-020. `%%` rich text/help формат может падать на плохих image/action строках

- Клиент парсит `%%` text по `§`, затем `=#` image rows и `>|` action rows.
- Симптом: help/справка/новости ломают окно из-за одного неверного разделителя.
- Что делать: не собирать этот формат вручную; если нужен help, создать helper
  с проверкой количества частей.

## Building GUI backlog

### UI-021. Crafter GUI сейчас отличается от ожидаемого inventory-grid

- По `docs/PARITY_AUDIT.md`: C# использует `InventoryItem` grid для выбора
  рецептов, Rust местами отдавал текст+кнопки.
- Симптом: выбор рецепта выглядит/работает не как оригинальный клиентский GUI.
- Что делать: перевести выбор рецепта на inventory/card/list контракт, который
  реально ожидает клиент, и проверить в живом клиенте.
- Статус: recipe list/detail/progress уже переведены с ручного HORB JSON на
  typed `Horb`; сам формат выбора рецептов всё ещё требует отдельного
  visual/parity pass, потому что C# reference ожидает inventory-grid стиль.

### UI-022. Crafter progress text отличается от клиентского спец-формата

- C# использует `@@\n...` и цветной progress bar; Rust использует другой текст.
- Симптом: окно прогресса выглядит чужеродно или неправильно выравнивается.
- Что делать: воспроизвести спец-префиксы `@/@@` и формат progress bar по
  клиентскому `PopupManager`.
- Статус: ручной HORB JSON для progress window убран, окно теперь строится
  typed `Horb`; визуальный паритет progress bar всё ещё не закрыт.

### UI-023. Gate/Spot null GUI не всегда шлёт `Gu`

- Симптом: окно может остаться открытым, когда игрок наступил на объект без GUI.
- По `PARITY_AUDIT`: C# `SendWindow(null)` закрывает окно через `Gu`, Rust в
  некоторых ветках просто return.
- Что делать: в server-side open GUI path при null/no window явно слать `Gu`.
- Статус: закрыто серверной правкой. `Gate.GUIWin() == null` и non-owner
  `Spot.GUIWin() == null` теперь явно вызывают `Gu` и сбрасывают
  `PlayerUI.current_window`, поэтому старое HORB-окно не остаётся висеть.

### UI-024. Spot GUI/привязка программы неизвестны

- C# reference для Spot неполный; `selected` не присваивается, GUI stub.
- Симптом: BotSpot есть на карте, но интерфейс назначения/запуска программы не
  определён.
- Что делать: сначала зафиксировать клиентский wire-контракт назначения
  программы Spot'у. Без этого не реализовывать "на глаз".

### UI-025. Programmator surface закреплён жёсткими размерами и требует visual pass

- Симптом: на части разрешений программатор выглядит слишком плотным, кнопки и
  grid могут стать тесными или потерять читаемость.
- Доказательство из клиента:
  - `ProgrammerView` создаёт сетку `16x12` с шагом `32f` и стартовой позицией
    `16f + 32f * k`, `-16f - 32f * j`.
  - `ProgrammatorWindow` в сцене имеет `sizeDelta: {x: 564.3, y: 428}`.
  - `ProgView` имеет `sizeDelta: {x: 520.3, y: 390}`.
  - `ProgButton` имеет высоту `26`.
- Что делать: не переписывать сетку сразу. Сначала пройти visual matrix на
  `1366x768` и `1024x768`, затем решать, нужен ли scale wrapper, scroll или
  локальная подгонка spacing/button height.
- Статус: evidence collected, fix not yet approved. Static fit показывает, что
  `ProgrammatorWindow` и `ProgView` в изоляции помещаются даже при DPI cap; live
  pass должен проверять overlap с HUD/chat, доступность кнопок и читаемость
  help/toolbar, а не только размер корневого окна.

### UI-026. `OverlayRenderer.Update` падает до готовности клиента/grid

- Симптом: `NullReferenceException` в `OverlayRenderer.Update()` на
  `Assets/Scripts/Gameplay/OverlayRenderer.cs:43`.
- Причина: `Update` deref-ил `ClientController.THIS`/`grid` без readiness guard,
  а `GUIManager`/`ServerController.Handlers` звали `OverlayRenderer.THIS`
  напрямую при inventory/overlay packets. При другом порядке инициализации сцены
  или отключённом overlay root это валит клиентский frame loop.
- Статус: закрыто клиентской правкой. `OverlayRenderer.Update`, `HideGrid` и
  `AddGrid` теперь имеют readiness guards; прямые call sites проверяют
  `OverlayRenderer.THIS != null` перед вызовом. Inventory grid payload также
  валидируется по `w/h` и длине `mapStr`/`codes`, чтобы malformed overlay packet
  не падал на построении UI grid.

### UI-027. `Robot.BodyUpdate` падает на неготовых/невалидных skin sprites

- Симптом: `NullReferenceException` в `Robot.BodyUpdate()` на
  `Assets/Scripts/Gameplay/Robot.cs:197`, вызвано из `Robot.Update()`.
- Причина: `Robot.BodyUpdate` индексировал `Robot.sprites[this.skin]` и
  использовал `bodyRenderer` без проверки. `skin` приходит из server packet, а
  `Resources.LoadAll<Sprite>("skins")`/prefab wiring могут быть не готовы или
  иметь меньше элементов, чем ожидает код.
- Статус: закрыто клиентской правкой. `Robot.Start`, `SetSkin`, `Update`,
  `PositionUpdate` и `BodyUpdate` получили guards на `body`, `tail`,
  `bodyRenderer`, `Robot.sprites`, `TerrainRenderer.map`/`CellModel.isEmpty` и
  clamp skin index; tail/update path больше не падает при отсутствующем
  `RobotRenderer.THIS` или `tail`.

### UI-036. Mission HUD падает на раннем/битом mission packet

- Симптом: mission panel packet может прийти до `MissionPad.Start`, а текст с
  битым `%T%...%` timer macro или progress `max=0` может уронить клиентский UI.
- Доказательство из клиента:
  - `ServerController.Handlers.UMPHandler` вызывал
    `MissionPad.THIS.UpdateMissionPanel(...)` без null-check, хотя progress
    update рядом уже guarded.
  - `MissionPad.UpdateMissionPanel` делал `Substring`/`int.Parse` по `%T%`
    без проверки закрывающего `%` и числового значения.
  - `MissionPad.UpdateMissionProgress` делил bar width на `max`.
- Статус: закрыто клиентской правкой. `UMPHandler` проверяет
  `MissionPad.THIS != null`; timer macro парсится через checked bounds +
  `int.TryParse`; progress bar clamp-ится и не делит на ноль.

### UI-037. Inventory grid может падать на битой строке или раннем packet

- Симптом: inventory panel и item grid могут падать, если `show/full` packet
  приходит до `InventoryPanel.Start` или если grid string содержит непарные /
  нечисловые элементы.
- Доказательство из клиента:
  - `ServerController.Handlers.InventoryHandler` вызывал
    `InventoryPanel.THIS.ShowInventory/ShowFullGrid(...)` без null-check.
  - `InventoryPanel.makeGrid` использовал `int.Parse(parts[i])` и
    `int.Parse(parts[i + 1])` без проверки формата.
- Статус: закрыто клиентской правкой. `InventoryHandler` guarded по
  `InventoryPanel.THIS != null`; `makeGrid` использует `TryParse` и пропускает
  битые элементы вместо падения всего UI.

### UI-038. Popup/HORB handler зависит от готовности PopupManager и полного UP payload

- Симптом: `GU`/`Gu` popup packet может упасть, если приходит до
  `PopupManager.Start`, или если `up` payload не содержит ожидаемые строки
  `k/i/si`.
- Доказательство из клиента:
  - `unsafePopupHandler` вызывал `PopupManager.THIS.ShowHORB/ShowUP(...)`
    напрямую.
  - `SkillsWrapper.k.Split(...)`, `SkillsWrapper.i.Length` и
    `SkillButton.skillShorts.ContainsKey(SkillsWrapper.si)` не проверяли `null`.
- Статус: закрыто клиентской правкой. Popup show/close call sites проверяют
  `PopupManager.THIS != null`; `up` payload нормализует `null` строки в пустые,
  отвергает отрицательный `s` и не вызывает `ContainsKey` с null key.

### UI-039. HUD/status packets могут прийти до готовности GUIManager

- Симптом: ранние `P$`/`LV`/`ON`/`@B`/`GE`/`@L`/`@S`/settings/panel/status
  packets могут падать на прямом обращении к `GUIManager.THIS`,
  `MiniSkillManager.THIS` или вложенным HUD fields.
- Доказательство из клиента: `MoneyHandler`, `LevelHandler`, `OnlineHandler`,
  `BasketHandler`, `GeoHandler`, `LiveHandler` и `SkillHandler` напрямую
  вызывали UI methods/fields без проверки singleton readiness.
- Статус: закрыто клиентской правкой. Handlers сохраняют прежний parse/format
  contract, но перед touch UI проверяют `GUIManager.THIS`,
  `GUIManager.THIS.GeoTF`, `GUIManager.THIS.statePanel`, misc HUD fields или
  `MiniSkillManager.THIS`.

### UI-040. Programmator/OK/speed packets завязаны на готовность локальных managers

- Симптом: `#P`/`#p`/`@P`/`OK`/`sp` packets могут падать, если приходят до
  готовности `GUIManager`, `ProgrammerView`, `OKWindowManager` или
  `ClientController`.
- Доказательство из клиента:
  - `ProgrammatorOpenHandler`/`ProgrammatorUpdateHandler` напрямую звали
    `GUIManager.THIS.OpenProgramm/UpdateProgramm`.
  - `GUIManager.OpenProgramm/UpdateProgramm` делали `source.Length` и обращались
    к `ProgrammerView.THIS` без readiness guard.
  - `SpeedHandler` и `OKHandler` напрямую трогали `ClientController.THIS` и
    `OKWindowManager.THIS`.
- Статус: закрыто клиентской правкой. Handlers проверяют singleton readiness,
  `GUIManager` нормализует `source ?? string.Empty` и не трогает
  `ProgrammerView.THIS`, если editor view ещё не создан.

### UI-041. Gameplay/HB packets могут падать до готовности render managers

- Симптом: ранние `Live`/`SmoothTP`/`TP`/`BI` packets и HB bundle events могут
  падать, если `ClientController`, `RobotRenderer`, `LocalChatMessages` или
  `PackRenderer` ещё не инициализированы.
- Доказательство из клиента: `LiveHandler`, `SmoothTPHandler`, `TPHandler`,
  `BotInfoHandler`, HB `C`/`O` cases напрямую звали render/controller methods.
- Статус: закрыто клиентской правкой. Handlers проверяют readiness перед
  `Tremor`, teleport, bot init, local chat spawn и pack add/remove.

### UI-042. Programmator локальные кнопки могли падать на незавершённой UI-сцене

- Симптом: click handlers программатора могли дать `NullReferenceException`, если
  локальный manager или singleton ещё не готов, либо scene reference отсутствует.
- Доказательство из клиента:
  - `ProgrammerManager.Start` без проверок подписывал `Button.onClick` у всех
    кнопок и `OpenWikiButton` напрямую дергал `ServerController.THIS`.
  - `ProgrammerManager.OnStartButton`, `OnClearButton`, `OnCopyButton`,
    `OnRenameButton`, `ExitToMenu` напрямую использовали `ProgrammerView.THIS`,
    `AYSWindowManager.THIS` и `ServerTime.THIS`.
  - `ProgPanel.OnPlayStop` напрямую дергал `GUIManager.THIS`,
    `ServerTime.THIS` и `ProgrammerView.THIS`, а `Update` предполагал готовые
    image refs.
- Статус: закрыто клиентской правкой. Кнопки подписываются только при наличии
  refs, destructive confirmations не выполняются без `AYSWindowManager`, сетевые
  действия не выполняются без `ServerTime`, а `ProgPanel` проверяет manager/image
  readiness перед touch UI.

### UI-043. World init/reconnect flow мог падать на UI managers до готовности сцены

- Симптом: `cf` world config packet во время старта или реконнекта мог уронить
  клиент, если UI/network managers ещё не готовы или scene references отсутствуют.
- Доказательство из клиента:
  - `WorldInitializer.Start` напрямую включал `pad` и подписывался на `Obvyazka`
    без проверки refs.
  - `OnWorldConfig` напрямую трогал `ConnectionManager.THIS.connectionText`,
    `ServerTime.THIS`, `SoundManager.THIS`, `PopupManager.THIS`, `GUIManager.THIS`
    и `ChatManager.THIS`.
  - На reconnect ветке UI cleanup (`CloseWindow`, `CloseInventoryItem`,
    `ChangeProgTo`) выполнялся без readiness guards.
- Статус: закрыто клиентской правкой. `WorldInitializer` проверяет scene refs и
  singleton readiness перед UI/network touch; `Miss`/`Chin`/`Rndm` отправляются
  через guarded helper, а reconnect cleanup пропускает неготовые managers вместо
  падения frame loop.

### UI-044. Runtime FX/local chat objects могли падать после renderer/controller teardown

- Симптом: серверно-триггерные визуальные эффекты и локальные чат-бабблы могли
  дать `NullReferenceException`, если bot renderer, client controller или pool
  уже не готовы во время реконнекта/смены мира.
- Доказательство из клиента:
  - `LocalChatMessages.AddLocalMessage` напрямую инстанцировал prefab/canvas и
    читал `RobotRenderer.THIS.Bots`.
  - `GunShot.Setup/Update` напрямую читал `RobotRenderer.THIS.Bots` и освобождал
    объект через `ClientController.THIS.gunShotPool`.
  - `Boom`, `Bz`, `CrysPlus` завершали анимацию через `ClientController.THIS.*Pool`
    без проверки controller/pool readiness.
- Статус: закрыто клиентской правкой. FX и local chat теперь проверяют prefab,
  canvas, renderer/bot dictionary и pools; если pool/controller уже недоступен,
  transient FX object уничтожается вместо падения.

### UI-045. Малые HUD-панели могли падать при неполном scene wiring/init

- Симптом: inventory, mission и OK-window могли дать `NullReferenceException`,
  если scene refs, buttons, `ServerTime` или `AYSWindowManager` ещё не готовы.
- Доказательство из клиента:
  - `InventoryPanel.Start/makeGrid` напрямую трогал `button`, `inventoryGrid`,
    `inventoryItemPrefab` и `ServerTime.THIS`.
  - `MissionPad.Start/UpdateMissionProgress/UpdateMissionPanel` напрямую трогал
    `Button`, `bar`, `tf`, `webImage` и nested parent `RectTransform`.
  - `OKWindowManager.Start/CheckQueue/Update` напрямую использовал `ExitButton`,
    title/body text refs и `AYSWindowManager.THIS.gameObject`.
- Статус: закрыто клиентской правкой. Панели проверяют scene refs/singletons
  перед подпиской, packet send, layout/progress update и hotkey close.

### UI-046. `ChatManager` падал на неполных prefab refs, scroll rect и packet-ready state

- Симптом: чат мог уронить клиент при открытии меню, приходе списка чатов,
  смене режима или отправке сообщения, если `ChatInput`, `ChatScroll`,
  `DownChatContainer`, `ChatLinePrefab` или `ServerTime/ClientController` еще
  не готовы.
- Доказательство из клиента:
  - `mnHandler`, `mlHandler`, `moHandler`, `mcHandler`, `SendChat` и scroll
    helpers обращались к сценовым refs и nested UI components без guard-ов.
  - `AddLine` и `AddMiniLine` полагались на `GetComponent`/`GetChild` и packet
    send без проверки prefab/component readiness.
  - `UpdateChatMode` переключал `LeftGUI`, `ChatPanel`, `ChatToggle*` и
    `DownChatContainer` напрямую.
- Статус: закрыто клиентской правкой. Чат теперь пропускает действие, если
  scene refs или network singletons не готовы, вместо падения frame loop.

### UI-047. `ClientController.Update` и click-path падали на неготовых UI singletons

- Симптом: основной gameplay input loop мог упасть во время старта, reconnect
  или смены scene, если `GUIManager`, `ChatManager`, `AYSWindowManager`,
  `OKWindowManager`, `PopupManager`, `TutorialNavigation`, `MapViewer` или
  `PackRenderer` ещё не готовы.
- Доказательство из клиента:
  - `NoGUIClick`, `TryToGoto`, `Update` и movement hotkeys читали singleton UI
    refs напрямую и смешивали их с packet-send logic.
  - `Camera.main`, `this.serverTime`, `this.obvyazka` и `PackRenderer.THIS`
    тоже могли быть пустыми в transitional states.
- Статус: закрыто клиентской правкой. Input loop теперь проверяет readiness
  перед чтением UI state и packet send, вместо падения на null singleton.
  Дополнительно защищены стартовые scene refs (`mainRenderer`, `obvyazkaObject`,
  `Cursor`, auto-dig/no-GUI buttons) и cursor update path.

### UI-048. `ServerController` packet handlers могли трогать client/render/UI refs до готовности

- Симптом: входящие пакеты во время старта/reconnect могли уронить клиент до
  готовности `ClientController`, `RobotRenderer`, `TerrainRenderer`,
  `SoundManager`, `PopupManager` или других UI singletons.
- Доказательство из клиента:
  - `resp`, `bibika`, `autoDigg`, `BadCells`, `Basket`, `PopupClose` и settings
    handlers напрямую трогали controller/sound/terrain refs.
  - `HubTranslator` вызывал `robotRenderer`/`clientController` при условии только
    static `*.inited`, не проверяя сами поля `ServerController`.
- Статус: закрыто клиентской правкой. Packet handlers теперь пропускают UI/FX
  side effects, если соответствующий client/render/UI ref ещё не готов.

### UI-049. `ClientController` runtime FX/audio helpers могли падать на prefab/pool/render refs

- Симптом: packet-driven эффекты, звук или pooled animations могли дать
  `NullReferenceException` при неполной сценовой сборке, reconnect/teardown или
  некорректном FX payload.
- Доказательство из клиента:
  - `AddFX`, `AddDirectedFX`, `AddCrys`, `AddBoom`, `AddAnimation` и `AddBz`
    использовали prefab/pool results, `RenderWrapper`, `RobotRenderer.THIS.Bots`,
    `SoundManager.THIS` и `crysFromCode[col]` без полного readiness/bounds guard.
  - `GetFree(out index)` мог вернуть `null` при валидном index, после чего код
    сразу делал `GetComponent`.
- Статус: закрыто клиентской правкой. Runtime FX/audio path теперь пропускает
  конкретный side effect, если prefab, pool object, renderer, bot object, sound
  manager или crystal-code index не готовы.

### UI-050. Chat scrollbar flicker находит себя в repeated layout/auto-hide path

- Симптом: вертикальный ползунок в чате мигает при новых сообщениях, открытии
  chat panel или пересчёте layout.
- Доказательство из клиента:
  - `ChatManager` вызывал `LayoutRebuilder`/`Invoke("ScrollDown")` несколько раз
    подряд и принудительно сбрасывал `verticalNormalizedPosition`.
  - `m1client.unity` держал `ChatScroll` на auto-hide visibility с `Scrollbar`
    transition/fade.
- Статус: частично исправлено. `ChatManager` теперь не трогает scroll/layout
  path без реального overflow; `ScrollDown` и `ForcedScrollDown` теперь
  пересчитывают layout, скрывают scrollbar через `CanvasGroup`, и не дергают
  его при контенте, который помещается в viewport. Для самого момента появления
  scrollbar transition отключён (`Selectable.Transition.None`), а visible-state
  применяется idempotent, чтобы repeated layout pass не перезапускал fade/tint.
  Headless verifier прошёл. Нужна живая Unity-проверка на экране с нулевым,
  маленьким и переполненным количеством строк.

### UI-051. Crystal transfer slider терял максимум из-за float precision

- Симптом: при переносе большого количества кристаллов выбор 100% давал
  99.999999% и отправлял не весь доступный объём.
- Доказательство из клиента:
  - `CrystalScroller.Update` вычислял `value` как `(long)((float)d * bar.value)`
    или через float easing-кривую.
  - Unity `Scrollbar.value` на правом краю может быть `0.99999994`, а cast к
    `long` всегда округляет вниз.
- Статус: закрыто клиентской правкой. Расчёт количества теперь идёт через
  `double`, с snap к точным 0%/100%; при максимуме `value == d`.

### UI-052. История чата могла не приходить после перезахода

- Симптом: пользователь написал сообщение, перезашёл, а история текущего чата
  пустая.
- Доказательство из server/client path:
  - login sync отправлял только `mO`; сама история зависела от последующего
    клиентского `Chin`.
  - повторный `mU` мог визуально дублироваться, потому что `ChatManager.muHandler`
    вызывал `AddLine` до проверки `LastIDs`.
- Статус: закрыто точечной server/client правкой. Login sync теперь отправляет
  `mO + mU` для текущего канала из bounded in-memory history, а клиент добавляет
  строку в визуальный чат только если `id > LastIDs[channel]`.

### UI-032. Market/auction/settings используют сложные формы, нужен единый контракт

- Симптом: ставки, ордера, настройки, profit/admin формы легко ломаются из-за
  `%M%`, `%R%`, `choose:{id}`, `auc...` action strings.
- Что делать: вынести отдельные typed builders для auction/order/settings,
  как уже сделано для базового HORB.

### UI-033. Не все client UI scripts реально подключены к сценам/prefab-ам

- Симптом: можно найти очевидный fixed-size/runtime-layout код и потратить время
  на правку, которая не меняет игру.
- Доказательство: `python3 tools/ui_layout_audit.py --script-usage` показывает
  `GlobalChatManager.cs` как единственный unreferenced attachable script среди
  UI/Gameplay MonoBehaviour-кандидатов.
- Что делать: перед client-side правкой проверять script usage. Если скрипт не
  привязан к Unity YAML, считать его `needs evidence`, пока не найден runtime
  attach/add component или другой asset path.
- Статус: audit guardrail добавлен, active UI-баг из `GlobalChatManager` не
  заводить без живого воспроизведения.

## Программатор: отдельный список

### UI-026. Нужна серверная state-machine спецификация программатора

- Минимальный сценарий: `Pope` список -> `createprog` -> `#P` open ->
  `PROG` save+run -> `Gu` close -> `#p` update -> `@P 1` -> `pRST` stop ->
  `@P 0` -> `#P` reopen.
- Что делать: оформить в `docs/PROTOCOL.md` или отдельном тест-доке и покрыть
  protocol-пробой.
- Статус: спецификация оформлена в `docs/PROTOCOL.md` в разделе
  `Programmator State Machine`. Targeted integration tests добавлены для
  `PROG`, `pRST`, `PCOP`, `PREN` и `PDEL`; осталась live-проба в
  Unity-клиенте.

### UI-027. `PROG` decode не валидирует compiled length строго

- Статус: закрыто. `decode_prog_packet` теперь отклоняет payload, если
  compiled-length указывает за конец буфера, покрыт unit-тестом, а malformed
  payload получает `@P 0` и `OK`-сообщение.

### UI-028. Ошибка компиляции/парсинга программы не показывается пользователю

- Если Rust не смог `parse_normal`, `run_program` просто не включает running.
- Симптом: пользователь нажал старт, окно закрылось, программатор не работает.
- Статус: закрыто. При `PROG` сервер отличает "сохранено, но не запущено" от
  "запущено" и отправляет `OK` с короткой ошибкой при parse/run failure.
  Дополнительно закрыт stale-running регресс: невалидный новый source теперь
  сбрасывает старую running-программу, чтобы клиент не получил ложный `@P 1`.

### UI-029. Удаление/переименование/копирование должны возвращать предсказуемый UI

- Сейчас `PDEL` по паритетной заметке не шлёт пакетов; `PREN` открывает custom
  dialog; `PCOP` реализован.
- Симптом: после действия пользователь не понимает, что произошло.
- Статус: закрыто на server/wire уровне. `PCOP` возвращает список программ с
  новой копией, `PREN` открывает typed rename dialog, rename возвращает
  актуальную программу через `#P`, `PDEL` оставлен без wire-ответа ради текущего
  паритета, но in-memory selected/running сбрасывается. Все эти ветки покрыты
  targeted integration tests.

## Низкий приоритет / deferred

### UI-030. Клиентский `OnMenuButton` подтверждает и одновременно выполняет выход

- Это дублирует UI-005, но как чистый client defect: без изменения клиента
  полностью не исправляется.
- Статус: закрыто точечной client-правкой в `ProgrammerManager.OnMenuButton`:
  `return` после показа confirmation.

### UI-031. Некоторые UI-баги являются чисто клиентскими и не должны маскироваться

- Chat scrollbar flicker.
- Layout issues внутри Unity prefabs/scene.
- Ошибки фокуса/клавиатуры при локальных окнах.
- Что делать: помечать как `client-only`, не тратить время на серверные
  workaround, если live-проверка показывает воспроизведение без сети.

## Порядок работ

1. Зафиксировать HORB contract tests и запретить ручной JSON для новых окон.
2. Перед каждой client C# правкой проверять
   `python3 tools/ui_layout_audit.py --script-usage`.
3. Закрыть `PCOP`.
4. Провести live-пробу программатора по сценариям UI-003/UI-004/UI-026.
5. Нормализовать формат source программ на сервере.
6. Добавить user-visible ошибки для parse/run failures.
7. Пройти все building GUI, которые ещё собирают JSON вручную, и перевести на
   builder.
8. Отдельно решить продуктовые Help/GDon/Wiki URL.
