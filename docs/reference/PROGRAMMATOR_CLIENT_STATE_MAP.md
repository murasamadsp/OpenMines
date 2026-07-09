# Programmator Client State Map

Дата: 2026-07-09.

Источник правды: Unity client source in `client/Assets/Scripts` and scene
bindings in `client/Assets/Scenes/m1client.unity`.

## Static Bindings

- `GUIManager.programmator` points to scene object `ProgrammatorWindow`
  (`m1client.unity`, fileID `1790376288100402007`).
- `ProgrammatorWindow` has `ProgrammerManager` on the same GameObject and
  contains the editor children.
- `ProgPanel` is a separate always-active HUD object. Its play/stop button calls
  `ProgPanel.OnPlayStop()`.

## Client Mutable State

| Field | Owner | Meaning |
| --- | --- | --- |
| `GUIManager.programToSend` | `GUIManager` static | special payload for opening program menu; `"_"` means normal selected program path |
| `ProgrammerView.opened` | `ProgrammerView` static | editor has been initialized/shown at least once |
| `ProgrammerView.active` | `ProgrammerView` static | editor input mode is active; movement is blocked while true |
| `ProgPanel.playing` | `ProgPanel` static | HUD shows stop/animation; set by `@P` |
| `ProgPanel.handMode` | `ProgPanel` static | manual movement allowed while programmator is running |
| `ClientController.isProgrammator` | `ClientController` | movement gate; if true, manual movement needs `handMode` |

## Server Packets

### `#P`

Handler:

```text
ServerController.ProgrammatorOpenHandler
-> GUIManager.OpenProgramm(id,title,source)
```

For `id != -1`:

- `ProgrammatorWindow.SetActive(true)`;
- `ProgrammerView.active = true`;
- `ProgrammerView.programId/title` are set;
- `GUIManager.programToSend = "_"`;
- source is loaded/cleared;
- `ProgrammerView.Show()` sets `ProgrammerView.opened = true`.

Effect: opens editor and keeps it visible/active.

For `id == -1`:

- only title fields and `programToSend = source` are changed;
- editor is not opened.

### `#p`

Handler:

```text
ServerController.ProgrammatorUpdateHandler
-> GUIManager.UpdateProgramm(id,title,source)
```

For `id != -1`:

- `ProgrammatorWindow.SetActive(true)`;
- `ProgrammerView.active = true`;
- `ProgrammerView.programId/title` are set;
- `GUIManager.programToSend = "_"`;
- source is loaded/cleared;
- `ProgrammerView.Show()` sets `ProgrammerView.opened = true`;
- then `ProgrammatorWindow.SetActive(false)`;
- then `ProgrammerView.active = false`.

Effect: hydrates selected program and ends by hiding the editor object. Because
it still calls `Show()`, it must not be treated as harmless, but it is the only
observed client path that both hydrates selected program state and hides
`ProgrammatorWindow` after `@P 1`.

For `id == -1`:

- only title fields and `programToSend = source` are changed.

### `@P`

Handler:

```text
ServerController.ProgrammatorHandler
```

For payload `"1"`:

- `GUIManager.ChangeProgTo(true)`;
- `RobotRenderer.isProgrammator = true`;
- `ClientController.isProgrammator = true`;
- `ProgPanel.playing = true`;
- `ClientController.stopAutoMove()`.

Important: `ChangeProgTo(true)` calls `ProgrammatorWindow.SetActive(true)`.
Therefore `@P 1` alone opens the `ProgrammatorWindow` GameObject.

For payload not `"1"`:

- `GUIManager.ChangeProgTo(false)`;
- `RobotRenderer.isProgrammator = false`;
- `ClientController.isProgrammator = false`;
- `ClientController.TimeSync()`;
- `ProgPanel.playing = false`.

### `BH`

Handler:

```text
ServerController.HandModeHandler
-> ProgPanel.handMode = (msg == "1")
```

It does not open/close editor UI. It only changes the movement gate and hand-mode
indicator.

### `Gu`

Handler:

```text
ServerController.PopupCloseHandler
```

It closes HORB/popup UI. It does not close `ProgrammatorWindow` and does not
change `ProgrammerView.active/opened`.

## Client-Initiated Flows

### HUD programmator button: `GUIManager.OnProgButton()`

When `ProgrammatorWindow` is inactive:

1. sends `pRST` as a pre-open/reset signal;
2. if `ProgrammerView.opened == false`, sends `Pope` with
   `GUIManager.programToSend` and returns;
3. otherwise activates `ProgrammatorWindow`, sets `ProgrammerView.active = true`,
   and calls `ProgrammerView.Show()`.

Implication: stopped `pRST` must not emit visible editor packets. It is often
just a client pre-open signal.

### HUD play/stop button: `ProgPanel.OnPlayStop()`

1. If `ProgPanel.playing || ProgPanel.handMode`, calls
   `GUIManager.OnProgCloseButton()` -> sends `pRST`.
2. Else if `GUIManager.programToSend.StartsWith("@")`, sends `Pope` with
   `"@" + programToSend` and returns.
3. Else if `ProgrammerView.THIS != null`, calls
   `ProgrammerView.SendAndStartProgram()` -> sends `PROG`.

Implication: if selected program is not hydrated into `ProgrammerView.programId`
and `GUIManager.programToSend`, the HUD play button can open a list/menu instead
of starting the intended program.

### Editor start button: `ProgrammerManager.OnStartButton()`

Before sending `PROG`, client does:

- `base.gameObject.SetActive(false)` on `ProgrammatorWindow`;
- `ProgrammerView.active = false`;
- `ProgrammerView.unsaved = false`;
- resets title text;
- calls `ProgrammerView.SendAndStartProgram()`.

Implication: this path closes editor locally before `PROG`, but `@P 1` from
server will reopen `ProgrammatorWindow` unless a later packet hides it again.

## Movement Gates

Manual movement and map goto are blocked when:

- `ProgrammerView.active == true`;
- or `ClientController.isProgrammator == true && ProgPanel.handMode == false`.

Relevant client checks:

- `ClientController.FromMapGoto`;
- `ClientController.NoGUIClick`;
- main keyboard movement block in `ClientController.Update`.

## Derived Wire Rules

1. `#P` is only for explicit editor open/create/open-program flows.
2. `#p` is an editor update packet, but for `id != -1` it ends by hiding
   `ProgrammatorWindow`.
3. `@P 1` opens `ProgrammatorWindow` by itself.
4. Therefore a successful `PROG` start cannot end at `@P 1/BH`; it leaves the
   window open.
5. Successful running `PROG` needs:

```text
Gu -> optional @T -> @P 1 -> BH <mode> -> #p {id,title,source}
```

`#p` must be after `@P 1`, not before it.

6. Login/reconnect with a selected/running program needs the same principle:
   send `@P/BH` for mode, then `#p` for selected-program hydration and final
   editor hide.
7. Stopped pre-open `pRST` should remain wire-silent for editor packets.

