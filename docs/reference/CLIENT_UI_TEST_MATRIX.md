# Client UI test matrix

Цель: каждая правка интерфейса должна проверяться не только compile gate, но и
ручной визуальной матрицей. Это особенно важно для Unity UI: часть дефектов
видна только на конкретном aspect ratio, DPI или размере окна.

## Resolution set

Минимальный набор:

- 1280x720
- 1366x768
- 1600x900
- 1920x1080
- 2560x1440
- 3440x1440
- 1280x1024
- 1024x768

## Baseline status

До полноценного ручного прохода статус большинства экранов остаётся `unknown`.
Это намеренно: audit показывает risk points, но не заменяет визуальную проверку.

| Экран | 1280x720 | 1366x768 | 1920x1080 | 1024x768 | Статус |
| --- | --- | --- | --- | --- | --- |
| HORB popup shell | needs pass | needs pass | needs pass | needs pass | code clamp present |
| Chat | needs pass | needs pass | needs pass | needs pass | flicker fix present |
| Programmator | needs pass | needs pass | needs pass | needs pass | protocol fixes present |
| HUD | unknown | unknown | unknown | unknown | evidence needed |
| Inventory/cargo | unknown | unknown | unknown | unknown | evidence needed |
| Building GUI | unknown | unknown | unknown | unknown | evidence needed |
| Map | unknown | unknown | unknown | unknown | evidence needed |
| Settings/help | needs pass | needs pass | needs pass | needs pass | HORB-dependent |

Status meanings:

- `unknown`: no current visual evidence.
- `needs pass`: must be manually checked before visual-complete.
- `code fix present`: code-side fix exists, visual pass still required.
- `blocked`: cannot be verified because screen/action cannot be reached.
- `pass`: checked and acceptable.
- `fail`: reproducible defect, must link to backlog item.

## Manual capture format

For every checked screen/resolution, record:

```text
Screen:
Resolution:
Action path:
Expected:
Actual:
Status: pass | fail | blocked
Evidence: screenshot/log/manual note
Likely source:
Next action:
```

## Phase 1: Popup/HORB shell

Проверяемые окна:

- market / auction;
- crafter / storage / gun / respawn;
- settings;
- help / wiki style rich text;
- building GUI with inventory grid;
- big input HORB;
- long `list`, `richList`, `clanlist`, `%%` rich text.

Pass criteria:

- окно целиком остаётся внутри root canvas;
- заголовок, back/exit/buttons видны;
- длинные списки скроллятся, а не растягивают окно за экран;
- big input остаётся редактируемым и не перекрывает кнопки;
- server `css` продолжает работать, но не может сделать окно выше экрана;
- на 1920x1080 обычные окна визуально не меняются без причины.

Known client clamp:

- `PopupManager.ShowHORB` после сборки окна делает layout rebuild, затем
  уменьшает активные `scrollView`, big input и `canvasGUI`, если итоговый
  `GUIWindow` выше root canvas с safe margin.
- Wire format не изменён: `HORBConfig`, `buttons`, `tabs`, `list`, `richList`,
  `canvas`, `css` остаются совместимыми.

## Phase 2: Chat

Проверяемые сценарии:

- открыть чат без сетевого reconnect;
- много сообщений подряд;
- переключение каналов;
- reconnect/resync истории;
- resize окна с открытым чатом.

Pass criteria:

- scrollbar не мигает в idle-состоянии;
- input field не перекрывает HUD;
- reconnect не создаёт визуальные дубли;
- layout не перескакивает при новой строке.

Known client fix:

- `ChatManager.ChatScroll` в `m1client.unity` должен иметь
  `m_VerticalScrollbarVisibility: 1` (`AutoHide`), а не `2`
  (`AutoHideAndExpandViewport`).
- `ChatManager.ScrollDown` / `ForcedScrollDown` должны двигать scrollbar только
  при реальном overflow `Content > Viewport`; без overflow scrollbar скрывается
  через `CanvasGroup`, без отключения GameObject.
- При появлении scrollbar не должен мигать: chat scrollbar transition должен
  быть отключён runtime-настройкой, а повторный visible-state не должен заново
  применять alpha/interactable.

## Phase 2.5: DisplayScale / DPI

Проверяемые сценарии:

- старт клиента на одном и том же разрешении с разными физическими экранами;
- resize окна после старта;
- `Ctrl/Cmd +/-/0` для HUD;
- `Ctrl/Cmd + Shift +/-/0` для мира.

Pass criteria:

- одинаковое разрешение даёт предсказуемо похожий HUD, если `ui_scale` не
  менялся вручную;
- `[DisplayScale] ... dpi=... density=...` не показывает неожиданный boost на
  desktop/windowed окружении;
- ручные `ui_scale` и `world_zoom` остаются рабочим escape hatch и сохраняются
  per-device.

Known risk:

- Desktop/laptop `DisplayScale` снова учитывает `Screen.dpi`, но с отдельным
  cap `2.05x` вместо mobile cap `2.2x`. Правка всё равно должна пройти visual
  matrix, потому что она меняет не одно окно, а весь HUD и размер видимого мира
  на desktop. На macOS с `Screen.dpi=0` и `short side <= 2240` должен сработать
  fallback, который выводит effective DPI из предполагаемой физической короткой
  стороны MacBook.

Static audit:

- `python3 tools/ui_layout_audit.py --matrix-only --scale-matrix`
- `python3 tools/ui_layout_audit.py --matrix-only --fit-matrix "GUIWindow|ProgrammatorWindow"`
- `python3 tools/ui_layout_audit.py --matrix-only --horb-risk`
- Для desktop matrix `DensityBoost` не должен превышать `2.05` для всех значений
  `DPI`. Если audit снова покажет desktop `DensityBoost` около mobile cap `2.2`,
  это regression-risk.
- После правки `DisplayScale` сохранить значения `Canvas reference` и
  `World tile px` для `dpi=0`, `dpi=110`, `dpi=160`, `dpi=220`; они должны быть
  bounded by desktop cap `2.05`, а ручной `ui_scale`/`world_zoom` остаётся
  escape hatch.
- В live-log `[DisplayScale] ... dpi=... effectiveDpi=... density=...` на
  MacBook/laptop-like macOS с `dpi=0` ожидается `density≈1.56`; если там `1.00`,
  fallback не сработал, если `2.05`, logical scaled resolution снова раздуло HUD.
- Для logical `1512x982` и Retina backing-size `3024x1964` при `macbook dpi=0`
  static matrix должна показывать `DensityBoost≈1.56` в обоих случаях; это
  проверяет M3/custom-resolution сценарий, где старая fixed-`220dpi` логика
  давала скачок `2.05 -> 1.56`.
- Static fit result на текущем коде: `GUIWindow` и `ProgrammatorWindow` не
  доказывают offscreen-баг сами по себе. На `1024x768` при `dpi=160/220`
  `GUIWindow` примерно `662x557`, `ProgrammatorWindow` примерно `617x468`.
  На MacBook-like `1512x982` при `macbook dpi=0` fallback `GUIWindow` примерно
  `645x542`, `ProgrammatorWindow` примерно `600x455`; на backing `3024x1964`
  screen-pixel размеры примерно вдвое больше, но физически соответствуют тому
  же `DensityBoost≈1.56`.
  Поэтому live pass должен искать не только "помещается/не помещается", но и
  перекрытие HUD/chat, доступность кнопок, внутренние scroll areas и читаемость.
- HORB dynamic-content baseline: `manual_horb_json=1` только в typed builder
  emit, `plain_text_format=0`, `rich_no_scroll=0`. Любой новый ручной `horb:`
  payload, рост `plain_text_format` или рост `rich_no_scroll` должен попасть в
  backlog перед client/server правкой.

## Phase 3: Programmator

Проверяемые сценарии:

- открыть список программ;
- создать новую программу;
- открыть существующую через `#P`;
- сохранить и запустить через `PROG -> Gu/optional @T/@P/BH`;
- stop/reset через `pRST`;
- copy через `PCOP`;
- rename/delete;
- выход в меню с unsaved changes.

Pass criteria:

- основной 16x12 tool surface доступен на 1366x768 и 1024x768;
- кнопки start/copy/rename/delete/help/menu не уезжают за экран;
- help panel scrollable;
- confirmation unsaved changes не выполняет выход до подтверждения;
- server state и client state не расходятся после save/start/stop.

Known client fix:

- `ProgrammerManager.OnMenuButton` должен возвращаться сразу после показа
  confirmation при `ProgrammerView.unsaved`. В меню можно выходить только через
  OK callback.
- `PCOP` должен возвращать список программ с новой копией; чужой `programId`
  не должен копироваться.
- `PREN`/rename должен возвращать `#p`, потому что это update выбранной
  программы.
- malformed `PROG`, где compiled-length выходит за payload, должен отклоняться
  и не должен сохранять пустой source.

Layout evidence:

- `ProgrammerView` grid создаётся как `16x12` с шагом `32f`.
- `ProgrammatorWindow`: `anchorMin/Max {x: 0.5, y: 0.5}`,
  `sizeDelta {x: 564.3, y: 428}`.
- `ProgView`: `anchorMin/Max {x: 0, y: 1}`, `sizeDelta {x: 520.3, y: 390}`.
- `ProgrammerView` инстанцирует сетку через `localPosition` с шагом `32f`, а
  hit-test считает координаты тем же шагом. Любой scale/anchor change должен
  сохранять это соответствие.
- Для повторной проверки:
  `python3 tools/ui_layout_audit.py --object "Prog|Program|HelpPanel"`.
- Для статического fit:
  `python3 tools/ui_layout_audit.py --matrix-only --fit-matrix "ProgrammatorWindow|ProgView"`.

## Required checks per client UI patch

- `M3_CLIENT_DIR=client ./client/verify.sh --list`
- `python3 tools/ui_layout_audit.py`
- `python3 tools/ui_layout_audit.py --script-usage` для client C# targets
- manual pass/fail по затронутому phase на минимум четырёх размерах:
  `1280x720`, `1366x768`, `1920x1080`, `1024x768`.

Script usage note:

- `GlobalChatManager.cs` сейчас не найден в scene/prefab references. Не
  использовать его как доказательство активного UI-дефекта без live path.
