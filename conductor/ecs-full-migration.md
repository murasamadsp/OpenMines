# План полной миграции на ECS (Buildings)

Этот план описывает перенос данных строений (зданий) из DashMap в компоненты ECS (bevy_ecs), что обеспечит архитектурную целостность и повысит производительность систем.

## 1. Определение компонентов (server/game/buildings.rs)

Необходимо заменить структуру `PackData` на набор компонентов:
- `BuildingMetadata { id: i32, pack_type: PackType }`
- `BuildingStats { charge: f32, max_charge: f32, cost: i32, hp: i32, max_hp: i32 }`
- `BuildingStorage { money: i64, crystals: [i64; 6], items: HashMap<i32, i32> }`
- `BuildingCrafting { recipe_id: Option<i32>, num: i32, end_ts: i64 }`
- `BuildingOwnership { owner_id: PlayerId, clan_id: i32 }`
- `GridPosition { x: i32, y: i32 }`
- `BuildingFlags { dirty: bool }` - для системы сохранения в БД.

## 2. Рефакторинг GameState (server/game/mod.rs)

- Удаление `packs: DashMap<(i32, i32), PackData>`.
- Добавление `building_index: DashMap<(i32, i32), Entity>` для быстрого поиска здания по координатам (точка привязки).
- Обновление `GameState::new`: загрузка зданий из БД теперь должна создавать сущности в ECS.
- Обновление вспомогательных методов: `get_pack_at`, `find_pack_covering`, `get_packs_in_chunk_area` должны использовать ECS и индекс.
- Добавление метода `modify_building<F, R>(&self, entity: Entity, f: F) -> Option<R>`.

## 3. Обновление систем (server/game/combat.rs)

- `gun_firing_system`: переписать на использование `Query<(&BuildingMetadata, &mut BuildingStats, &BuildingOwnership, &GridPosition)>`.
- Поиск целей-игроков через `Query<(&PlayerMetadata, &PlayerPosition, &mut PlayerStats, &PlayerCooldowns)>`.

## 4. Обновление сетевых обработчиков (server/net/session/social/buildings.rs)

- `handle_place_building`: создание сущности вместо вставки в DashMap.
- `handle_remove_building`: удаление сущности и обновление индекса.
- `update_pack_with_db` / `update_pack_with_world_sync`: перенос логики обновления на `modify_building`.

## 5. Персистентность (Persistence)

- Реализация механизма сохранения "грязных" зданий в `server/net/lifecycle.rs`.
- Добавление `extract_building_row` в `server/game/buildings.rs` для сериализации компонентов ECS в строку БД.

## Этапы реализации

1.  **Phase 1**: Определение компонентов в `buildings.rs` и реализация `extract_building_row`.
2.  **Phase 2**: Рефакторинг `GameState` (удаление `packs`, добавление индекса, обновление `new`).
3.  **Phase 3**: Обновление `combat.rs` и других игровых систем.
4.  **Phase 4**: Обновление сетевых обработчиков в `social/buildings.rs`.
5.  **Phase 5**: Добавление цикла сохранения зданий в `lifecycle.rs`.
