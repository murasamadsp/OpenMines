//! Shared imports for `session` submodules.
pub use std::collections::HashSet;
pub use std::net::SocketAddr;
pub use std::ops::ControlFlow;
pub use std::sync::Arc;
pub use std::time::{Duration, Instant};

pub use anyhow::Result;
pub use bytes::BytesMut;
pub use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub use tokio::net::TcpStream;
pub use tokio::sync::mpsc;

pub use crate::db::BuildingExtra;
pub use crate::game::crafting::{Recipe, recipe_by_id, recipes};
pub use crate::game::direction::dir_offset;
pub use crate::game::skills::skill_progress_payload;
pub use crate::game::skills::{SkillType, add_skill_exp, get_player_skill_effect};
pub use crate::game::chat::ChatMessage;
pub use crate::game::buildings::{PackView, PackType};
pub use crate::game::{ActivePlayer, GameState, PlayerId};
pub use crate::protocol::Packet;
pub use crate::protocol::packets::{
    AuAuthType, AuClientPacket, LoclClient, PongClient, TyPacket, XbldClient, au_session,
    auth_hash, auto_digg, basket, bot_info, chat_current, chat_list, chat_messages,
    chat_notification, clan_hide, clan_show, config_packet, decode_gui_button, decode_whoi,
    decode_xdig, decode_xmov, geo, hb_bot, hb_bundle, hb_cell, hb_chat, hb_directed_fx, hb_fx,
    hb_map, hb_packs, health, inventory_close, inventory_show, level, money, ok_message, online,
    ping, programmator_status, skills_packet, speed, status, tp, world_info,
};
pub use crate::world::{World, WorldProvider};
pub use crate::world::cells::{
    cell_type, crystal_multiplier, crystal_type, is_boulder, is_crystal,
};

pub const CLOSE_WINDOW_BUTTON_LABELS: [&str; 2] = ["ВЫЙТИ", "exit"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackAccessError {
    NotAtObject,
    NoRights,
}

#[inline]
pub fn is_pack_owner_or_clan_member(
    state: &Arc<GameState>,
    pid: PlayerId,
    view: &PackView,
) -> bool {
    let player_clan = state.query_player(pid, |world, entity| {
        world.get::<crate::game::PlayerStats>(entity).and_then(|s| s.clan_id)
    }).flatten().unwrap_or(0);

    view.owner_id == pid || (view.clan_id != 0 && view.clan_id == player_clan)
}

#[inline]
pub fn validate_pack_access(
    view: &PackView,
    player_pos: (i32, i32),
    player_clan: i32,
    pid: PlayerId,
) -> Result<(), PackAccessError> {
    if !view
        .pack_type
        .building_cells()
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
pub use super::util::{net_u8_clamped, net_u16_nonneg};
pub use super::wire::{
    INCOMING_PACKET_PREVIEW, describe_wire_packet, encode_hb_bundle, make_u_packet_bytes,
    send_b_packet, send_u_packet,
};
