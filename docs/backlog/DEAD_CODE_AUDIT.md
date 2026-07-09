# Dead Code / Allow Audit

Дата: 2026-07-08.

Цель: уменьшать `#[allow(dead_code)]` только там, где есть доказательство, что
это ложный scaffold или уже подключённый live-path. Не удалять reference-code и
не вводить фичи по одному имени.

## Сделано

- Удалён старый `handle_chat_message` wrapper: `Locl` уже идёт в
  `handle_local_chat`, `Chat` уже идёт в `handle_channel_chat`; отдельного
  клиентского события под wrapper нет.
- Удалён старый `handle_pack_action` wrapper: открытие паков идёт через movement
  proximity и `open_pack_gui` / `pack_op:*`; уникальная логика Resp уже
  перенесена в live `open_pack_gui`.
- Удалён `PlayerCooldowns.last_shot`: поле нигде не читалось, а gun logic в
  reference не является per-player cooldown.

## Осталось

| Area | Current state | Decision |
|---|---|---|
| `crates/openmines-server/src/game/logic/contracts.rs` | `PlayerCommand::{Connect,Disconnect,Ty}` live; остальные variants пока не подключены | Не удалять точечно. Нужен отдельный архитектурный проход: либо закончить command-bus, либо сузить enum до реально используемого API. |
| `crates/openmines-server/src/game/logic/contracts.rs` | `GameEvent` и `SaveCommand` пока не имеют live dispatcher/persistence worker | Не расширять искусственно. Либо довести event/save bus до runtime, либо удалить эти enums вместе с недоделанным contracts-layer решением. |
| `crates/openmines-shared/src/db/provider.rs` | `DatabaseProvider` целиком не используется runtime-кодом | Не расширять искусственно. Либо подключать как реальный persistence port, либо удалить после архитектурного решения. |
| `crates/openmines-server/src/game/actors/botspot.rs` | Spot entity/rendering live, programmator basket/execution не завершены | Не удалять: это незавершённая фича BotSpot programmator. Сначала закрыть `docs/backlog/BOTSPOT_PROGRAMMATOR.md`. |
| `crates/openmines-server/src/game/actors/programmator.rs::ActionType` | Enum intentionally wider than local call sites | Не удалять variants: это wire/reference surface программатора. |
| `crates/openmines-shared/src/world/cells.rs::cell_type` | Reference constants; часть используется генератором/механиками | Не удалять константы механически. |
| `crates/openmines-shared/src/world/anl.rs` | 1:1 port reference noise code with clippy/dead-code suppressions | Не трогать без dedicated generator parity audit. |
| `crates/openmines-shared/src/world/sectors_gen.rs` | Generated/reference-like worldgen fields | Не трогать без dedicated generator audit. |
| `crates/openmines-shared/src/world/sector_palette.rs::merged_palette_buf` | Alternative helper, not current reference behavior | Не подключать вслепую: может изменить generation distribution. |
