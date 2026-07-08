# Programmator GUI Regression Postmortem

Дата: 2026-07-07.

## Симптом

После запуска программы Unity снова показывает окно редактора программатора.
Это повторялось несколько раз, потому что в репозитории одновременно жили два
несовместимых контракта:

- ранняя трактовка C# reference: `PROG` после запуска отправляет `#p`, затем `@P`;
- фактический Unity-контракт: `@P 1` сам показывает programmator object, значит
  `#p` должен прийти последним и спрятать editor view.

## Что делает клиент

Клиентские handlers:

- `#P` -> `GUIManager.OpenProgramm(...)`: открывает редактор и оставляет его
  открытым;
- `#p` -> `GUIManager.UpdateProgramm(...)`: загружает source, затем делает
  `programmator.SetActive(false)` и `ProgrammerView.active=false`;
- `@P 1` -> `GUIManager.ChangeProgTo(true)`, `ProgPanel.playing=true`,
  `ClientController.isProgrammator=true`.

`ProgrammerView.SendAndStartProgram()` только отправляет `PROG`. Он не закрывает
редактор локально. Поэтому закрытие editor-state обязано прийти от сервера.

## Реальная причина

Неверная девиация сначала убрала `#p` из успешного `PROG` path, затем вернула
его в неправильном порядке (`#p` перед `@P 1`). После этого:

1. игрок нажимает play в редакторе;
2. клиент отправляет `PROG`;
3. сервер отвечает `Gu`, optional `@T`, `#p`, `@P 1`, `BH`;
4. `#p` вызывает `UpdateProgramm()` и прячет editor view;
5. `@P 1` приходит после этого и снова активирует programmator object;
6. визуально редактор появляется поверх игры.

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

`#p` обязателен последним: это не “обновление ради UI”, а единственный
legacy-клиентский путь, который после `@P 1` сбрасывает
`ProgrammerView.active=false` и скрывает editor view.

Stopped `pRST`:

```text
wire-silent
```

Running `pRST`:

```text
Gu -> @P 0 -> BH 0
```

## Что запрещено

- Успешный `PROG` без `#p`.
- Успешный `PROG` с `#P`.
- Stopped `pRST` с `#P`.
- Документировать девиацию от C# без live-проверки Unity и без строки в
  `docs/DEVIATIONS.md`.

## Изменённые источники правды

- `docs/PROTOCOL.md`
- `docs/reference/PROGRAMMATOR_GUI_PROTOCOL.md`
- `docs/reference/GUI_WIRE_CODEX.md`
- `docs/reference/PROGRAMMATOR_GUI_REGRESSION_POSTMORTEM.md`
