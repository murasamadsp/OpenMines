# TODO

## Входящие баги от ручной проверки 2026-07-07

Порядок работы: сначала скиллы, затем программатор GUI, затем shutdown/HORB.

- `/skill`: закрыто коммитом `35f935f`. Команда принимает только wire/DB-код
  (`/skill me U 200 10 900000`), `/skill codes` показывает список из `SkillType`,
  результат сразу синкается пакетами `@S/LV/sp/@L/@B` и сохраняется в SQLite.
- Скиллы: многие скиллы не подключены к действиям, не накапливают опыт или
  имеют сомнительную формулу. Нужен отдельный аудит `SkillType` -> gameplay hook
  -> exp hook -> UI/packet sync. Текущий честный статус: `docs/SKILLS_STATUS.md`.
- Программатор GUI: регресс "после запуска остаётся/открывается редактор"
  закрыт текущим PROG wire-контрактом `Gu -> @P/BH -> #p`: `#p` обновляет source
  и закрывает Unity programmator object. Остаётся общий ручной аудит
  программатора по `docs/PROGRAMMATOR_GUI_PROTOCOL.md`.
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

Исходная проблема: лог вида
`OVER-BUDGET tick: total=32.760416ms dispatch=2.25µs schedule=1.002791ms side=31.7485ms actions=0`
показывает, что тик вышел за бюджет 10ms не из-за входящих TY-пакетов и не из-за ECS schedule, а из-за post-ECS side-effects.

Осталось:
- собрать реальные `tickprof`-логи с новым разбиением;
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
