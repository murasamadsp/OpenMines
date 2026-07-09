# Sound / FMOD Contract

Дата: 2026-07-08.

## Client Contract

Unity не получает отдельный универсальный sound packet. Звук вызывается из:

- `BB` -> `SoundManager.PlayBibika()` -> sound id `1`;
- HB `F` FX -> `ClientController.AddFX(...)`;
- HB `D` directed FX -> `ClientController.AddDirectedFX(...)`;
- basket update `@B` локально может играть sound id `0`, если включён `ownSounds`.

`SoundManager` воспроизводит эффекты через FMOD backend по numeric ids:

| Id | FMOD event |
|---:|---|
| 0 | `event:/ui/basket` |
| 1 | `event:/ui/signal` |
| 2 | `event:/world/bomb` |
| 3 | `event:/world/bomb_tick` |
| 4 | `event:/player/death` |
| 5 | `event:/world/destroy` |
| 6 | `event:/world/emi` |
| 7 | `event:/player/geology` |
| 8 | `event:/player/heal` |
| 9 | `event:/player/hurt` |
| 10 | `event:/world/mining` |
| 11 | `event:/player/dizz` |
| 12 | `event:/world/tp_in` |
| 13 | `event:/world/tp_out` |
| 14 | `event:/world/volcano` |
| 15 | `event:/world/c190` |

Авторитетный машинный список этих путей: `docs/reference/FMOD_EVENTS.txt`.
`scripts/check-fmod-events.sh` проверяет и этот manifest, и ссылки в
`SoundManager.cs`, и содержимое `Master.strings.bank`.
Единая ручная команда: `scripts/quality-extra.sh fmod`.

## Server Wire Sources

| Client sound id | Client trigger | C# server evidence | Current Rust source/status |
|---:|---|---|---|
| 0 | `@B` basket update when `ownSounds` is enabled | basket sync path | player basket syncs (`@B`) |
| 1 | `BB` | packet exists as bibika/signal path | `bibika()` |
| 2 | HB `F` fx=4 | no C# server source found yet | not emitted by Rust; do not invent |
| 3 | HB `F` fx=3 | no C# server source found yet | not emitted by Rust; do not invent |
| 4 | HB `F` fx=2 | `Player.Death` -> `SendFXoBots(2, x, y)` | death FX |
| 5 | HB `F` fx=5 | no C# server source found yet | not emitted by Rust; do not invent |
| 6 | HB `F` fx=7 | no C# server source found yet | not emitted by Rust; do not invent |
| 7 | HB `F` fx=8 | C# `Geo()` only sends `GE`; no FX | not emitted by Rust; do not invent |
| 8 | HB `D` fx=5 | `Player.Heal` -> `SendDFToBots(5, 0, 0, id, 0)` | heal FX |
| 9 | HB `D` fx=6 | `Player.Hurt` -> `SendDFToBots(6, 0, 0, id, 0)` | hurt FX |
| 10 | HB `D` fx=0 / `AddBz` | `Player.Bz` / `BotSpot.Bz` -> `SendDFToBots(0, x, y, id, dir)` | dig/mining FX |
| 11 | HB `F` fx=6 | no C# server source found yet | not emitted by Rust; do not invent |
| 12 | HB `F` fx=10 | no C# server source found yet | not emitted by Rust; do not invent |
| 13 | HB `F` fx=11 | no C# server source found yet | not emitted by Rust; do not invent |
| 14 | HB `F` fx=24 | no C# server source found; likely client/local volcano visual path | not emitted by Rust; do not invent |
| 15 | HB `F` fx=25 | no C# server source found; C# C190 uses HB `D` fx=7 beam | not emitted by Rust; do not invent |

HB `D` fx=7 (`hb_gun_shot_fx`) is a visual gun beam; the client does not play a
dedicated FMOD sound id for it.

Important: C# boom/protector/razryadka use `Chunk.SendDirectedFx(1, x, y, dir, bid,
color)`, which reaches Unity as HB `D` fx=1 and calls `ClientController.AddBoom`.
That path is visual only in the current client and does not call `SoundManager`.
It is not equivalent to HB `F` fx=4 (`event:/world/bomb`) or HB `F` fx=3
(`event:/world/bomb_tick`).

## Evidence Ledger

- Unity `ClientController.AddFX`: HB `F` fx values 2,3,4,5,6,7,8,10,11,24,25
  can call FMOD sound ids 4,3,2,5,11,6,7,12,13,14,15 respectively.
- Unity `ClientController.AddDirectedFX`: HB `D` fx=0 calls `AddBz` and sound id
  10; fx=5 calls heal sound id 8; fx=6 calls hurt sound id 9; fx=7 is visual gun
  beam only.
- C# `PEntity.SendFXoBots` is HB `F`; `PEntity.SendDFToBots` and
  `Chunk.SendDirectedFx` are HB `D`.
- C# `Player.Geo()` calls `SendGeo()` only. Therefore geology sound id 7 is
  client-supported but not server-proven.
- C# `Player.tp()` sends `@T` and `SendMyMove()` only. Therefore tp sounds ids 12
  and 13 are client-supported but not server-proven.
- C# `IDamagable.SendBrokenEffect()` sends HB `F` fx=12 (`nohpfxPrefab`), which is
  visual and has no FMOD id in `SoundManager`.
- C# `ShitClass.C190Shot()` sends HB `D` fx=7 beam, not HB `F` fx=25. Therefore
  C190 sound id 15 is client-supported but not server-proven.
- C# `ShitClass.Boom/Prot/Raz` send HB `D` fx=1 with different `dir/color`, not HB
  `F` fx=3 or fx=4. The current client `AddBoom` method does not play sound.

## Rust / Client Coverage Checked

- `SoundManager.cs`: `PlaySound(id, volume)` and `PlayBibika()` resolve ids through
  exact `event:/...` FMOD paths and call `FMODUnity.RuntimeManager.CreateInstance`;
  there is no AudioClip fallback for effects.
- `BB`: `crates/openmines-protocol/src/packets.rs::bibika`; live use in
  programmator debug/message action path.
- `@B`: `basket(...)` is used by player init, dig pickup, boxes, GUI item flows,
  pack fill, and combat box pickup.
- Death sound path: `crates/openmines-server/src/net/session/play/death.rs::run_death_broadcasts`
  sends HB `F` fx=2.
- Heal sound path: `crates/openmines-server/src/net/session/ui/heal_inventory.rs::handle_heal`
  sends HB `D` fx=5.
- Hurt sound path: `crates/openmines-server/src/net/session/play/death.rs::hurt_player_pure`,
  `crates/openmines-server/src/net/session/ui/heal_inventory.rs`, and
  `crates/openmines-server/src/game/mechanics/combat.rs` send HB `D` fx=6 for surviving hurt.
- Dig/mining sound path: `crates/openmines-server/src/net/session/play/dig_build.rs` sends HB `D`
  fx=0 for dig and HB `D` fx=2 for crystal amount visual. Only HB `D` fx=0 maps
  to sound id 10 in the client.
- Gun/C190 beam path: Rust uses HB `D` fx=7 like C#; this is visual-only in the
  current client and must not be counted as FMOD sound coverage.

## Current Blocker

`scripts/check-fmod-events.sh` currently fails:

```text
missing FMOD event: all 16 expected event:/... paths
bank size: 724 bytes
FMOD metadata/cache matching expected event paths: 0/16
```

So FMOD is not complete yet: `SoundManager.cs` is wired to 16 FMOD events, but
the checked-in/generated `Master.strings.bank` and FMOD project metadata do not
prove that any of them exist. Before calling FMOD done, build or author real
FMOD events with those exact paths and make `scripts/check-fmod-events.sh` pass.
`PRE_COMMIT_EXTENDED=1 scripts/pre-commit.sh` также включает этот FMOD gate.

CLI check on 2026-07-08:

- `/Applications/FMOD Studio.app/Contents/MacOS/fmodstudiocl` is a wrapper around
  macOS `open -W ... --stdout $(tty) --stderr $(tty)`. From Codex/non-TTY shell it
  resolves `tty` to bad path fragments and prints `/a` / `/tty` missing-file
  errors instead of building banks.
- Direct
  `/Applications/FMOD Studio.app/Contents/MacOS/fmodstudio -build client/FMODProject/FMODProject.fspro`
  reached 100% CPU and produced no stdout/stderr for ~90 seconds, so it was
  terminated. Do not call FMOD complete from CLI evidence until a deterministic
  bank build path is proven.
