# Намеренные девиации от C#-референса

> **Зачем.** Сервер портируется 1:1 с `docs/reference/server_reference/` (C#), но в ряде мест мы
> ОСОЗНАННО отклоняемся — по требованию пользователя, ради UX, безопасности, или
> потому что C#-баг доказан клиентом. Без реестра следующий рефактор «починит»
> такое отклонение и сломает поведение/совместимость. **Прежде чем «выравнивать
> под C#» — проверь, нет ли строки здесь.**
>
> Ось отличается от [`CLIENT_PROTOCOL_GAPS.md`](CLIENT_PROTOCOL_GAPS.md) (там —
> где C#-референс неполон/неверен относительно клиента) и
> [`WORLDGEN_PARITY.md`](archive/WORLDGEN_PARITY.md) (байт-паритет генерации). Здесь —
> только намеренные поведенческие отклонения.

Легенда: **[USER]** по прямому требованию пользователя · **[UX]** ради опыта ·
**[SAFETY]** живучесть/защита · **[KEEP-BUG]** намеренно держим C#-quirk 1:1.

---

## Геймплей

| Где | Девиация | Причина |
|-----|----------|---------|
| `crates/openmines-server/src/net/session/play/geo.rs` | Поведение геологии изменено относительно C# | **[USER]** прямое требование |
| `crates/openmines-server/src/net/session/play/bonus.rs` + `gameplay.bonus` | `GDonPacket` реализован (в C# — заглушка, декод без эффекта); cooldown/reward настраиваются через config | **[UX]** ежедневный бонус нужен живым |
| `crates/openmines-server/src/net/session/ui/heal_inventory.rs` | building-items `{0,1,2,3,24,26,29}` обрабатываются, хотя в C# не входят в `typeditems` | паритет с КЛИЕНТОМ (эталон), а не с неполным C# |
| `crates/openmines-server/src/net/session/ui/gui_buttons.rs` | Порядок кнопок маркета: «Продать» → «Продать всё» | **[UX]** удобнее; wire-нейтрально |
| `crates/openmines-server/src/game/mechanics/combat.rs` | Урон пушки — округлённый каст после `AntiGun`, клампится снизу; базовое значение настраивается через `gameplay.combat.gun_damage` (default 60) | float→int без потери паритета, config-driven tuning |
| `crates/openmines-server/src/game/actors/programmator.rs` + `gameplay.programmator.min_move_delay_ms` | Настраиваемый floor задержки движения программы (в C# пола нет) | **[SAFETY]** анти-infinite-loop / CPU-stall |
| `crates/openmines-server/src/game/actors/programmator.rs` | Не портируем C# `delay += 200ms` для хода программатора | **[USER]** скорость движения робота определяется только прокачкой/базовым конфигом, без скрытых штрафов программатора |
| `crates/openmines-server/src/game/actors/programmator.rs` | `MacrosBuild` (id 142) намеренно не в этой ветке | 1:1 C# `PAction.Execute` его не имеет |
| `crates/openmines-server/src/net/session/ui/up_building.rs` `handle_skill_upgrade` | Апгрейд скилла СТОИТ денег (`cost = gameplay.skills.upgrade_cost_base * уровень`), списывает + блокирует при нехватке. В C# `Skill.Up` бесплатный (только exp) | **[USER]** «каждый апгрейд стоит денег» — экономика, конфиг-тюнинг |

## Производительность / живучесть

| Где | Девиация | Причина |
|-----|----------|---------|
| `crates/openmines-server/src/tasks/lifecycle.rs` `spawn_game_tick_loop` | game-tick под supervisor'ом, респавн при панике (backoff 200ms) | **[SAFETY]** паника не должна превращать сервер в «зомби» |
| `crates/openmines-server/src/tasks/auction.rs` | БД-скан аукциона реже, чем «каждый тик» C# | нагрузка на БД |
| `crates/openmines-server/src/net/session/play/chunks.rs` | Пустой HB-бандл не шлём (C# шлёт всегда) | безвредно, экономит трафик |
| `crates/openmines-server/src/net/session/play/chunks.rs` | HB без активных расходников при пересечении | намеренно (см. коммент) |
| PI-ответ (история) | Убран `Thread.Sleep(200)` из C# PI-ответа | **[UX]** убирал 200ms лаг |

## Worldgen (детали — в `archive/WORLDGEN_PARITY.md`)

| Где | Девиация | Причина |
|-----|----------|---------|
| `crates/openmines-shared/src/world/generator.rs:144` | `random_sized_parts`: редро сегментов **без cap** | **[KEEP-BUG]** 1:1 C# (тоже безлимитный); на реальных секторах сходится. ⚠️ багованный seed теоретически вешает worldgen-поток |
| `crates/openmines-shared/src/world/generator.rs` (f64) | Базис шума переведён `f32→f64` | байт-паритет с .NET `NextDouble()` |
| `crates/openmines-shared/src/world/anl.rs:7` | Опущены мёртвые ветки `rand.Next(0,4)` | в C# недостижимы |

## Модель данных

| Где | Девиация | Причина |
|-----|----------|---------|
| `crates/openmines-shared/src/db/clans.rs:39` | `clan_id == icon`, диапазон 1..=218, иначе отказ | 1:1 C#-модель (клиент рисует иконку по id) |
| `crates/openmines-server/src/bootstrap.rs` `regen_clear_world_state` | При `--regen` позиции игроков сбрасываются на `gameplay.spawn` | **[SAFETY]** старые `x/y` указывают внутрь нового рельефа → смерть на спавне |

---

> Дополняй при КАЖДОМ новом осознанном отклонении: `файл:строка` + причина + метка.
> Если отклонение временное/баг — ему не здесь, а в `ROADMAP.md`/issue.
