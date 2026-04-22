---
name: reference-parity-check
description: Use when implementing any feature, fixing bugs, or auditing existing code against server_reference/. Use when touching game logic, network handlers, packet construction, formulas, or any behavior that has a C# counterpart. Also use proactively to detect deadlocks in Arc/RwLock/DashMap usage.
---

# Reference Parity Check

## Overview

**Побитовый паритет с C# референсом. Никаких отклонений. Никаких "архитектура другая". 1:1.**

Rust-сервер ОБЯЗАН воспроизводить поведение C# сервера (`server_reference/`) байт-в-байт на уровне wire protocol и тик-в-тик на уровне игровой логики. Клиент неизменяем — любое расхождение = баг.

## Iron Rules

1. **`server_reference/` — единственный источник правды.** Не документация, не "как я думаю должно работать", не "логично было бы". Только C# код.
2. **1:1 значит 1:1.** Тот же порядок операций. Те же формулы. Те же edge cases. Те же magic numbers. Если в C# стоит `200` — в Rust стоит `200`, не `COOLDOWN_MS`.
3. **"Архитектура другая" — не аргумент.** Архитектура другая, поведение идентичное. Если C# делает X, Rust делает X. Способ реализации может отличаться, наблюдаемый результат — нет.
4. **Нет N/A.** Каждая ветка, каждый `if`, каждый early return в C# имеет аналог в Rust. Если не нашёл — не портировал.

## Процесс проверки

### Шаг 1: Найти C# источник

Для проверяемой фичи найти ВСЕ релевантные файлы в `server_reference/`. Не один файл — все.

```
Копание → Player.cs (Bz method), Basket.cs (Mine), Session.cs (TY dispatch), 
          World.cs (cell access), Skill.cs (dig power formula)
```

**Читать целиком.** Не grep по ключевому слову — читать метод от начала до конца.

### Шаг 2: Трассировка потока

Проследить полный путь выполнения в C#:

1. Откуда приходит вызов (Session.cs TY dispatch? таймер? ECS system?)
2. Какие проверки в каком порядке (early returns, guards)
3. Какие side effects (пакеты клиенту, изменения стейта, DB writes)
4. Какие broadcast'ы (кому, что, в каком порядке)
5. Какие edge cases обработаны явно

### Шаг 3: Побитовое сравнение

Для каждого элемента из шага 2, найти Rust-аналог и сверить:

| Что сверять | Как |
|-------------|-----|
| Порядок проверок | Тот же порядок `if`/`match` — первая сработавшая ветка та же |
| Формулы | Идентичные операнды, операторы, порядок вычисления, округление |
| Magic numbers | Те же числа, не вынесенные в константы (если в C# inline — в Rust inline) |
| Порядок пакетов | Пакеты отправляются в том же порядке |
| Payload формат | Байт-в-байт совпадение wire output |
| Edge cases | Каждый `if` в C# имеет аналог, включая "мёртвый" код |
| Error paths | Что происходит при невалидном input — тот же ответ |
| Типы данных | i32 vs u32, overflow behavior, cast semantics |
| String formatting | Те же разделители (`:`, `#`, `,`), тот же порядок полей |

### Шаг 4: Deadlock-аудит

При любой работе с shared state проверить:

```
ЗАПРЕЩЁННЫЕ ПАТТЕРНЫ:

1. Nested locks: lock A → lock B (где B может lock A в другом месте)
2. Lock held across await: RwLock guard живёт через .await point
3. DashMap iteration + mutation: iter() на DashMap при возможной вставке из другого потока
4. Arc<RwLock> read → upgrade to write (нет upgrade в std, deadlock)
5. Broadcast внутри lock: send пакета клиенту пока держишь write lock на стейт
```

**Проверка:**
- Для каждого `write()` / `lock()` — проследить все вызовы до release
- Убедиться что между acquire и release нет другого acquire
- Убедиться что между acquire и release нет отправки пакетов (может заблокировать на full TCP buffer)
- `DashMap` entry API (entry/get_mut) держит shard lock — не вызывать другие DashMap методы внутри

### Шаг 5: Вердикт

Для каждого проверенного элемента — один из трёх статусов:

- **MATCH** — поведение идентично, все ветки покрыты
- **DIVERGE** — расхождение найдено, описать конкретно что и где
- **MISSING** — в Rust отсутствует логика которая есть в C#

**DIVERGE и MISSING = баг. Исправлять немедленно.**

## Mapping: C# → Rust

| C# Location | Rust Location |
|---|---|
| `Server/Session.cs` TY switch | `server/net/session/dispatch/ty.rs` |
| `Player.cs` methods | `server/net/session/play/` + `server/game/` |
| `Player.cs` Init() | `server/net/session/player/init.rs` |
| `pSenders.cs` | `server/net/session/outbound/` |
| `Basket.cs` | `server/net/session/play/dig_build.rs` (crystal logic) |
| `Inventory.cs` | `server/net/session/ui/heal_inventory.rs` |
| `Pack.cs` + subclasses | `server/game/buildings.rs` + `server/net/session/social/buildings.rs` |
| `Skill.cs` | `server/game/skills.rs` |
| `Clan.cs` | `server/net/session/social/clans.rs` |
| `Chat.cs` | `server/net/session/social/misc.rs` |
| `Program.cs` | `server/game/programmator.rs` |
| `World.cs` | `server/world/` |
| `Physics.cs` | `server/game/sand.rs` + `server/game/combat.rs` |
| `Settings.cs` | `server/net/session/ui/settings.rs` |

## Рационализации которые НЕ ПРИНИМАЮТСЯ

| Отмазка | Почему не катит |
|---------|-----------------|
| "Архитектура ECS не позволяет" | ECS — способ хранения. Поведение извне то же. Найди способ. |
| "Это мёртвый код в C#" | Если клиент может триггернуть — не мёртвый. Порти. |
| "Rust идиоматичнее сделать иначе" | Идиоматичность внутри, wire output побитово. |
| "Формула эквивалентна" | `a*b/c` ≠ `a/c*b` из-за integer overflow/truncation. Порядок тот же. |
| "Убрал намеренно для производительности" | Отметь в ROADMAP как осознанное отклонение с обоснованием. Не молча. |
| "N/A — не нужно для нашей версии" | Порти всё. Отключай через конфиг если надо, но код должен быть. |
| "Потом доделаю" | Сейчас. Или явно пометь ❌ в ROADMAP с описанием что именно missing. |

## Deadlock Red Flags

Если видишь любое из этого — **СТОП, разбирайся:**

- `game_state.ecs.write()` внутри блока где уже есть другой lock
- `.await` между `let guard = x.read()` и drop(guard)
- `active_players.get()` внутри `chunk_players.iter()`
- Отправка пакета (`send_packet`, `send_u_packet`) внутри любого write lock
- `DashMap::iter()` когда другой код может делать `insert`/`remove` в тот же map
- Любой `RwLock` который держится >1 строки без очевидной причины

## Чеклист при портировании новой фичи

1. [ ] Нашёл ВСЕ C# файлы затрагивающие фичу
2. [ ] Прочитал каждый метод ЦЕЛИКОМ (не grep)
3. [ ] Нарисовал flow: вход → проверки → мутации → пакеты → broadcast
4. [ ] Каждый `if`/`switch` в C# имеет аналог в Rust
5. [ ] Формулы скопированы побитово (порядок операций, типы, округление)
6. [ ] Порядок отправки пакетов идентичен
7. [ ] Wire payload формат сверен (разделители, порядок полей, encoding)
8. [ ] Edge cases: null/empty/overflow/negative — обработаны так же
9. [ ] Deadlock-аудит: нет nested locks, нет lock across await, нет broadcast inside lock
10. [ ] Проверил что клиент получает ровно те же байты что от C# сервера
