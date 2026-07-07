# Programmator GUI Regression Postmortem

Дата: 2026-07-07.

## Симптом

После запуска программы Unity снова показывает окно редактора программатора.
Это повторялось несколько раз, потому что в репозитории одновременно жили два
несовместимых контракта:

- C# reference: `PROG` после запуска отправляет `#p`, затем `@P`;
- часть Rust-документации и тестов: успешный `PROG` не должен отправлять `#p`.

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

Неверная девиация убрала `#p` из успешного `PROG` path. После этого:

1. игрок нажимает play в редакторе;
2. клиент отправляет `PROG`;
3. сервер отвечает `Gu`, optional `@T`, `@P 1`, `BH`;
4. `#p` не приходит, поэтому Unity не вызывает `UpdateProgramm()`;
5. `ProgrammerView.active` остаётся `true`;
6. `@P 1` снова активирует programmator object;
7. визуально редактор остаётся/появляется поверх игры.

Отдельная регрессия: stopped `pRST` при `current_window == "prog"` открывал
`#P`. Это закреплял тест
`prst_from_open_program_list_reopens_selected_stopped_program`. Но Unity шлёт
stopped `pRST` как pre-open/reset сигнал из `GUIManager.OnProgButton()`, поэтому
сервер не должен сам открывать редактор на этом пакете.

## Правильный контракт

Успешный `PROG`:

```text
Gu -> optional @T -> #p -> @P -> BH
```

`#p` обязателен: это не “обновление ради UI”, а единственный legacy-клиентский
путь, который сбрасывает `ProgrammerView.active=false` перед `@P 1`.

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

