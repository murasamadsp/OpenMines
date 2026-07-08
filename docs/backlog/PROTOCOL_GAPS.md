# Protocol Gaps Backlog

Дата: 2026-07-08.

Цель: использовать wire-протокол как главный индекс недоделанных фич. Если
клиент отправляет или ждёт пакет, а сервер не реализует полный contract, это
приоритетнее, чем механический поиск `dead_code`.

Источники:

- Unity client: `client/Assets/Scripts/Network/NetworkProtocol.cs`;
- текущий Rust dispatcher: `server/src/net/session/dispatch/ty.rs`;
- C# reference: `references/server_reference/Server/Session.cs` и packet files;
- протокол: `docs/PROTOCOL.md`.

## Client -> Server TY

| Event | Rust status | C# reference status | Work state |
|---|---|---|---|
| `Xmov` | implemented | implemented | live |
| `Xdig` | implemented | implemented | live |
| `Xbld` | implemented | implemented | live |
| `Xgeo` | implemented | implemented | live |
| `Xhea` | implemented | implemented | live |
| `GUI_` | implemented | implemented | live |
| `Locl` | implemented | implemented | live |
| `Whoi` | implemented | implemented | live |
| `TADG` | implemented | implemented | live |
| `TAGR` | implemented | reference stub | live, client-driven |
| `INVN` | implemented | implemented | live; toggles `IN show` mini vs `IN full` full |
| `INUS` | implemented | implemented | live |
| `INCL` | implemented | implemented | live |
| `DPBX` | implemented | implemented | partial GUI: no C# crystal sliders |
| `Sett` | implemented | implemented | partial GUI: simplified settings |
| `ADMN` | implemented | implemented | partial: only known building admin paths |
| `RESP` | implemented | implemented | live |
| `Clan` | implemented | implemented | partial clan feature |
| `Pope` | implemented | implemented | fragile: programmator GUI path is high-risk |
| `Blds` | implemented | implemented | live enough |
| `GDon` | implemented as daily bonus | C# donation/stub-like path | intentional repurpose |
| `Help` | explicit `OK` placeholder | decoded/no real handler | missing content |
| `PROG` | implemented | implemented | high-risk, keep testing |
| `PDEL` | implemented | implemented | live |
| `pRST` | implemented | implemented | live but high-risk with GUI state |
| `PREN` | implemented | implemented | live |
| `PCOP` | implemented | no C# Session case found | client-driven extension |
| `Chat` | implemented | implemented | live |
| `Chin` | implemented as resync | C# stub | client-driven extension |
| `Cset` | implemented | no C# Session case found | client-driven extension |
| `Cmen` | implemented | no C# Session case found | client-driven extension |
| `Choo` | implemented | no C# Session case found | client-driven extension |
| `Cpri` | implemented | no C# Session case found | client-driven extension |
| `Miso` | minimal `MM` hide via `openmines-protocol` helper | mission packet exists | missing missions |
| `THID` | telemetry-only no-op | tutorial packet exists | missing tutorial state |
| `Miss` | validated no-op | no Session case | accepts only `"0"`/`"1"`; keep no-op until mission system exists |
| `Rndm` | validated no-op | no Session case | accepts only `"hash=..."`; keep no-op unless auth/device hash needs it |
| `TAUR` | validated no-op | explicit C# empty handler | accepts only `"_"`; auto-respawn feature missing |
| `FINV` | explicit no-op | decoded, no C# Session case | Unity sends Alpha0..Alpha9 as payload `"0"`..`"9"`; server validates the payload and logs debug only. Do not implement filtering until UX contract is proven |
| `Xhur` | explicit no-op | decoded, no C# Session case | legacy self-hurt method exists in Unity but no live call site found; server validates `_` payload and logs debug only |

## Server -> Client

| Event | Rust status | Risk / missing work |
|---|---|---|
| `ST`, `AU`, `PI`, `PO`, `cf` | implemented | core handshake |
| `AE` | weak/rare | auth error path should be audited against client UI |
| `AH` | implemented in protocol, unclear live use | reconnection/auth-hash flow needs audit |
| `RC` | missing/unclear | reconnect notification not proven |
| `BI`, `@T`, `sp`, `@L`, `@S`, `@B`, `LV`, `P$` | implemented | core gameplay |
| `@t` | packet documented, live Rust source not proven | smooth teleport missing unless found |
| `NL`, `ON` | implemented enough | verify broadcast cadence later |
| `GU`, `Gu`, `OK`, `IN` | implemented | `IN show:{all}:{selected}:{grid}` and `IN full:{selected}:{grid}` are covered by wire/unit tests; GUI regressions remain high-risk |
| `GR` | packet exists in client/ref, Rust live source not proven | open-url feature missing |
| `cS`, `cH` | implemented | clan partial |
| `$$` | packet exists in client/ref, Rust live source not proven | purchase flow missing/unclear |
| `PM` | packet exists in client/ref, Rust live source not proven | modules feature missing |
| `@P`, `#P`, `#p`, `BH` | implemented | central risk: Unity `#P` opens editor, `#p` hydrates then hides editor, `@P 1` shows programmator object, `BH 1` enables hand-mode movement |
| `BC` | packet exists in client/ref, Rust live source not proven | bad-cells feature missing |
| `BA`, `BD` | implemented | aggression/autodig status |
| `BR` | missing | auto-respawn status missing with `TAUR` |
| `SP` | packet exists in client/ref, Rust live source not proven | state panel missing |
| `GE` | implemented with limited data | region/name source incomplete |
| `SU` | packet exists in client/ref, Rust live source not proven | ban-hammer/moderation UI missing |
| `BB` | implemented | sound signal path |
| `@R` | packet exists in client/ref, Rust live source not proven | respawn point UI missing |
| `GO` | packet exists in client/ref, Rust live source not proven | navigation arrow missing |
| `DR` | implemented | daily reward |
| `#F`, `#S` | implemented | config/settings |
| `MM` | `openmines-protocol` packet helper + minimal hide only | mission panel content missing |
| `MN` | `openmines-protocol` packet helper only | tutorial/mission notice missing |
| `MP` | `openmines-protocol` packet helper only | mission progress missing |
| `mO`, `mU`, `mL`, `mN`, `mC` | implemented enough | chat UX still needs live testing |
| `HB` tags `M`, `X`, `L`, `O`, `F`, `D`, `C`, `B`, `Z` | mostly implemented in protocol | tag `S`/`B` live usage should be audited before removing dead-code allows |

## Current Priorities

1. Programmator packets: keep `Pope`/`PROG`/`pRST`/`#P`/`#p` state-machine as
   the highest-risk protocol area. GUI regressions here are user-visible.
   Runtime variable semantics are also protected: JS `lastVariables` order,
   two-variable commands with readonly younger values, and JS-style division
   fallback are covered by Rust tests in `server/src/game/actors/programmator.rs`.
2. Auto-respawn: `TAUR` + `BR` are a real missing pair, but C# `Taur` is empty.
   Implement only after gameplay contract is defined.
3. Missions/tutorial: `Miss`/`Miso`/`THID` + `MM`/`MN`/`MP` are a whole missing
   subsystem. Current `Miso -> MM hide` is only a safe placeholder, but `MM`
   `MN`, and `MP` wire builders are now centralized in
   `openmines-protocol`.
4. Inventory hotkeys: `FINV` is decoded by C# packet layer but not handled by
   C# `Session`. Unity sends it from `ClientController.Update()` on numeric
   hotkeys only. Rust treats it as an explicit no-op with payload validation so
   it does not pollute `Unknown TY event` logs. Do not guess whether it means
   filter, quick-select, or legacy no-op.
5. Server-only outgoing packets (`GR`, `PM`, `BC`, `SP`, `SU`, `@R`, `GO`, `$$`)
   should be traced from C# feature owners before Rust implementation.

## Unity Client Evidence Notes

- `client/Assets/Scripts/Gameplay/ServerController.Handlers.cs`:
  - `#P` calls `GUIManager.OpenProgramm(...)` and leaves editor visible.
  - `#p` calls `GUIManager.UpdateProgramm(...)`; for real program ids it loads
    source, calls `Show()`, then hides `programmator` and sets
    `ProgrammerView.active=false`.
  - `@P "1"` sets `ProgPanel.playing=true`, `ClientController.isProgrammator=true`,
    and stops automove.
  - `BH "1"` only sets `ProgPanel.handMode=true`; movement gates check
    `(!isProgrammator || ProgPanel.handMode)`.
- `client/Assets/Scripts/UI/ProgPanel.cs`:
  - play/stop button sends `pRST` while `playing || handMode`;
  - otherwise it starts via `ProgrammerView.SendAndStartProgram()` or sends
    `Pope` only for `GUIManager.programToSend` links beginning with `@`.
- `client/Assets/Scripts/UI/InventoryPanel.cs`:
  - `IN show` expects `show:{all}:{selected}:{grid}`;
  - `IN full` expects `full:{selected}:{grid}`;
  - mini/full button sends `INVN`; item click sends `INCL` with item id.
- `client/Assets/Scripts/Gameplay/ClientController.cs`:
  - Alpha0..Alpha9 send `FINV` with payload `"0"`..`"9"` while no blocking UI is
    active.
  - `SelfHurt()` sends `Xhur` with payload `_`, but no live caller was found in
    current client scripts.
