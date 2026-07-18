use crate::game::GameState;
use crate::world::WorldProvider;
use std::sync::Arc;

pub enum BoxPickupApplyResult {
    Picked {
        save: crate::game::SaveCommand,
        broadcasts: Vec<crate::game::BroadcastEffect>,
    },
    Stale,
}

fn box_pickup_broadcasts(
    intent: crate::game::BoxPickupIntent,
    picked: [i64; 6],
    crystals: [i64; 6],
    connection: Option<crate::game::SessionId>,
) -> Vec<crate::game::BroadcastEffect> {
    let mut broadcasts = Vec::new();
    if let Some(session_id) = connection {
        broadcasts.push(crate::game::BroadcastEffect::Direct {
            session_id,
            data: crate::net::session::wire::make_u_packet_bytes(
                "@B",
                &crate::protocol::packets::basket(&crystals, 1).1,
            ),
        });
    }

    if let crate::game::BoxPickupSource::Dig {
        direction,
        skin,
        clan_id,
        tail,
        exclude_self,
        ..
    } = intent.source
    {
        let (box_x, box_y): (i32, i32) = intent.box_pos.into();
        if let Some(session_id) = connection {
            let total: i64 = picked.iter().sum();
            let bubble = crate::protocol::packets::hb_chat(
                0,
                crate::net::session::util::net_u16_nonneg(box_x),
                crate::net::session::util::net_u16_nonneg(box_y),
                &format!("+ {total}"),
            );
            broadcasts.push(crate::game::BroadcastEffect::Direct {
                session_id,
                data: crate::net::session::wire::encode_hb_bundle(
                    &crate::protocol::packets::hb_bundle(&[bubble]).1,
                ),
            });
        }
        let (player_x, player_y): (i32, i32) = intent.player_pos.into();
        let bot = crate::protocol::packets::hb_bot(
            crate::net::session::util::net_u16_nonneg(intent.player_id),
            crate::net::session::util::net_u16_nonneg(player_x),
            crate::net::session::util::net_u16_nonneg(player_y),
            crate::net::session::util::net_u8_clamped(direction, 3),
            crate::net::session::util::net_u8_clamped(skin, 255),
            crate::net::session::util::net_u16_nonneg(clan_id),
            tail,
        );
        let (cx, cy) = crate::world::World::chunk_pos(player_x, player_y);
        broadcasts.push(crate::game::BroadcastEffect::Nearby {
            cx,
            cy,
            data: crate::net::session::wire::encode_hb_bundle(
                &crate::protocol::packets::hb_bundle(&[bot]).1,
            ),
            exclude: exclude_self.then_some(intent.player_id),
        });
    }
    broadcasts.push(crate::game::BroadcastEffect::CellUpdate(intent.box_pos));
    broadcasts
}

pub fn apply_box_pickup(
    state: &Arc<GameState>,
    intent: crate::game::BoxPickupIntent,
) -> BoxPickupApplyResult {
    state
        .modify_player(intent.player_id, |ecs, entity| {
            let Some(pos) = ecs.get::<crate::game::player::PlayerPosition>(entity) else {
                return Some(BoxPickupApplyResult::Stale);
            };
            if crate::game::WorldPos::from((pos.x, pos.y)) != intent.player_pos {
                return Some(BoxPickupApplyResult::Stale);
            }
            if ecs
                .get::<crate::game::player::PlayerStats>(entity)
                .is_none()
                || ecs
                    .get::<crate::game::player::PlayerFlags>(entity)
                    .is_none()
            {
                return Some(BoxPickupApplyResult::Stale);
            }

            let connection = ecs
                .get::<crate::game::player::PlayerConnection>(entity)
                .map(|connection| connection.session_id);
            if let crate::game::BoxPickupSource::Dig {
                session_id: Some(expected),
                ..
            } = intent.source
                && connection != Some(expected)
            {
                return Some(BoxPickupApplyResult::Stale);
            }

            let (x, y): (i32, i32) = intent.box_pos.into();
            if state.world.get_cell_typed(x, y)
                != crate::world::CellType(crate::world::cells::cell_type::BOX)
            {
                return Some(BoxPickupApplyResult::Stale);
            }
            let Some(picked) = state.remove_box_cell_authoritative(x, y) else {
                return Some(BoxPickupApplyResult::Stale);
            };

            let crystals = {
                let mut player_stats = ecs
                    .get_mut::<crate::game::player::PlayerStats>(entity)
                    .expect("PlayerStats checked before hazard box pickup");
                for (slot, amount) in player_stats.crystals.iter_mut().zip(picked) {
                    *slot = slot.saturating_add(amount);
                }
                player_stats.crystals
            };
            ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                .expect("PlayerFlags checked before hazard box pickup")
                .dirty = true;

            let broadcasts = box_pickup_broadcasts(intent, picked, crystals, connection);

            Some(BoxPickupApplyResult::Picked {
                save: crate::game::SaveCommand::Box {
                    write: crate::db::BoxWrite {
                        x,
                        y,
                        crystals: None,
                    },
                },
                broadcasts,
            })
        })
        .flatten()
        .unwrap_or(BoxPickupApplyResult::Stale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::PlayerId;
    use crate::test_support::ServerTestHarness;
    use crate::world::WorldProvider;

    async fn make_box_test_state(label: &str) -> ServerTestHarness {
        let test = ServerTestHarness::new(&format!("box_{label}"), "box-user").await;
        let mut rx = test.connect(1);
        while rx.try_recv().is_ok() {}
        test
    }

    #[tokio::test]
    async fn pickup_box_updates_crystals_marks_dirty_and_removes_box() {
        let test = make_box_test_state("pickup").await;
        let pid = PlayerId(test.player.id);
        test.state
            .put_box_cell_authoritative(10, 11, [3, 2, 1, 0, 0, 0]);
        let player_pos = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                Some((pos.x, pos.y).into())
            })
            .unwrap();

        let result = apply_box_pickup(
            &test.state,
            crate::game::BoxPickupIntent {
                player_id: pid,
                player_pos,
                box_pos: (10, 11).into(),
                source: crate::game::BoxPickupSource::Standing,
            },
        );

        let BoxPickupApplyResult::Picked { save, broadcasts } = result else {
            panic!("box pickup must apply");
        };
        assert!(matches!(save, crate::game::SaveCommand::Box { .. }));
        assert_eq!(broadcasts.len(), 2);
        let (crystals, dirty) = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                let stats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
                let flags = ecs.get::<crate::game::player::PlayerFlags>(entity)?;
                Some((stats.crystals, flags.dirty))
            })
            .unwrap();
        assert_eq!(crystals, [3, 2, 1, 0, 0, 0]);
        assert!(dirty);
        assert_eq!(
            test.state.world.get_cell(10, 11),
            crate::world::cells::cell_type::EMPTY
        );
    }

    #[tokio::test]
    async fn pickup_box_missing_flags_keeps_box_and_player_crystals() {
        let test = make_box_test_state("missing_flags").await;
        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerFlags>();
        }
        test.state
            .put_box_cell_authoritative(10, 11, [3, 2, 1, 0, 0, 0]);
        let player_pos = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                Some((pos.x, pos.y).into())
            })
            .unwrap();

        let result = apply_box_pickup(
            &test.state,
            crate::game::BoxPickupIntent {
                player_id: pid,
                player_pos,
                box_pos: (10, 11).into(),
                source: crate::game::BoxPickupSource::Standing,
            },
        );

        assert!(matches!(result, BoxPickupApplyResult::Stale));
        let crystals = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                Some(
                    ecs.get::<crate::game::player::PlayerStats>(entity)?
                        .crystals,
                )
            })
            .unwrap();
        assert_eq!(crystals, [0; 6]);
        assert_eq!(
            test.state.world.get_cell(10, 11),
            crate::world::cells::cell_type::BOX
        );
    }
}
