# Client UI modernization plan

Цель: привести Unity-клиент к предсказуемому интерфейсу на современных
разрешениях и DPI, не ломая игровой протокол. Клиент можно менять, но только
точечно, с проверяемой пользой. Серверные UI-фиксы остаются отдельным треком:
см. `docs/CLIENT_UI_BACKLOG.md`.

## Короткая стратегия

Не делать "новый клиент" и не переписывать весь UI слой. Сначала убрать
доказанные причины того, что интерфейс выглядит по-разному на разных
разрешениях:

1. Зафиксировать визуальную матрицу и baseline.
2. Стабилизировать общие shell-компоненты: HORB popup, chat, programmator panel.
3. Только затем разбирать HUD-зоны и отдельные building окна.
4. После layout-стабилизации делать visual refresh: spacing, states, text,
   scrollbars, button consistency.

Каждая клиентская правка должна быть маленькой и обратимой: один сценарий, один
источник дефекта, одна проверка.

## Что означает "уровень 2026" для этого клиента

Это не про модный редизайн. Для OpenMines это набор инженерных стандартов:

- responsive constraints вместо случайных `sizeDelta` без границ;
- единый источник масштаба, без конкурирующих формул;
- окна не могут стать выше root canvas;
- длинный контент всегда скроллится, а не раздвигает экран;
- fixed tool surfaces имеют стабильный aspect ratio и масштабируются как блок;
- кнопки, scrollbar, input focus и confirmation работают предсказуемо;
- изменения проверяются на 16:9, 4:3, 5:4, ultrawide и small laptop;
- сервер не должен знать физический размер клиентского экрана.

## Анти-цели

- Не менять wire protocol ради косметики.
- Не переносить layout-логику на Rust-сервер.
- Не трогать `CanvasScaler` первым, потому что он уже централизован через
  `DisplayScale`.
- Не редактировать сцену массово вручную.
- Не чинить "все anchors" одной большой правкой.
- Не делать visual refresh до того, как окна перестанут уезжать за экран.

## Decision gate для клиентской правки

Правка клиента допускается только если она проходит этот фильтр:

| Вопрос | Требование |
| --- | --- |
| Сценарий | Есть конкретный экран, действие и разрешение. |
| Причина | Найден файл/метод/scene field, который создаёт дефект. |
| Альтернатива | Серверный workaround хуже или невозможен. |
| Объём | Изменение ограничено shell/helper/одной настройкой/одной веткой. |
| Проверка | Есть compile/audit и ручная проверка затронутых разрешений. |

Если хотя бы один пункт не выполнен, правка остаётся в backlog как
`needs evidence`.

## Рабочий roadmap

### Sprint A. Baseline и guardrails

- Запустить `tools/ui_layout_audit.py` и сохранить счётчики в задаче.
- Пройти ручную матрицу для HORB/chat/programmator/HUD минимум на:
  `1280x720`, `1366x768`, `1920x1080`, `1024x768`.
- Для каждого дефекта записать:
  - экран;
  - разрешение;
  - expected;
  - actual;
  - likely source;
  - server workaround возможен или нет.

Exit criteria: у нас нет абстрактного "всё едет"; есть список конкретных
дефектов с воспроизведением.

### Sprint B. Общий popup/HORB shell

- Держать `HORBConfig` wire-compatible.
- Clamp высоты делать в клиентском shell, потому что сервер не должен угадывать
  экран.
- Запретить серверу слать огромные plain `text` там, где нужен list/richList.
- Добавить contract tests на HORB builder на сервере.

Exit criteria: market/settings/crafter/help не уходят за экран на small laptop
и сохраняют нормальный вид на 1920x1080.

### Sprint C. Chat

- Отделить client-only layout/flicker от серверных дублей истории.
- ScrollRect не должен менять viewport при появлении/скрытии scrollbar.
- Проверить input focus: чат не должен ломать игровые hotkeys вне режима ввода.

Exit criteria: idle чат не мигает, reconnect не дублирует строки, input не
перекрывает HUD.

### Sprint D. Programmator

- Сначала закрыть protocol/state-machine (`Pope`, `#P`, `#p`, `@P`, `PROG`,
  `pRST`, `PCOP`, `PREN`, `PDEL`).
- Потом проверять layout.
- 16x12 grid считать fixed tool surface: он может масштабироваться как целый
  блок, но не должен ломать пропорции и click targets.
- Help/long panels должны быть scrollable.

Exit criteria: создать, открыть, сохранить, запустить, остановить, копировать,
rename/delete и выйти с unsaved confirmation можно без рассинхрона UI.

### Sprint E. HUD zones

- Разобрать HUD на зоны: top-left status, top/right actions, bottom chat/input,
  center overlays.
- Убирать только доказанные пересечения.
- Не менять visual identity и размеры всех элементов сразу.

Exit criteria: HUD не перекрывает chat/program panel на матрице разрешений.

### Sprint F. Visual refresh

- Единый spacing scale.
- Единая высота кнопок внутри одного типа окна.
- Hover/disabled/active состояния.
- Читаемые размеры текста без viewport-based font scaling.
- Современные, но нейтральные scrollbar/input/button states.

Exit criteria: layout уже стабилен; refresh не меняет сетевой контракт и не
ломает старые сценарии.

## Очередь решений

Эта очередь нужна, чтобы не спорить каждый раз "можно ли трогать клиент".
Пункты сверху имеют самый высокий ROI и проходят текущий gate лучше остальных.

| Статус | Зона | Решение | Почему |
| --- | --- | --- | --- |
| Done / verify | HORB shell | Ограничивать высоту окна внутри `PopupManager.ShowHORB`. | Общий shell используется множеством окон; сервер не знает экран клиента. |
| Done / verify | Chat | `ScrollRect.AutoHide`, не `AutoHideAndExpandViewport`. | Это client-only flicker; сервер не управляет viewport. |
| Done / verify | Programmator | Unsaved confirmation не должен сразу выходить в меню. | Сервер не может отменить локальный выход после click handler. |
| Active | HORB builder | Запретить ручной JSON и покрыть builder contract-тестами. | Это снижает количество "мертвых" кнопок и пустых окон без правок клиента. |
| Active | Programmator protocol | Закрыть `PCOP`, `PREN/#P`, `PROG` malformed/OK feedback. | Layout программатора бессмысленно чинить, пока state-machine расходится. |
| Done / needs live matrix | DisplayScale | Desktop/laptop DPI boost ограничен cap `2.05x`; mobile cap `2.2x` сохранён; macOS `dpi=0` при `short side <= 2240` выводит effective DPI из предполагаемой физической короткой стороны MacBook. | Это помогает Retina MacBook с кастомным logical/backing render size без скачка HUD между `2.05x` и `1.56x`. |
| Next | Programmator layout | Проверить 16x12 grid на `1366x768` и `1024x768`. | Это самый вероятный fixed tool surface, который ломается на small laptop. |
| Next | HUD zones | Найти реальные пересечения HUD/chat/program panel. | Массовая anchor-правка слишком рискованна без screenshots. |
| Later | Visual refresh | Button/input/scrollbar/text consistency. | Делать только после layout stability. |
| Hold | CanvasScaler rewrite | Не делать первым. | `DisplayScale` уже централизует масштаб, broad change слишком рискованен. |
| Hold | Scene-wide anchor rewrite | Не делать без конкретного дефекта. | Audit показывает риск, но не доказывает каждую правку. |

## Evidence ledger

Текущая картина по аудиту Unity UI:

| Метрика | Текущее значение | Интерпретация |
| --- | ---: | --- |
| YAML files | 53 | Сцена и prefab-ы участвуют в layout-риск зоне. |
| Script files | 63 | UI-логика частично строится runtime-кодом. |
| `anchored_position` | 2002 | Много absolute-позиций. Это риск, не автоматический баг. |
| `size_delta` | 2002 | Много fixed-размеров. Чинить только по сценариям. |
| point anchors | 1896 | Много элементов привязаны к точке, а не к responsive области. |
| runtime size mutations | 152 | Есть код, который может ломать layout после CanvasScaler. |
| runtime position mutations | 40 | Есть runtime-позиционирование, проверять по зонам. |
| screen dependencies | 27 | В основном ожидаемо в `DisplayScale`/rendering, но UI-кандидаты проверять отдельно. |
| attachable scripts | 42 | MonoBehaviour-скрипты из UI/Gameplay, которые могут быть привязаны к сценам/prefab-ам. |
| unreferenced attachable scripts | 1 | `GlobalChatManager.cs` не найден в scene/prefab references; не чинить как active UI без отдельного доказательства. |

Вывод: audit доказывает высокий риск старого layout-подхода, но не даёт права
на массовый rewrite. Он нужен как карта, куда смотреть при воспроизведённом
визуальном дефекте.

Дополнительное наблюдение по scaling-коду было закрыто точечной правкой:
`DisplayScale.DensityBoostFor` больше не использует mobile cap `2.2x` на
desktop. На desktop/laptop DPI-компенсация сохранена, но ограничена cap `2.05x`,
потому что на MacBook Retina полное отключение DPI делает GUI слишком мелким.
Если Unity отдаёт `Screen.dpi=0` на macOS при laptop-like render size
(`short side <= 2240`), клиент выводит effective DPI из предполагаемой
физической короткой стороны MacBook. Это важно, потому что Unity может вернуть
как logical scaled MacBook size (`1512x982`), так и Retina backing render size
MacBook-класса (`3024x1964`): оба режима теперь дают `DensityBoost≈1.56`, а не
скачок `2.05 -> 1.56`. Старый `<= 1800` порог оставлял backing-size GUI слишком
мелким.
Mobile builds сохраняют физическую DPI-компенсацию до `2.2x`.

До правки риск был численно большой. На `1366x768` и `UiSize=0.75` один и тот
же desktop-клиент мог получить примерно такие дефолтные масштабы HUD:

| `Screen.dpi` | `DensityBoost` | Эффект |
| ---: | ---: | --- |
| 0 / unknown | 1.00 | базовый desktop scale |
| macOS + 0 / unknown + short side <= 2240 | inferred physical MacBook short side | Retina fallback без скачка между logical/backing render size |
| 110 | ~1.64 | HUD заметно крупнее при том же разрешении |
| 220 | 2.05 desktop cap | HUD становится крупнее, но ниже mobile cap `2.2x` |

Значит это был не абстрактный refactor, а реальный кандидат на точечную
клиентскую правку. После фикса `--scale-matrix` должен показывать desktop
`DensityBoost` не выше `2.05` для всех `DPI`; live matrix всё равно нужна, чтобы
подтвердить читаемость HUD и размер видимого мира.

Дополнительная проверка перед клиентской правкой:
`python3 tools/ui_layout_audit.py --script-usage`. Если target-скрипт не
привязан ни к сцене, ни к prefab-у, правка не считается полезной, пока не найден
другой путь использования.

Для DisplayScale/DPI отдельно:
`python3 tools/ui_layout_audit.py --matrix-only --scale-matrix`. Этот режим
зеркалит текущую desktop-формулу `DisplayScale` и должен показывать, что
`DensityBoost`, `UiReferenceResolution` и `WorldTilePixels` не превышают
desktop cap `2.05x` при одинаковом разрешении, но разном `Screen.dpi`. Строка
`macbook dpi=0` проверяет fallback для Retina MacBook/laptop-like macOS
render size, где Unity может вернуть нулевой DPI. В матрице должен быть
backing-size кейс `3024x1964` и logical-size кейс `1512x982`: оба ожидаемо
дают fallback `DensityBoost≈1.56`. Если `1512x982` снова даёт `2.05`, это
regression-risk для MacBook/custom-resolution сценария.

Для fixed scene windows отдельно:
`python3 tools/ui_layout_audit.py --matrix-only --fit-matrix
"GUIWindow|ProgrammatorWindow"`. Этот режим переводит scene `sizeDelta` в
примерные экранные пиксели через текущую формулу `DisplayScale`. Текущий вывод
важен как критика гипотезы "окно просто не помещается": на `1024x768` при
`dpi=160/220` `ProgrammatorWindow` получается примерно `617x468` с запасом
`204x150`, а `GUIWindow` примерно `662x557` с запасом `181x105`. На MacBook-like
`1512x982` при `macbook dpi=0` fallback `ProgrammatorWindow` примерно `600x455`,
а `GUIWindow` примерно `645x542`; на backing `3024x1964` они физически
соответствуют примерно тому же размеру (`1024x777` и `1099x925` экранных px).
Значит корневые
окна в изоляции помещаются; оставшиеся live риски искать в перекрытиях с
HUD/chat, динамическом HORB-контенте, внутренних скроллах и плотности текста.

Для server-driven HORB dynamic content:
`python3 tools/ui_layout_audit.py --matrix-only --horb-risk`. Текущий baseline:
`manual_horb_json=1` (только `Horb` builder emit), `plain_text_format=0`,
`rich_no_scroll=0`. Рост `manual_horb_json` считается regression-risk, потому
что новый ручной HORB payload обходит typed builder. Рост `plain_text_format`
считается regression-risk: динамический текст должен идти в scroll/list/rich
контейнер, если заранее не доказано, что строка короткая.

## Definition of done для UI-изменения

Для любого изменения, которое влияет на клиентский UI:

1. В backlog есть defect id или явно указанная зона roadmap.
2. Есть короткое объяснение, почему это client fix, а не server/wire fix.
3. `M3_CLIENT_DIR=client ./client/verify.sh --list` проходит.
4. `python3 tools/ui_layout_audit.py` запускается и не показывает неожиданный
   рост risk-класса в затронутой зоне.
5. Для client C# target проверено
   `python3 tools/ui_layout_audit.py --script-usage`: скрипт реально привязан к
   scene/prefab или есть другое доказательство использования.
6. Для `DisplayScale`/CanvasScaler изменений дополнительно сохранён вывод
   `python3 tools/ui_layout_audit.py --matrix-only --scale-matrix` до/после и
   live-log `[DisplayScale] ... dpi=... density=...`.
7. Для fixed scene windows дополнительно проверен
   `python3 tools/ui_layout_audit.py --matrix-only --fit-matrix
   "<object-regex>"`, чтобы не путать глобальный scale-risk с доказанным
   offscreen-дефектом.
8. Для server-driven HORB окон дополнительно сохранён вывод
   `python3 tools/ui_layout_audit.py --matrix-only --horb-risk`; новый ручной
   `horb:` payload запрещён без отдельного обоснования.
9. Для touched UI есть ручной matrix note минимум по:
   `1280x720`, `1366x768`, `1920x1080`, `1024x768`.
10. Для server/wire companion есть `cargo test` или точечный protocol test.

Если ручная визуальная проверка невозможна в текущей среде, изменение можно
оставить как code-complete, но нельзя считать visual-complete.

## Текущее состояние

### Что уже есть

- В клиенте уже есть централизованный слой масштабирования:
  - `client/Assets/Scripts/UI/DisplayScale.cs`
  - `client/Assets/Scripts/Gameplay/ServerController.CanvasScale.cs`
- Canvas в сцене уже настроен на `ScaleWithScreenSize`.
- `ServerController.CanvasScale` обновляет `CanvasScaler` при изменении
  разрешения/safe area и поддерживает локальные hotkeys масштаба:
  - `Ctrl/Cmd +/-/0` для HUD;
  - `Ctrl/Cmd + Shift +/-/0` для мира.

### Главные источники поломок

1. **Hardcoded размеры в Popup/HORB shell.**
   `PopupManager.ShowHORB` вручную выставляет `600x245`, `596x264`,
   `498x55`, `55x55`, `400` и другие размеры. На нестандартных разрешениях
   это конкурирует с `CanvasScaler`.

2. **Абсолютные позиции и фиксированные сетки.**
   Программатор создаёт 16x12 иконок с шагом `32f` и локальными позициями.
   Это нормально для fixed tool surface, но должно жить в контролируемом
   responsive контейнере, а не зависеть от того, как встала вся панель.

3. **Слишком много UI собирается из wire-данных.**
   Сервер отдаёт HORB JSON, а клиент строит окно. Если JSON структурно верный,
   окно всё равно может быть плохо адаптировано, потому что shell старый.

4. **Сцена/префабы содержат много anchors в одну точку.**
   Это типичный источник "поехало на другом aspect ratio": элемент привязан к
   углу/центру и имеет fixed `sizeDelta`, вместо растяжения/constraints.

5. **Нет визуального regression gate.**
   UI можно "починить" на 1920x1080 и случайно сломать 1366x768 или ultrawide.

## Матрица проверки

Минимальные размеры для каждого UI-изменения:

- 1280x720
- 1366x768
- 1600x900
- 1920x1080
- 2560x1440
- 3440x1440
- 1280x1024
- 1024x768

Подробный чеклист по фазам: `docs/CLIENT_UI_TEST_MATRIX.md`.

Для каждого размера проверять:

- HUD не перекрывает критичные кнопки.
- Chat не мигает сильнее и не выходит за экран.
- Popup/HORB окно видно целиком, списки скроллятся.
- Программатор виден целиком или имеет понятное масштабирование/скролл.
- Текст кнопок не обрезается до нечитабельности.
- Игровая камера и HUD масштабируются независимо.

## Архитектурное правило

`DisplayScale` должен остаться единственным источником правды для глобального
масштаба. Новые правки не должны добавлять ещё одну независимую формулу
масштаба.

Разрешены:

- constraints и clamps внутри конкретного окна;
- responsive containers;
- ScrollRect вместо разрастания по высоте;
- локальные min/max размеры панели;
- server-side выбор `list/richList/canvas` формата.

Запрещены:

- новые "если 16:9, то..." в UI;
- магические buckets по aspect ratio;
- ручная подгонка под одно разрешение;
- правки префабов без проверки матрицы.

## Правило client patch gate

Клиентскую правку можно брать в работу только если выполнены все пункты:

1. Есть точный сценарий: какое окно/экран, какое разрешение, что именно едет.
2. Найден конкретный источник в клиенте: файл, метод, размер/anchor/layout.
3. Серверный workaround хуже: ломает wire-контракт, требует дублировать layout
   знания на сервере или не покрывает client-only баг.
4. Diff минимальный: один helper/одна настройка/одна ветка поведения, без
   переписывания сцены.
5. Есть проверка: compile/audit + ручная матрица хотя бы для затронутого окна.

Первый кандидат, который проходит этот gate: `PopupManager.ShowHORB`.
Причина: hardcoded размеры применяются уже после серверного `css`, а окно
используется множеством серверных GUI. Сервер не должен угадывать физический
экран клиента, поэтому защитный clamp в client shell полезнее, чем попытки
рассылать разные HORB JSON под разные разрешения.

## Phase 0. Инвентаризация и gate

**Цель:** получить измеримую картину до правок.

1. Зафиксировать список критичных экранов:
   - HUD;
   - chat;
   - popup/HORB shell;
   - programmator;
   - inventory/cargo;
   - market/auction;
   - crafter/storage/gun/resp/UP;
   - map;
   - settings/help.

2. Добавить lightweight UI audit checklist в документацию:
   - screenshot per resolution;
   - expected visible controls;
   - known acceptable defects;
   - blocking defects.
   Базовый чеклист: `docs/CLIENT_UI_TEST_MATRIX.md`.

3. Не менять визуал до первого прохода матрицы.

**Exit criteria:**

- Есть таблица экранов и разрешений.
- Для каждого экрана понятно: ok / broken / unknown.

## Phase 1. Popup/HORB shell

**Почему первым:** большинство серверных окон идут через `PopupManager.ShowHORB`.
Одна правка shell даст эффект на market/crafter/settings/buildings/help.

Работы:

1. Вынести magic sizes из `PopupManager.ShowHORB` в локальные константы или
   helper:
   - base window width;
   - base content height;
   - max content height;
   - input height;
   - inventory cell size.

2. Добавить расчёт доступной высоты окна от текущего canvas rect:
   - `maxWindowHeight = canvasHeight - safeMargins`;
   - `scrollView.height = min(contentPreferred, maxWindowHeight - header - buttons)`.

3. Длинный `text` и `list` не должны расширять окно за экран.

4. Сохранить wire-совместимость HORB: `HORBConfig` не менять без крайней
   необходимости.

**Client files:**

- `client/Assets/Scripts/UI/PopupManager.cs`
- prefabs: `ButtonLine`, `ClanLine`, `TextLine`, `MultilineText`, `CardLinePrefab`

**Server companion:**

- продолжать использовать `server/net/session/ui/horb.rs`;
- не слать огромные plain `text`, если это список.

**Exit criteria:**

- HORB окно с длинным списком не выходит за экран на 1280x720 и 1024x768.
- Market/settings/crafter не регрессируют на 1920x1080.

## Phase 2. Chat

Работы:

1. Разделить chat layout дефекты:
   - клиентский ScrollRect flicker;
   - дубли/история от сервера;
   - размер/позиция поля ввода;
   - список каналов.

2. Если flicker подтверждён как `AutoHideAndExpandViewport`, точечно изменить
   Scrollbar visibility в scene/prefab на стабильный режим.
   Для `ChatManager.ChatScroll` это `m_VerticalScrollbarVisibility: 1`
   (`AutoHide` без viewport expand).

3. Проверить, что chat не перекрывает HUD на 1366x768 и ultrawide.

**Client files likely involved:**

- `client/Assets/Scenes/m1client.unity`
- `client/Assets/Scripts/UI/ChatManager.cs`
- chat prefabs: `NewChatLine`, `NewChatMiniLine`, `ChatMenuLine`

**Exit criteria:**

- Нет визуального мигания без сетевых пакетов.
- Реконнект не создаёт визуальные дубли.

## Phase 3. Programmator UI

Работы:

1. Сначала закрыть protocol/state-machine:
   - `Pope`;
   - `#P`;
   - `#p`;
   - `@P`;
   - `PROG`;
   - `pRST`;
   - `PCOP`.

2. Только после этого трогать layout.

   Layout evidence на текущем коде не указывает на обязательный resize:
   `ProgView` и `ProgrammatorWindow` помещаются в статической fit matrix даже на
   `1024x768`; текущий риск больше похож на readiness/NRE и внутренние fixed
   handlers, чем на root window overflow.

3. Layout-задачи:
   - панель должна помещаться на 1366x768;
   - grid 16x12 должен иметь фиксированный aspect, но масштабироваться как
     единый tool surface;
   - кнопки start/copy/rename/delete/help/menu не должны съезжать;
   - help panel должен быть scrollable.

4. Client-only candidate:
   - `ProgrammerManager.OnMenuButton` вызывает `ExitToMenu()` сразу после показа
     confirmation. Это можно исправить точечно, но только после решения UX.

**Client files:**

- `client/Assets/Scripts/UI/ProgrammerView.cs`
- `client/Assets/Scripts/UI/ProgrammerManager.cs`
- `client/Assets/Scripts/UI/ProgPanel.cs`
- `client/Assets/Prefabs/ProgIcon.prefab`

**Exit criteria:**

- Создать/открыть/запустить/остановить/переоткрыть/копировать программу можно
  без рассинхрона UI.
- На 1366x768 весь основной tool surface доступен.

## Phase 4. HUD

Работы:

1. Проверить, какие элементы HUD имеют absolute anchors/positions:
   - деньги/креды;
   - груз;
   - HP;
   - online/level;
   - programmator panel;
   - buttons row.

2. Свести HUD к зонам:
   - top-left status;
   - top/right actions;
   - bottom chat/input;
   - center overlays.

3. Не менять visual identity сразу. Сначала стабильность layout.

**Client files:**

- `client/Assets/Scripts/UI/GUIManager.cs`
- `client/Assets/Scenes/m1client.unity`
- relevant HUD prefabs embedded in scene.

**Exit criteria:**

- HUD не перекрывает chat/program panel на всех разрешениях матрицы.

## Phase 5. Building windows

Работы:

1. Перевести окна с ручного JSON на typed server builders.

2. На клиенте чинить только shell/layout, не индивидуальную бизнес-логику каждого
   окна.

3. Специальные окна:
   - Crafter recipe picker;
   - Market/auction tabs;
   - Storage crystal sliders;
   - UP skill tree;
   - Resp/Gun admin/fill pages.

**Exit criteria:**

- Все building окна открываются и закрываются одинаково.
- Длинные списки всегда скроллятся.
- Нет окна, которое на 1280x720 уходит за экран без возможности скролла.

## Phase 6. Visual refresh

Это делается только после layout-стабилизации.

Допустимые изменения:

- единый spacing scale;
- единый button height;
- consistent hover/disabled states;
- читаемый font sizing;
- меньше случайных fixed colors;
- современные scrollbars/buttons, но без смены логики.

Не делать на этом этапе:

- новый дизайн с нуля;
- замену всего GUI фреймворка;
- переписывание сетевого контракта.

## Что чинить клиентом, а что сервером

Клиентом:

- resolution/layout;
- ScrollRect visibility;
- confirmation logic, если сервер не может компенсировать;
- focus/keyboard issues;
- prefab anchors/constraints;
- tool surface scaling.

Сервером:

- неверный HORB JSON;
- отсутствующие TY handlers;
- неверный program source format;
- ошибки state-machine;
- user-visible protocol errors.

## Первый рабочий вертикальный срез

1. Добавить UI test checklist.
2. Проверить HORB shell на 1280x720/1920x1080/3440x1440.
3. Точечно исправить PopupManager constraints, если окно выходит за экран.
4. Прогнать compile check.
5. Затем переходить к программатору.

## Риски

- Unity scene YAML плохо редактировать вручную. Предпочтение: править scripts
  и prefabs точечно; сцену трогать только для Canvas/ScrollRect/anchors, когда
  это невозможно кодом.
- Client UI может зависеть от старых magic numbers. Любую замену magic number
  проверять на старом 1920x1080 сценарии.
- Изменение CanvasScaler влияет на весь клиент. Его не трогать первым, потому
  что уже есть `DisplayScale`.
