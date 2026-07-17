//! Capability-limited view used by simulation command application.

use crate::game::{EcsWorld, GameState, PlayerId};
use crate::world::WorldProvider;
use bevy_ecs::entity::Entity;
use std::sync::Arc;

/// Deliberately excludes database, Tokio and session outboxes.
pub struct KernelContext<'a> {
    state: &'a Arc<GameState>,
}

impl<'a> KernelContext<'a> {
    pub(crate) const fn new(state: &'a Arc<GameState>) -> Self {
        Self { state }
    }

    pub(crate) fn modify_player<F, R>(&self, player_id: PlayerId, mutation: F) -> Option<R>
    where
        F: FnOnce(&mut EcsWorld, Entity) -> R,
    {
        self.state.modify_player(player_id, mutation)
    }

    pub(crate) fn is_admin(&self, player_id: PlayerId) -> bool {
        self.state
            .query_player(player_id, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerStats>(entity)
                    .is_some_and(|stats| stats.role == 2)
            })
            .unwrap_or(false)
    }

    pub(crate) fn resolve_online_player(
        &self,
        self_id: PlayerId,
        target: &str,
    ) -> Option<PlayerId> {
        if target.eq_ignore_ascii_case("me") || target == "self" {
            return Some(self_id);
        }
        if let Ok(player_id) = target.parse::<PlayerId>() {
            return self.state.is_player_active(player_id).then_some(player_id);
        }
        self.state
            .active_player_ids()
            .into_iter()
            .find(|&player_id| {
                self.state
                    .query_player(player_id, |ecs, entity| {
                        ecs.get::<crate::game::player::PlayerMetadata>(entity)
                            .is_some_and(|metadata| metadata.name.eq_ignore_ascii_case(target))
                    })
                    .unwrap_or(false)
            })
    }

    pub(crate) fn player_session(&self, player_id: PlayerId) -> Option<crate::game::SessionId> {
        self.state.sessions.session_for_player(player_id)
    }

    pub(crate) fn kick_player(&self, player_id: PlayerId) -> bool {
        self.state.kick_player(player_id)
    }

    pub(crate) fn reserve_clan_create(&self, player_id: PlayerId) -> Result<(), &'static str> {
        self.modify_player(player_id, |ecs, entity| {
            {
                let Some(mut stats) = ecs.get_mut::<crate::game::player::PlayerStats>(entity)
                else {
                    return Err("Состояние игрока недоступно.");
                };
                if stats.clan_id.is_some() {
                    return Err("Вы уже в клане");
                }
                if stats.creds < 1_000 {
                    return Err("Недостаточно кредитов (нужно 1000)");
                }
                stats.creds -= 1_000;
            }
            let Some(mut flags) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) else {
                return Err("Состояние игрока недоступно.");
            };
            flags.dirty = true;
            Ok(())
        })
        .unwrap_or(Err("Состояние игрока недоступно."))
    }

    pub(crate) fn refund_clan_create(&self, player_id: PlayerId) {
        let _ = self.modify_player(player_id, |ecs, entity| {
            if let Some(mut stats) = ecs.get_mut::<crate::game::player::PlayerStats>(entity) {
                stats.creds = stats.creds.saturating_add(1_000);
            }
            if let Some(mut flags) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
                flags.dirty = true;
            }
        });
    }

    pub(crate) fn teleport_player(&self, player_id: PlayerId, x: i32, y: i32) -> bool {
        self.modify_player(player_id, |ecs, entity| {
            if ecs
                .get::<crate::game::player::PlayerPosition>(entity)
                .is_none()
                || ecs.get::<crate::game::player::PlayerUI>(entity).is_none()
                || ecs.get::<crate::game::player::PlayerView>(entity).is_none()
                || ecs
                    .get::<crate::game::player::PlayerFlags>(entity)
                    .is_none()
            {
                return false;
            }
            {
                let Some(mut position) = ecs.get_mut::<crate::game::player::PlayerPosition>(entity)
                else {
                    return false;
                };
                position.x = x;
                position.y = y;
            }
            {
                let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) else {
                    return false;
                };
                ui.current_window = None;
            }
            {
                let Some(mut view) = ecs.get_mut::<crate::game::player::PlayerView>(entity) else {
                    return false;
                };
                view.last_chunk = None;
                view.visible_chunks.clear();
            }
            let Some(mut flags) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) else {
                return false;
            };
            flags.dirty = true;
            true
        })
        .unwrap_or(false)
    }

    pub(crate) fn modify_pack<F, R>(&self, x: i32, y: i32, mutation: F) -> Result<R, String>
    where
        F: FnOnce(&mut EcsWorld, Entity) -> R,
    {
        let entity = self
            .state
            .building_entity_at(x, y)
            .ok_or_else(|| "Объект не найден".to_owned())?;
        let mut ecs = self.state.ecs_write_profiled("kernel_context.modify_pack");
        if ecs
            .get::<crate::game::buildings::BuildingFlags>(entity)
            .is_none()
        {
            return Err("Состояние здания недоступно".to_owned());
        }
        let result = mutation(&mut ecs, entity);
        ecs.get_mut::<crate::game::buildings::BuildingFlags>(entity)
            .ok_or_else(|| "Состояние здания недоступно".to_owned())?
            .dirty = true;
        ecs.resource_mut::<crate::game::DirtyBuildings>()
            .0
            .insert(entity);
        drop(ecs);
        Ok(result)
    }

    pub(crate) fn pack_at(&self, x: i32, y: i32) -> Option<crate::game::PackView> {
        self.state.get_pack_at(x, y)
    }

    pub(crate) fn validate_pack_move(
        &self,
        view: &crate::game::PackView,
        x: i32,
        y: i32,
    ) -> Result<(), &'static str> {
        crate::net::session::social::buildings::validate_pack_footprint(
            self.state,
            view,
            x,
            y,
            view.pack_type,
        )
    }

    pub(crate) fn move_pack_index_and_cells(
        &self,
        old_view: &crate::game::PackView,
        x: i32,
        y: i32,
    ) {
        self.state
            .move_building_entity(old_view.x, old_view.y, x, y);
        crate::net::session::social::buildings::move_pack_cells(self.state, old_view, x, y);
    }

    pub(crate) fn geo_cooldown_ms(&self) -> u64 {
        self.state.config.gameplay.cooldowns.geo_ms
    }

    pub(crate) fn cell_defs(&self) -> std::sync::Arc<crate::world::cells::CellDefs> {
        self.state.world.cell_defs()
    }

    pub(crate) fn world_valid_coord(&self, x: i32, y: i32) -> bool {
        self.state.world.valid_coord(x, y)
    }

    pub(crate) fn world_cell(&self, x: i32, y: i32) -> crate::world::CellType {
        self.state.world.get_cell_typed(x, y)
    }

    pub(crate) fn destroy_world_cell(&self, x: i32, y: i32) {
        self.state.world.destroy(x, y);
    }

    pub(crate) fn write_world_cell(&self, x: i32, y: i32, cell: crate::world::WorldCell) {
        self.state.world.write_world_cell(x, y, cell);
    }

    pub(crate) fn access_gun_full(
        &self,
        ecs: &EcsWorld,
        x: i32,
        y: i32,
        clan_id: i32,
    ) -> (bool, bool) {
        self.state.access_gun_full_in_ecs(ecs, x, y, clan_id)
    }

    pub(crate) fn pack_covering(&self, ecs: &EcsWorld, x: i32, y: i32) -> Option<(i32, i32)> {
        self.state.find_pack_covering_in_ecs(ecs, x, y)
    }

    pub(crate) fn exp_context(&self) -> crate::game::ExpContext {
        crate::game::ExpContext::from_state(self.state)
    }

    #[allow(dead_code)]
    pub(crate) fn queue_cell_update(&self, x: i32, y: i32) {
        self.state.queue_cell_update(x, y);
    }

    #[allow(dead_code)]
    pub(crate) fn queue_heal_fx(&self, x: i32, y: i32, player_id: PlayerId) {
        let effect = crate::protocol::packets::hb_heal_fx(
            crate::net::session::util::net_u16_nonneg(player_id),
        );
        self.state.queue_hb_at(x, y, &[effect], None);
    }

    pub(crate) fn slash_ok_effect(
        &self,
        session_id: crate::game::SessionId,
        _player_id: PlayerId,
        title: &str,
        message: &str,
    ) -> crate::game::CommandEffects {
        if let Some(tx) = self.state.sessions.outbox_for_session(session_id) {
            crate::net::session::wire::send_u_packet(
                &tx,
                "OK",
                &crate::protocol::packets::ok_message(title, message).1,
            );
        }
        crate::game::CommandEffects::default()
    }
}
