# js_reference — разбор `fodinae.online` (клиент "testG")

Зеркало браузерного JS-клиента с `https://fodinae.online/` + разбор. Скачано
2026-06-23 (23 файла, ~7250 строк кода + карта 4.9 МБ).

## ⚠️ Что это и чем НЕ является

Это **оффлайн-песочница / прототип механик** на чистом JS+Canvas (не Unity).
`main.js` спавнит ботов локально (ИИ-программатор), сети нет, предметы выдаются
случайно (`GenerateRandomItems`), мир — захардкоженный `TestWorld0` 2000×2000.

**Авторитетно как референс для:** игровой модели — таблицы блоков (hardness/
урон/падение), 58 скиллов + формулы, 51 предмет, геофизика (песок/валуны/живки),
футпринты зданий, **полная таблица 111 опкодов программатора** (главный пробел
OpenMines).

**НЕ авторитетно для:** wire-протокола (его тут нет) и точных серверных констант.
Расхождения с `docs/reference/server_reference/` (C#) и `crates/openmines-server/` (Rust) ожидаемы — напр. кулдаун
копания тут `333мс`, а у нас `200мс` (CLAUDE.md). При конфликте источник правды
для протокола — `docs/PROTOCOL.md` и C#, а не этот прототип. Этот код ценен
поведением и числами, а не сетью.

## Карта файлов (порядок загрузки из `index.html`)

| Файл | Строк | Назначение |
|------|------|------------|
| `scripts/tables/MapTables.js` | 94 | `ColorTable` — RGB миникарты по ID блока (+ тёмная версия) |
| `scripts/tables/DataTables.js` | 531 | **Ядро данных**: `Block` enum, `BlockStats`, `SkillType`(58), `SkillCalculator` (формулы effect/cost), ники-генератор |
| `scripts/tables/minestileset.js` | 1997 | Атлас спрайтов (`WorldTilesetRaw.frames`: x/y/w/h в тайлсете по ID) |
| `scripts/tables/items.js` | 78 | `ItemCodes` 0..50, пути к иконкам |
| `scripts/usefuladitions.js` | 195 | Утилиты: `Vector2d`, `RandInt`, `RandColor16`… |
| `scripts/cargo.js` | 128 | `CryCargo`(6 кри), `BotCargo`(стек геологии), `BotItemInventory` |
| `scripts/entitys/entity.js` | 161 | `Pack` (база зданий), `PackConstruction.GetBlockGrid` (парсер ASCII-футпринта) |
| `scripts/entitys/packs.js` | 252 | 10 типов зданий + `PacksContainer` (чанковое хранилище) |
| `scripts/userGUI/guiControls.js` | 262 | GUI-виджеты (бары, панели) |
| `maps/test.js` | — | `TestWorld0` — RLE-карта 2000×2000 (см. формат ниже) |
| `scripts/wmap.js` | 217 | `WorldMap` — слои terrain/background/blocksMeta, density, RLE-загрузка |
| `scripts/geophys.js` | 307 | `GeoPhisics` — падение песка/валунов + рост живок (7 типов) |
| `scripts/programmator.js` | 1160 | **Программатор**: 111 опкодов, исполнитель, переменные, стек |
| `scripts/bot.js` | 742 | `Bot` (действия), `BotSkillEffects`, кулдауны, `BotsContainer` |
| `scripts/wrenderer.js` | 360 | Canvas-рендер мира, анимации, хвосты |
| `scripts/userGUI/{Inventory,Skills,Mapdrawer,Console}.js` | ~400 | UI-панели (Console.js пустой) |
| `scripts/main.js` | 90 | Точка входа: загрузка мира, спавн ботов, тик программатора |
| `scripts/controls.js` | 287 | Ввод (клавиатура/мышь/тач) |
| `styles/*.css` | — | Стили |

## Модель мира (`wmap.js`)

Три `Uint8Array` на `width*height`:
- `terrain` — твёрдые блоки (solid)
- `background` — задник (дорога/земля/пусто), видно если terrain пуст
- `blocksMeta` — плотность (density) для блоков с `hasdensity`

Чанки 32×32 (`>>5`). `GetIDByCoord` → terrain если solid, иначе background;
вне границ → `104` (фед-блок). `DiggTile` сначала снижает density, при 0 — сносит.
`alivesList: Set` — индексы живых блоков для роста.

**Формат RLE карты** (`LoadWorld`): строка hex-пар → байты. Если за байтом идёт
`00` — следующий байт = длина повтора предыдущего значения (`[val][00][count]`).

## Блоки (`DataTables.js`)

`Block` enum + `BlockStats[256]` (`BlockStatistics`). Поля: `hardness`(−1=неруш.),
`solid`, `falltype`(`sand`=0/`bolder`=1), `replesable`(можно ставить блок поверх),
`cantake`(геология), `diggablerock`, `is_alive`/`is_slime`/`is_cry`, `hasdensity`,
`damage_fall`, `damage_digg`. ID совпадают с сервером (gate 30, box 90, ВБ 81…).
Полная таблица hardness/урона — в `DataTables.js:175-247`.

## Скиллы (`DataTables.js`)

`SkillType` — 59 типов (0..58), у каждого в комментарии буква/код C#
(напр. `Digging:12 // d|digg`). `SkillCalculator[type]` = `SkillParams(effect(lvl),
cost(lvl))`. Формулы вида `lvl ** Math.LN2 * k` с потолком. Конкретные коэффициенты
и точки насыщения (напр. `Movement` 566 lvl, `AntiGun` 521 lvl 92%) —
`DataTables.js:505-531`.

## Предметы (`items.js`)

`ItemCodes` 0..50 (tp,resp,up,market,clan,boom,proto,razr,cred… c190=40,
dollar=49,opp=50). Подтверждает «полное пространство 0..=50» из BACKLOG. Иконки:
`graphics/inventory/icons/{code}.png`.

## Геофизика (`geophys.js`) — ценно, в OpenMines пробел

Тик `FallingCycle` каждые 333мс по активным чанкам (активность задаётся вокруг
ботов в `SetActiveChunks`). `DownFree` решает: упасть вниз / сместиться по
диагонали (рандом влево-вправо). **Валун** требует свободы и сбоку, и снизу-сбоку;
**песок** — только снизу-сбоку.

**Рост живок** `AliveGrowth` (тик ~1000мс, выборка из `alivesList`):
- `alive_cyan(50)` → `cry_cyan` во все 4 стороны
- `alive_red(51)`/`alive_vio(52)` → кри если рядом чёрноскал (`IsBlackRockNear`)
- `alive_white(54)` → `cry_white` если сверху магма (поглощает магму)
- `alive_blue(116)` → ползёт (density-флаг), оставляя `cry_blue`
- `alive_black(53)`/`alive_reinbow(55)` — заглушки
Твёрдость продукта = `AliveProductHardness[block] * (1 + кол-во гипноскал рядом)`.

## Здания (`packs.js` + `entity.js`)

`Pack` база: `owner`, `clan`, `hp=1000`, footprint через `PackConstruction`
(ASCII-сетка: `f`=рамка, `c`=угол, `e`=вход, `r`/`g`=дорога, цифра=интерактив).
Флаги: `__onlyClanpack`, `__onlyNoClanPack`, `__consumeCry`, `__interactive`.
Типы: `Resp`(consume b), `Stock`, `Craft`, `Gun`(clan-only, consume c),
`Gate`(не интерактивен), `Market`(no-clan), `UP`(no-clan), `TP`, `Clans`, `Science`.
Футпринты-сетки — `packs.js`.

## Бот (`bot.js`)

Кулдауны (мс): `digging 333, rotate 100, heal 200, setblock 200, geo 200,
macrosCD 50`. Действия: `Move`(u/d/l/r/f, авто-копа при упоре), `Rotate`, `Digg`,
`SetBlock`(зел101→жёлт102→красн105 / replesable→101), `SetRoad`, `SetWB`,
`SetQuadro`(опора49→квадро48), `UseGeo`(взять `cantake`/поставить из стека),
`Heal`. `Trails` — затаптывание земли 32→33→34. `BotSkillEffects` — все
вычисляемые параметры (hp, скорости, добыча по кри, цены стройки, объёмы корзины).

## Программатор (`programmator.js`) — самый полный референс

`CellID` — **111 опкодов (0..110)**, `ExecutorList[id](condition)` исполняет.
Категории:
- **0-2** служебные (empty/new_line/mark)
- **3,16,17** вход в под-функцию (`GO_SUB`/`GO_STATE`/`GO_FUNC` — разные правила
  наследования логики/курсора в стек)
- **4-13,18-26** `look_*` — смещение курсора просмотра (`viewOffset`), абс. и относит.
- **27** `GO_TO`, **30-31** hand-mode, **33-34** `IF_TRUE`/`IF_FALSE`
- **35-39** макросы (gun/digg/block/heal/digg_around) — с `macrosCD`
- **40-41** `OR`/`AND`, **42** `FLIP`, **43-44** автокопа, **45-47** `RETURN*`
- **48-50** `VAR_EQUAL/MORE/LESS` (сравнения + присвоение user-переменных)
- **51-61** `ONLINE_*` (применение предметов — заглушки в прототипе)
- **62-82** действия бота (move/rotate/digg/set_*/heal)
- **83-86** инвентарь (заглушки)
- **87-110** предикаты клетки (`is_empty/is_crys/is_alive/is_bolder/is_sand/
  is_road/is_*_block/HP_half`…)

**Переменные** (`ReadonlyVariables`): AUT,AGR,HND,DBG,STK,DIR,X,Y,CEL,HP,HPP,TIM,
G/GP,B/BP,R/RP,W/WP,V/VP,C/CP,GEO/GEP,LOA,RND,FLP,BOO,AX,AY,DX,DY. User-переменные
через Map. **Команды-операции** (`PComands`): SET,ADD,MUL,DIV,SUB,MOD + парные
AD2/MU2/DI2/SU2 (над двумя последними переменными, `VarCacher`).

**Стек/логика**: `ProgramStack`(viewOffset, logicalValue, mode_logic off/none/
and/or, returnIndex), глубина до 500. Программа — плоский массив ячеек, ширина
строки `LineLength=16`; конец строки → возврат в начало строки (цикл), пустые
ячейки пропускаются.

`IsOperationFast[111]` — флаг «быстрая операция» (true) vs требует тика (false:
макросы, move/rotate/digg, online-предметы).

## Что сверить с нашим сервером

- Кулдауны и формулы скиллов — числа тут конкретны, но прототипные; сверять с C#.
- Геофизика живок (7 типов) и `alive_blue` ползущая — в OpenMines не реализовано.
- Опкоды программатора 0..110 — эталон семантики для нашего парсера/исполнителя.
- Цепочки SetBlock/SetQuadro/Trails — простые, удобны как чек-лист поведения.
