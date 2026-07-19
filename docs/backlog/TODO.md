# TODO

> [!WARNING]
> Это product/parity/tooling backlog, а не текущий server migration handoff.
> Порядок архитектурной работы и следующий обязательный срез находятся только в
> `SERVER_MIGRATION_STATUS.md`. Датированные разделы ниже сохраняют историю и не
> могут переопределять checkpoint.

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
- Building/admin scenario-smoke: `scripts/dev-smoke.sh` проверяет
  `Blds -> open_buildings -> bld_place:O -> ADMN` и базовый HORB wire-contract
  для списка построек, постановки Spot и админ-кнопки.
- GUI/Wire Codex: начальный машинно-читаемый реестр добавлен в
  `docs/reference/GUI_WIRE_CODEX.md` для auth, programmator, toggles и common
  HORB routes.

Осталось:

- Scenario-smoke добить по оставшимся крупным GUI routes: auction/market HORB.
  Базовые building/admin routes (`Blds`, `open_buildings`, `bld_place:O`,
  `ADMN`), programmator routes (`openprog`, `PREN`, `rename`, `PDEL`, `PCOP`),
  `TAGR` и settings save уже закрыты.
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

Исторический product triage. Он не задаёт текущий порядок simulation migration.

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

## Исторический tickprof-трек

Первичная детализация `dispatch/schedule/side/unprofiled`, thread CPU/off-CPU,
schedule runs и side sections завершена и теперь находится в
`tasks/simulation/profiler.rs`. Fixed idle loop удалён.

Последние активные runtime-наблюдения и их интерпретация зафиксированы в
`SERVER_MIGRATION_STATUS.md`: `channel_chat` показал настоящий `201ms` CPU-bound
dispatch, а отдельный `SLOW hazards` имел микросекундный lookup и десятки
миллисекунд unaccounted/lock hold. Не выбирать следующий фикс по этому
историческому разделу и не увеличивать tick budget.

---

## Программатор: текущий статус и следующий аудит

Текущая серверная реализация: `crates/server/openmines-server/src/game/actors/programmator.rs`.

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
  Unity-клиентом и `docs/reference/server_reference/`, а не с устаревшими аудитами.
- Если добавляется новая намеренная девиация от C# — сразу заносить в
  `docs/DEVIATIONS.md`.

---

## Исторический assessment 2026-07-07

На тот момент самым больным считалось:

- **Скиллы**: самый большой доменный долг. Есть команда `/skill`, но нужен
  полный audit matrix `SkillType -> action hook -> exp hook -> UI sync`.
- **Programmator runtime**: GUI start/stop/reconnect закрыт smoke-ом, но runtime
  bytecodes/handmode/debug/actions всё ещё требуют прохода по клиенту и
  reference. Не считать готовым.
- **HORB/admin окна**: Codex начат, но не полный. Любой `IndexOutOfRange` в
  Unity popup почти наверняка означает неверную cardinality payload на сервере.
- **Tickprof side path**: исторический диагноз до event-driven owner и новых
  active-path логов; текущие evidence и приоритеты находятся в checkpoint.
- **Implicit defaults**: чистить только runtime defaults, которые маскируют
  повреждение config/DB/game state. Массовый grep по `unwrap_or` без доменной
  проверки запрещён.
- **Архитектура ECS**: отсутствие единого владельца остаётся реальным долгом, но
  порядок его миграции теперь задан `SERVER_MIGRATION_STATUS.md`.

---

## SRP-декомпозиция GameState (2026-07-19)

Вынесено 5 кластеров из god-object `game/mod.rs` (2370 → 2046 строк, -13.5%):

- **DueSchedules** (`game/kernel/due_schedules.rs`, 59 строк) — crafting/programmator/hazard
  due-расписания, 12 методов делегированы.
- **CommandIngress** (`game/kernel/command_ingress.rs`, 139 строк) — очередь команд,
  метрики, broadcasts, 14 методов делегированы.
- **PlayerRegistry** (`game/kernel/player_registry.rs`, 25 строк) — 5 DashMap
  реестров игроков (active/entities/chunk/bots_render/botspots).
- **BuildingIndex** (`game/kernel/building_index.rs`, 17 строк) — 4 DashMap
  индекса зданий/ботспотов.
- **WebSnapshotOwner** (`game/kernel/web_snapshot.rs`, 36 строк) — снепшот
  для веб-API (3 типа + update метод).

Итого: 21 поле + 3 struct definition вынесены из GameState. Все методы —
однострочные делегаты. `cargo check` — 0 ошибок.

**Дальше:**
- `commands/mod.rs` (2126 строк) — ядро роутинга команд, кандидат на sub-роутеры
  (admin, player_action, building_action).
- `contracts/mod.rs` (1371 строк) — команды/events/types, много boilerplate.
- `gui_buttons.rs` (1580 строк) — GUI-обработчики.
- `player_init.rs` (1368 строк) — логин/спавн, тестовые хелперы.
- `heal_inventory.rs` (1341 строк) — лечение/инвентарь/предметы.

## M6: Owned ECS — устранение глобального RwLock<EcsWorld>

**Приоритет: высокий.** Это корневая причина фризов и 100ms+ тиков.

**Диагноз:** `GameState` содержит `RwLock<EcsWorld>`. Все 8 schedule'ов,
каждый `modify_player` из network threads, dirty flush, admin/web — всё
конкурирует за один write lock. Preemption владельца блокирует независимую
работу.

**Целевая модель** (из `SIMULATION_KERNEL_PLAN.md`):
- Simulation thread владеет Bevy World по значению, без lock
- Session handlers → typed commands → bounded channel → simulation
- Admin/web читают immutable `ReadSnapshot`, не ECS
- Нет `ecs.read()`/`ecs.write()`/`modify_player`/`query_player` снаружи

**Что уже сделано (mitigation):**
- [x] Typed command pipeline (market, pack, teleport, up_building, crafter)
- [x] `&Outbox` → `&dyn PacketSink` (105/106 функций)
- [x] Wire-in-lock guard (0 violations)
- [x] Убран `refresh_bots_render_player_in_ecs` из `modify_player`
- [x] Батчинг `snapshot_dirty_players` по 16
- [x] Hazards/programmator: 10ms → 50ms

**Что нужно для M6:**
- [ ] Все session handlers → typed commands (остались: dig_build, clans, programmator, consumables, settings, heal_inventory)
- [ ] Убрать `modify_player` как публичный API — заменить на typed commands
- [ ] Убрать `query_player` как публичный API — заменить на read snapshots
- [ ] Admin/web → immutable `ReadSnapshot`
- [ ] Перенести Bevy World в runtime по значению
- [ ] Физически удалить `RwLock<EcsWorld>`

**Gate:** zero external ECS writers, нет `ecs_write_profiled`/`ecs_read_profiled`
вне simulation thread, `RwLock<EcsWorld>` удалён.

