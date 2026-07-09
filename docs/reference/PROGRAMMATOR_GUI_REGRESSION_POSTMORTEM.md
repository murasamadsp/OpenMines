# Programmator GUI Regression Postmortem

Дата: 2026-07-07.

## Симптом

После запуска программы Unity снова показывает окно редактора программатора.
Это повторялось несколько раз, потому что сервер и документы считали `#p`
"тихим" обновлением selected-программы, хотя клиентский исходник показывает
обратное.

- `#P` открывает editor path;
- `#p` тоже проходит через editor path;
- `@P 1` включает режим running-программатора.

## Что делает клиент

Клиентские handlers:

- `#P` -> `GUIManager.OpenProgramm(...)`: открывает редактор и оставляет editor
  path активным;
- `#p` -> `GUIManager.UpdateProgramm(...)`: загружает source, вызывает
  `ProgrammerView.Show()`, затем делает `programmator.SetActive(false)` и
  `ProgrammerView.active=false`;
- `@P 1` -> `GUIManager.ChangeProgTo(true)`, `ProgPanel.playing=true`,
  `ClientController.isProgrammator=true`.

`ProgrammerView.SendAndStartProgram()` только отправляет `PROG`. Он не является
доказательством, что сервер должен гидратить редактор через `#p`: live wire
контракт берётся из handlers, а `#p` там вызывает `Show()`.

## Реальная причина

Неверная девиация вернула `#p` в успешный `PROG` path. После этого:

1. игрок нажимает play в редакторе;
2. клиент отправляет `PROG`;
3. сервер отвечает `Gu`, optional `@T`, `#p` и/или `#P`, затем `@P/BH`;
4. `#p`/`#P` вызывает `ProgrammerView.Show()`;
5. `ProgrammerView.active` временно блокирует движение и может оставить editor
   state видимым/открытым;
6. визуально редактор появляется поверх игры или моргает при запуске.

Отдельная регрессия: stopped `pRST` при `current_window == "prog"` открывал
`#P`. Это закреплял тест
`prst_from_open_program_list_reopens_selected_stopped_program`. Но Unity шлёт
stopped `pRST` как pre-open/reset сигнал из `GUIManager.OnProgButton()`, поэтому
сервер не должен сам открывать редактор на этом пакете.

## Правильный контракт

Успешный `PROG`:

```text
Gu -> optional @T -> @P 1 -> BH 0 -> #p
```

`#P` запрещён на старте программы. `#p` нужен последним после `@P/BH`: `@P 1`
включает `ProgrammatorWindow`, а `UpdateProgramm()` в конце выключает его.

Stopped `pRST`:

```text
wire-silent
```

Running `pRST`:

```text
Gu -> @P 0 -> BH 0
```

## Что запрещено

- Успешный `PROG` с `#P`.
- Успешный `PROG` с `#p` до `@P/BH`.
- Login/reconnect running-программы с `#P`.
- Stopped `pRST` с `#P`.
- Документировать девиацию от C# без live-проверки Unity и без строки в
  `docs/DEVIATIONS.md`.

## Изменённые источники правды

- `docs/PROTOCOL.md`
- `docs/reference/PROGRAMMATOR_GUI_PROTOCOL.md`
- `docs/reference/GUI_WIRE_CODEX.md`
- `docs/reference/PROGRAMMATOR_GUI_REGRESSION_POSTMORTEM.md`
