//! Геология (Xgeo) — pickup/place блоков.
use crate::game::broadcast_cell_update;
use crate::game::player::{
    PlayerCooldowns, PlayerGeoStack, PlayerPosition, PlayerSkillsComp, PlayerStats,
};
use crate::game::programmator::ProgrammatorState;
use crate::game::skills::SkillType;
use crate::net::session::prelude::*;
use rand::Rng;

fn send_geo_state_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("ГЕОЛОГИЯ", "Состояние игрока недоступно.").1,
    );
}

/// `Session.GeoHandler` → `TryAct(player.Geo, 200)` → `PEntity.Geo` + `SendGeo` (`pSenders.cs`).
pub fn handle_geo(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    programmatic: bool,
) {
    let result = state
        .modify_player(pid, |ecs, entity| {
            let Some(program_state) = ecs.get::<ProgrammatorState>(entity) else {
                tracing::error!(player_id = %pid, component = "ProgrammatorState", "Player component missing for geo");
                send_geo_state_error(tx);
                return None;
            };
            if !programmatic && !program_state.is_manual_control_allowed() {
                return None;
            }
            {
                let Some(cd) = ecs.get::<PlayerCooldowns>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerCooldowns", "Player component missing for geo");
                    send_geo_state_error(tx);
                    return None;
                };
                if !programmatic && cd.last_geo.elapsed() < Duration::from_millis(200) {
                    return None;
                }
            }

            // НАМЕРЕННАЯ ДЕВИАЦИЯ от C# по ПРЯМОМУ ТРЕБОВАНИЮ ПОЛЬЗОВАТЕЛЯ:
            // геология работает ТОЛЬКО при УСТАНОВЛЕННОМ в слот скилле Geology.
            // В эталоне (`PEntity.Geo`) гейта нет — гео доступно без скилла; юзер
            // явно указал требовать установленный скилл. `find(code)` = в слоте
            // (НЕ `get_player_skill_effect`, который для неустановленного даёт
            // эффект уровня 0 и может быть >0).
            let Some(skills) = ecs.get::<PlayerSkillsComp>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing for geo");
                send_geo_state_error(tx);
                return None;
            };
            skills.states.find(SkillType::Geology.code())?;

            let (px, py, dir) = {
                let Some(pos) = ecs.get::<PlayerPosition>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerPosition", "Player component missing for geo");
                    send_geo_state_error(tx);
                    return None;
                };
                (pos.x, pos.y, pos.dir)
            };
            let Some(player_stats) = ecs.get::<PlayerStats>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing for geo");
                send_geo_state_error(tx);
                return None;
            };
            let cid = player_stats.clan_id.unwrap_or(0);
            if ecs.get::<PlayerGeoStack>(entity).is_none() {
                tracing::error!(player_id = %pid, component = "PlayerGeoStack", "Player component missing for geo");
                send_geo_state_error(tx);
                return None;
            }
            let (dx, dy) = dir_offset(dir);
            let (tgt_x, tgt_y) = (px + dx, py + dy);

            let mut broadcast: Vec<(i32, i32)> = Vec::new();

            if state.world.valid_coord(tgt_x, tgt_y)
                && state.access_gun_full_in_ecs(ecs, tgt_x, tgt_y, cid).0
            {
                let cell = state.world.get_cell_typed(tgt_x, tgt_y);
                let defs = state.world.cell_defs();
                let cell_props = defs.get_typed(cell);
                let pickable = cell_props.nature.is_pickable && !cell_props.cell_is_empty();
                let place_here = cell_props.cell_is_empty()
                    && cell_props.can_place_over()
                    && state.find_pack_covering_in_ecs(ecs, tgt_x, tgt_y).is_none();

                if pickable {
                    {
                        let mut stack = ecs.get_mut::<PlayerGeoStack>(entity)?;
                        stack.0.push(cell.0);
                    }
                    state.world.destroy(tgt_x, tgt_y);
                    broadcast.push((tgt_x, tgt_y));
                } else if place_here {
                    if let Some(cplaceable) = ecs.get_mut::<PlayerGeoStack>(entity)?.0.pop() {
                        let place_cell = crate::world::CellType(cplaceable);
                        let d = if place_cell.is_crystal() {
                            0.0
                        } else {
                            let mut rng = rand::rng();
                            if rng.random_range(1..=100) > 99 {
                                0.0
                            } else {
                                defs.get_typed(place_cell).durability
                            }
                        };
                        state.world.write_world_cell(
                            tgt_x,
                            tgt_y,
                            crate::world::WorldCell {
                                cell_type: place_cell,
                                durability: d,
                            },
                        );
                        broadcast.push((tgt_x, tgt_y));
                    }
                }
            }

            let geo_name = ecs
                .get::<PlayerGeoStack>(entity)
                .and_then(|s| s.0.last())
                .map(|&c| state.world.cell_defs().get(c).name.clone())
                .unwrap_or_default();

            {
                let mut cd = ecs.get_mut::<PlayerCooldowns>(entity)?;
                cd.last_geo = Instant::now();
            }

            Some((geo_name, broadcast))
        })
        .flatten();

    let Some((geo_name, broadcast)) = result else {
        return;
    };
    for (x, y) in broadcast {
        broadcast_cell_update(state, x, y);
    }
    send_u_packet(tx, "GE", &geo(&geo_name).1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc::UnboundedReceiver;

    struct TestState {
        state: Arc<GameState>,
        player: crate::db::PlayerRow,
        world_name: String,
        db_path: std::path::PathBuf,
    }

    impl TestState {
        fn cleanup(&self) {
            let dir = std::env::temp_dir();
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(self.db_path.with_extension("db-wal"));
            let _ = std::fs::remove_file(self.db_path.with_extension("db-shm"));
            let _ = std::fs::remove_file(dir.join(format!("{}_v2.map", self.world_name)));
            let _ = std::fs::remove_file(dir.join(format!("{}_durability.mapb", self.world_name)));
        }
    }

    async fn make_test_state(label: &str) -> TestState {
        let dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = dir.join(format!("{label}_{}_{}.db", std::process::id(), nonce));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();
        let player = database.create_player("geo-user", "p", "h").await.unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("{label}_world_{}_{}", std::process::id(), nonce);
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::default(),
            cron: crate::config::CronConfig::default(),
            gameplay: crate::config::GameplayConfig::default(),
        };
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();

        TestState {
            state,
            player,
            world_name,
            db_path,
        }
    }

    fn drain_events(rx: &mut UnboundedReceiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
        let mut events = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            let mut buf = BytesMut::from(&frame[..]);
            let packet = crate::protocol::Packet::try_decode(&mut buf)
                .expect("valid packet")
                .expect("decoded packet");
            events.push((packet.event_str().to_owned(), packet.payload.to_vec()));
        }
        events
    }

    #[tokio::test]
    async fn geo_missing_programmator_state_is_explicit_error_not_not_running_fallback() {
        let test = make_test_state("geo_missing_programmator").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<ProgrammatorState>();
        }

        handle_geo(&test.state, &tx, pid, false);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));

        test.cleanup();
    }

    #[tokio::test]
    async fn geo_without_installed_skill_stays_quiet_noop() {
        let test = make_test_state("geo_no_skill_quiet").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        handle_geo(&test.state, &tx, PlayerId(test.player.id), false);

        let events = drain_events(&mut rx);
        assert!(events.is_empty());

        test.cleanup();
    }
}
