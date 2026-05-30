# BotSpot programmator — анализ и план (ROADMAP §3 Spot / открытый #8)

Статус: **НЕ реализовано. Референс НЕПОЛОН для ключевого звена (привязка
программы). Реализация 1:1 невозможна без контракта клиента.** Анализ от
2026-05-30 (автономная сессия паритета).

## Что уже есть (Rust)

- `server/game/botspot.rs`: `BotSpotMarker`, `BotSpotData` (id=-owner_id,
  skin=3, tail=1, dir, spot_entity), `BotSpotBasket` (своя корзина + `cb`).
- Spot building (`PackType::Spot`): placement/removal, HB-рендер бота
  (skin=3, tail=1, id=-owner_id), owner GUI, lifecycle spawn/despawn — есть.
- Игроковый `programmator_system` (`server/game/programmator.rs:546`):
  парсер (LZMA+base64, ~90 ActionType, If/Loop/GoTo/RunSub/Macros) и
  пошаговое исполнение — РАБОТАЕТ для игроков.

## Поведение BotSpot из референса (`Entities/BotSpot.cs`, 1:1 цель)

- **Стационарный.** `Move(...)` → `return false` ВСЕГДА (бот не перемещается,
  даже dir не меняет через Move). Программа = вращение (`Rotate`) + копка
  (`Bz`) вокруг фиксированной позиции.
- **`Bz()` (копка, BotSpot.cs:83-149):** копает клетку в `GetDirCord()` (по
  `dir`); урон/`hitdmg` и `Mine` — по скиллам ВЛАДЕЛЬЦА (`owner.skillslist`),
  но кристаллы в СВОЮ корзину (`crys`, отдельная от владельца). FX
  `SendDFToBots(0,…)` старт, `SendDFToBots(2,…)` на Mine. BOX(90)/
  MilitaryBlock(81) special-case (DamageCell 1). Boulder push 1:1 как у
  игрока, exp владельцу. `cb` — дробный аккумулятор (как player Mine).
- **`Update() => _pdata.Step()`** — каждый тик шаг программатора бота.
- Heal=false, Hurt=пусто (неуязвим), Death=пусто, Geo=base.

## ГЭП 1 — система исполнения (большая, additive)

Нужна BotSpot-вариация исполнения программ. Текущий `programmator_system`
жёстко player-coupled: `PlayerConnection` (шлёт пакеты клиенту),
`PlayerSkills`, `PlayerStats`, `PlayerPosition`. Для бота:
- нет `PlayerConnection` (бот безголовый; пакеты-эффекты идут через broadcast,
  не персональный tx);
- скиллы — владельца (lookup по owner_id среди игроков/БД);
- Move — no-op (false), Rotate — меняет `BotSpotData.dir`;
- Dig — `Bz`-бота (стационарный, скиллы владельца, своя корзина);
- own basket → при изменении сериализовать/сохранять (C# `Translate()` —
  тоже заглушка: `Console.WriteLine("should save basket and pos")`).

**План:** вынести action-исполнение программатора (If/Loop/GoTo/RunSub/cell-
checks/Rotate/Geo) в обобщённый слой над трейтом «исполнитель» (player |
botspot), а Move/Dig/skills/output — через реализацию трейта. Затем
`botspot_programmator_system`, итерирующий `(BotSpotData, ProgrammatorState,
BotSpotBasket)` и зовущий Step через botspot-исполнитель. Оценка: 400-600
строк + рефактор обработчиков `programmator.rs`. Риск: средний (additive, не
ломает player-путь, если обобщать аккуратно).

## ГЭП 2 — ПРИВЯЗКА ПРОГРАММЫ (БЛОКЕР: референс — заглушка)

`Buildings/Spot.cs` НЕПОЛОН: `public Program? selected` объявлен, но **нигде
не присваивается**; `GUIWin` возвращает пустые `Tabs = []`; `Destroy`/
`ClearBuilding` — `//idk` / `//No dick no balls`. Как клиент назначает
программу боту — в референсе НЕ показано (тот самый `/////FIX THIS SH`).

Значит 1:1 невозможен — нет эталона. Нужен контракт КЛИЕНТА (источник
правды, см. [[client-is-source-of-truth-not-reference]]): какой TY-ивент
шлёт клиент при назначении программы Spot'у (вероятно вариант `PROG` с
target=spot или отдельный Spot-GUI-кнопка → `GUI_`). Имена пакетов
RSA-зашифрованы → нужен анализ `client/` (ProgrammatorManager.cs / Spot-UI)
с расшифровкой. До этого — реализовывать assignment нельзя.

## Рекомендация

1. Сначала закрыть ГЭП 2 анализом клиента (отдельная сессия с `client/`):
   зафиксировать контракт назначения программы → `docs/CLIENT_PROTOCOL_GAPS`.
2. Затем ГЭП 1 (обобщённый исполнитель + `botspot_programmator_system`),
   с верификацией на ЖИВОМ клиенте (программа боту → бот вращается/копает,
   кристаллы в его корзину, HB tail=1).

Не реализовывать вслепую: исполнение непроверяемо без клиента, а assignment
не имеет 1:1-эталона.
