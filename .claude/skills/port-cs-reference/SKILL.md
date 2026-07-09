---
name: port-cs-reference
description: Use when porting any feature from docs/reference/server_reference/ (C#) to crates/openmines-server/ (Rust) in OpenMines — before reading any C# code, before planning, before implementing. Also use when C# and client behavior seem to conflict, or when a feature exists in the client but not in docs/reference/server_reference/.
---

# Перенос C# → Rust (OpenMines)

## Иерархия истины

```
1. JS РЕФЕРЕНС (docs/reference/js_reference/) — эталон, если конфликтует с C# или клиентом
2. КЛИЕНТ (client/*.cs) — эталон wire-ожиданий и GUI-поведения
3. C# РЕФЕРЕНС (docs/reference/server_reference/) — источник логики реализации
4. Rust сервер (crates/openmines-server/) — что мы пишем
```

Wire-формат неизменяем. Сервер подстраивается под legacy-клиент; `client/`
трогать только по явному запросу и с обязательной компиляцией.

**Никогда не начинай с C#. Начни с клиента.**

## Процесс переноса

### 1. Проверь CLIENT_PROTOCOL_GAPS.md

`docs/reference/CLIENT_PROTOCOL_GAPS.md` — список мест, где клиент расходится с C#.

Читай ДО любого кода. Если фича там есть — это источник правды, не C#.

### 2. Проверь клиент

`client/Assets/Scripts/ServerController.cs` — главный диспатчер пакетов (~45 обработчиков).

Найди обработчик для нужного пакета. Смотри что клиент **ждёт**:

- Структуру wire-пакета (поля, разделители, кодировку)
- Порядок пакетов при инициализации / ответе
- Edge-cases: что при пустом, нулевом, отсутствующем поле

### 3. Проверь C# референс

Теперь читай C# для логики. Фокус:

- Алгоритм (формулы, порядок операций)
- Состояние (какие поля нужны, что персистируется)
- Побочные эффекты (что ещё отправляется / сохраняется / бродкастится)

### 4. Реализуй в Rust (1:1 если нет причины отклониться)

Используй таблицу переводов ниже. Следуй C# логике точно, если нет **конкретной причины** из раздела «Когда отклоняться».

### 5. Верифицируй

- Wire-формат совпадает с тем, что ждёт клиент
- Порядок пакетов совпадает с C# / клиентом
- `cargo clippy --all-targets --all-features -- -D warnings` — 0 ошибок

## OOP → ECS

| C# | Rust / ECS |
| - | - |
| `class Player { int hp; }` | `#[derive(Component)] struct PlayerStats { hp: i32 }` |
| `player.Mine()` метод | ECS система: `fn handle_dig(query: Query<...>)` |
| `null` / `T?` nullable | `Option<T>` |
| `DateTime` / cooldown поле | `Instant` + `Duration::from_millis(N)` |
| `World.W` синглтон | `GameState`/ECS resources in `crates/openmines-server/` |
| EF `[NotMapped]` | Поле в Component (не в DB row) |
| `using var db = new DataBase()` | Функция в `crates/openmines-shared/src/db/` |
| `Dictionary<K,V>` | `HashMap<K,V>` |
| Наследование / interface | Trait или `enum` с вариантами |
| `static` helper | Свободная `fn` в модуле |

## Когда отклоняться от C#

| Ситуация | Действие |
| - | - |
| `docs/reference/js_reference/` расходится с C# или клиентом | Следуй JS-референсу, зафиксируй расхождение |
| Клиент ведёт себя иначе, чем C# | Следуй клиенту, задокументируй в `docs/reference/CLIENT_PROTOCOL_GAPS.md` |
| Явный баг C# (доказан клиентом/JS) | Следуй верхнему источнику, добавь запись в `docs/reference/CLIENT_PROTOCOL_GAPS.md` |
| Rust idiom (Option вместо null, enum) | Следуй Rust, сохраняй семантику |
| Deadlock / async safety | Реструктурируй без изменения логики |
| Намеренная девиация (уже задокументирована в `docs/DEVIATIONS.md`) | Следуй документации, не «чини» |

**Не отклоняться, если:**

- «Архитектура другая» — найди способ перевести, не пропускай
- «Это кажется неправильным» — докажи через клиент, потом решай
- «В Rust не принято» — принято, если это бизнес-логика

## Особые случаи

### Фича есть в клиенте, но не в C# референсе

C# реф неполный (временами содержит `/////FIX THIS SH` заглушки).

1. Клиент шлёт пакет — C# обработчик пустой или отсутствует
2. Читай **клиент**: что он отправляет, что ждёт в ответ
3. Реализуй по клиенту
4. Задокументируй в `docs/reference/CLIENT_PROTOCOL_GAPS.md`

*Примеры: `Chin` в Session.cs — пустой метод; реальный контракт в `WorldInitScript.cs:109`.*

### C# содержит баг (клиент ведёт себя иначе)

1. Зафиксируй расхождение через клиентский код (конкретная строка)
2. Реализуй по клиенту (не по C#)
3. Задокументируй в `docs/reference/CLIENT_PROTOCOL_GAPS.md`

*Примеры: `Chat.cs:44` хардкодит `ch="FED"` → DNO ломалось; `GCMessage.Encode` — неверный формат, клиент важнее.*

### Намеренная девиация (1:1 регрессирует)

Задокументируй в `docs/DEVIATIONS.md` с объяснением.

*Пример: C# `Thread.Sleep(200)` в PI-ответе убран — добавлял 200ms UX-регресс.*

## Красные флаги

- **«Не смотрел docs/reference/CLIENT_PROTOCOL_GAPS.md»** — стоп, читай первым
- **«Не смотрел client/»** — стоп, клиент всегда проверяется
- **«Архитектура другая, поэтому пропускаю X»** — нет. Найди способ перевести
- **«C# здесь явно неправильный»** — докажи через клиент, прежде чем пропускать
- **«C# достаточно, клиент очевидно то же самое»** — нет. Проверяй всегда

## Чеклист

- [ ] Проверил `docs/reference/CLIENT_PROTOCOL_GAPS.md`
- [ ] Нашёл и прочитал клиентский обработчик в `ServerController.cs` (или смежный файл)
- [ ] Прочитал C# референс
- [ ] Определил все wire-пакеты (что, в каком порядке, формат)
- [ ] Реализовал 1:1 (или задокументировал причину отклонения)
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` — 0 ошибок
- [ ] Если отклонение — запись в `docs/reference/CLIENT_PROTOCOL_GAPS.md` или `docs/DEVIATIONS.md`
