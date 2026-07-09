# TODO

## Dev-сахар / локальная проверка

Закрыто:

- Безопасность локальной разработки: глобально в Mac OS заблокировано использование деструктивных команд Git (`git reset`, `git clean`, `git restore`) через алиасы в `~/.gitconfig` во избежание случайной потери незакоммиченной работы.
- `openmines-server --doctor`: schema/resource + SQLite integrity/migration
  validation без запуска TCP/admin-сервера.
- Rust quality tooling: `cargo-deny`, `cargo-audit`, `cargo-machete`,
  `cargo-nextest`, `dev-smoke` подключены к tracked pre-commit/CI-контуру.
- Dev-run ergonomics: cargo aliases `check-server`, `doctor`, `server`,
  `test-fast`; `scripts/dev-run.sh` включает `sccache`, если он установлен.
- Tools hygiene: `docs/TOOLS_AUDIT.md`, `scripts/tools-audit.sh`, state/cache
  probe-файлы выведены из tracking и игнорируются.
- Базовый scenario-smoke: `scripts/dev-smoke.sh` проходит
  `connect -> auth-failure -> GUI register -> init packets -> PO/Xdig/Xmov` и
  проверяет, что сессия остаётся responsive.
- Programmator/reconnect scenario-smoke: `scripts/dev-smoke.sh` теперь проверяет
  `Pope -> createprog -> #P -> openprog -> rename -> PCOP -> PDEL -> PROG
  -> Gu/@T/@P/BH -> pRST -> Gu/@P/BH` и reconnect через сохранённый
  `AH`/legacy token. Критично: PROG-start и login/reconnect selected-программы
  не должны слать `#P`; `#p` должен идти последним после `@P/BH`.
- Settings/toggles scenario-smoke: `scripts/dev-smoke.sh` проверяет `TAGR -> BA`
  и `Sett -> save:%R% -> #S/GU`.
- GUI/Wire Codex: начальный машинно-читаемый реестр добавлен в
  `docs/reference/GUI_WIRE_CODEX.md` для auth, programmator, toggles и common
  HORB routes.

Осталось:

- Scenario-smoke добить по оставшимся крупным GUI routes: auction/building/admin
  HORB. Базовые programmator routes (`openprog`, `PREN`, `rename`, `PDEL`,
  `PCOP`), `TAGR` и settings save уже закрыты.
- GUI/Wire Codex расширить до полного покрытия HORB/admin/building/auction
  окон. Начальный реестр есть; теперь каждую GUI правку начинать с обновления
  строки в `docs/reference/GUI_WIRE_CODEX.md`.
- Live debug dashboard в админке: tickprof sections, queue sizes, dirty
  players/buildings/boxes, active programmators, schedule intervals, last save
  errors.
- Rust tooling: периодически запускать `cargo outdated`, `cargo geiger`,
  `cargo bloat` вручную и заносить реальные находки. Проверить быстрый linker
  (`mold`/`lld`) отдельным измеряемым срезом.
- Implicit defaults audit: запретить runtime-подстановки доменного состояния.
  Начато с fail-fast загрузки boxes/events; дальше разбирать `serde(default)` и
  `unwrap_or` только там, где это скрывает повреждение config/DB/game state.
- Config baseline cleanup: runtime config-структуры больше не реализуют
  `Default`, а загрузка `config.json` остаётся fail-fast без serde-подстановок.
  Убран тест, который требовал совпадения `configs/config.json` с кодовым
  baseline. Оставшийся `runtime_baseline()` — временная фабрика тестовых
  фикстур, не источник правды runtime.

## Входящие баги от ручной проверки 2026-07-07

Порядок работы: сначала скиллы, затем программатор GUI, затем shutdown/HORB.

- `/skill`: закрыто коммитом `35f935f`. Команда принимает только wire/DB-код
  (`/skill me U 200 10 900000`), `/skill codes` показывает список из `SkillType`,
  результат сразу синкается пакетами `@S/LV/sp/@L/@B` и сохраняется в SQLite.
- Скиллы: многие скиллы не подключены к действиям, не накапливают опыт или
  имеют сомнительную формулу. Нужен отдельный аудит `SkillType` -> gameplay hook
  -> exp hook -> UI/packet sync. Текущий честный статус: `docs/SKILLS_STATUS.md`.
- Программатор GUI: регресс "после запуска остаётся/открывается редактор"
  закрывается PROG wire-контрактом `Gu -> optional @T -> @P/BH -> #p`.
  `#P` на запуске запрещён; `#p` нужен последним, потому что `@P 1` включает
  `ProgrammatorWindow`, а `UpdateProgramm()` затем скрывает его.
  Остаётся общий ручной аудит программатора по
  `docs/reference/PROGRAMMATOR_GUI_PROTOCOL.md`.
- Программатор smoke: локально подтверждён сценарий
  create/open/rename/copy/delete/start/stop/reconnect. Это не доказывает весь
  runtime программатора, но теперь ломание базового GUI/start/reconnect будет
  ловиться без Unity.
- Shutdown: добавлены фазовые логи и таймауты коммитом `1671fdb`
  (`players/buildings` 5s, `box` 2s, `world.flush` 5s). `dev-smoke.sh`
  проходит. Если ручной `^C` снова зависнет, следующий лог обязан показать
  конкретную фазу-виновника.
- HORB popup: Unity падает в `PopupManager.ShowHORB` с
  `System.IndexOutOfRangeException` на popup handler. Проверить серверные HORB
  payload'ы, особенно окна программатора/админки; клиент не менять без отдельной
  явной задачи.
- `allow(dead_code)`: первый безопасный pass сделан. Нельзя чистить удалением
  “мёртвого” кода. Оставшиеся вхождения снимать только через понятное feature
  wiring: BotSpot programmator, skill hooks, programmator actions, provider/world
  boundaries, protocol HB packets.

---

## Tickprof: оптимизировать найденный `side` hot path

Статус: первичная детализация сделана. `server/src/tasks/lifecycle.rs` теперь
пишет в `tickprof` per-section timings для `side`-стадии: `broadcasts`,
`pack_resends`, `box_persist`, `cell_conversions`, `programmator_actions`, `death`,
`bots_render`.
Для `hazards` добавлен отдельный slow-log внутри
`standing_cell_hazard_system`: `players_scanned`, `active_cells`,
`fall_damage_hits`, `boxes_seen/taken`, `destructible_cells` и coarse timings
`lookup_time/fall_damage_time/box_time/destroy_time`. В `hazards` и `sand`
hot path убраны повторные `world.cell_defs()`/`CellDef::clone()` внутри частых
циклов; `CellDefs` теперь берётся один раз на запуск системы.

Исходная проблема: лог вида
`OVER-BUDGET tick: total=32.760416ms dispatch=2.25µs schedule=1.002791ms side=31.7485ms actions=0`
показывает, что тик вышел за бюджет 10ms не из-за входящих TY-пакетов и не из-за ECS schedule, а из-за post-ECS side-effects.

Осталось:
- собрать реальные `tickprof`-логи с новым разбиением;
- если снова всплывает `SLOW hazards system`, чинить конкретную секцию из
  нового лога, а не менять общий tick budget;
- подтвердить или исключить `bots_render` как источник пиков;
- оптимизировать конкретный hot path, а не увеличивать tick budget.

Критерий готовности: после живого лога есть конкретная секция-виновник и отдельный фикс этой секции.

---

# Программатор: текущий статус и следующий аудит

Текущая серверная реализация: `server/src/game/actors/programmator.rs`.

Актуальные проверенные факты:
- Unity text-format `#S/#E` мапится как `Start/Stop`.
- Direct actions (`Dig`/`Build*`/`Geology`/`Heal`/макросы копания) используют
  `gameplay.programmator.direct_action_delay_us`, сейчас 333333us.
- Задержки движения используют `gameplay.programmator.min_move_delay_ms`; штраф
  за ход в блок — `gameplay.programmator.blocked_move_penalty_ms`.
- Hand mode — bytecode `179/180`.
- Bytecode `162/163/164/165` — `BuildBlock`/`BuildPillar`/`BuildRoad`/
  `BuildMilitaryBlock`, это покрыто тестом
  `unity_hand_mode_bytecodes_map_to_hand_mode_actions`.

Осталось:
- Перед следующим изменением программатора сверять конкретный GUI/wire-сценарий с
  Unity-клиентом и `server_reference/`, а не с устаревшими аудитами.
- Если добавляется новая намеренная девиация от C# — сразу заносить в
  `docs/DEVIATIONS.md`.

---

## Assessment 2026-07-07

Самое больное сейчас:

- **Скиллы**: самый большой доменный долг. Есть команда `/skill`, но нужен
  полный audit matrix `SkillType -> action hook -> exp hook -> UI sync`.
- **Programmator runtime**: GUI start/stop/reconnect закрыт smoke-ом, но runtime
  bytecodes/handmode/debug/actions всё ещё требуют прохода по клиенту и
  reference. Не считать готовым.
- **HORB/admin окна**: Codex начат, но не полный. Любой `IndexOutOfRange` в
  Unity popup почти наверняка означает неверную cardinality payload на сервере.
- **Tickprof side path**: есть per-section логи, но нет реального виновника.
  Следующее действие — собрать живой over-budget лог и чинить конкретную секцию.
- **Implicit defaults**: чистить только runtime defaults, которые маскируют
  повреждение config/DB/game state. Массовый grep по `unwrap_or` без доменной
  проверки запрещён.
- **Архитектура ECS**: пока не трогать крупно. Реальная боль не в потоках, а в
  отсутствии единого владельца cell/building/durability/DB/cache state.
