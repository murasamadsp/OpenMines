//! Shared imports for `session` submodules.
pub use std::collections::HashSet;
pub use std::net::SocketAddr;
pub use std::sync::Arc;
pub use std::time::{Duration, Instant};

pub use anyhow::Result;
pub use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub use crate::db::BuildingExtra;
pub use crate::game::buildings::{PackType, PackView};
pub use crate::game::chat::{CHAT_HISTORY_LIMIT, ChatMessage, dotnet_epoch_minutes};
pub use crate::game::direction::dir_offset;
pub use crate::game::skills::{SkillType, get_player_skill_effect};
pub use crate::game::{GameState, PlayerId, SessionId};
pub use crate::protocol::Packet;
pub use crate::protocol::packets::{
    AuAuthType, AuClientPacket, PongClient, TyPacket, XbldClient, aggression, auto_digg, basket,
    bot_info, chat_color, chat_current, chat_list, chat_messages, chat_notification, clan_hide,
    clan_show, config_packet, decode_gui_button, geo, gu_close, hand_mode, hb_bot, hb_bot_del,
    hb_bundle, hb_cell, hb_chat, hb_crystal_mine_fx, hb_dig_fx, hb_fx, hb_gun_shot_fx, hb_hurt_fx,
    hb_map, hb_packs, health, level, money, ok_message, programmator_status, settings_default_wire,
    skills_packet, speed, tp,
};
pub use crate::world::cells::cell_type;
pub use crate::world::{World, WorldProvider};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackAccessError {
    NotAtObject,
    NoRights,
    ConfigMissing,
}

#[inline]
pub fn validate_pack_access(
    view: &PackView,
    player_pos: (i32, i32),
    player_clan: i32,
    pid: PlayerId,
) -> Result<(), PackAccessError> {
    let Ok(cells) = view.pack_type.building_cells() else {
        return Err(PackAccessError::ConfigMissing);
    };
    if !cells
        .iter()
        .any(|(dx, dy, _)| view.x + dx == player_pos.0 && view.y + dy == player_pos.1)
    {
        return Err(PackAccessError::NotAtObject);
    }

    if view.owner_id == pid || (view.clan_id != 0 && view.clan_id == player_clan) {
        Ok(())
    } else {
        Err(PackAccessError::NoRights)
    }
}

pub use super::constants::*;
pub use super::outbox::Outbox;
pub use super::util::{net_u8_clamped, net_u16_nonneg};
pub use super::wire::{
    PacketSink, encode_hb_bundle, make_u_packet_bytes, send_b_packet, send_u_packet,
};
