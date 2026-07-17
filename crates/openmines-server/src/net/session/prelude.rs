//! Shared imports for `session` submodules.
pub use std::collections::HashSet;
pub use std::net::SocketAddr;
pub use std::sync::Arc;
pub use std::time::{Duration, Instant};

pub use anyhow::Result;
pub use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub use crate::db::BuildingExtra;
pub use crate::game::buildings::validate_pack_access;
pub use crate::game::buildings::{PackType, PackView};
pub use crate::game::chat::{CHAT_HISTORY_LIMIT, dotnet_epoch_minutes};
pub use crate::game::direction::dir_offset;
pub use crate::game::{GameState, PlayerId, SessionId};
pub use crate::protocol::Packet;
pub use crate::protocol::packets::{
    AuAuthType, AuClientPacket, PongClient, TyPacket, aggression, auto_digg, basket, bot_info,
    chat_current, chat_messages, clan_hide, clan_show, config_packet, decode_gui_button, geo,
    gu_close, hand_mode, hb_bot, hb_bot_del, hb_bundle, hb_cell, hb_chat, hb_fx, hb_map, hb_packs,
    health, level, money, ok_message, programmator_status, settings_default_wire, skills_packet,
    speed, tp,
};
pub use crate::world::cells::cell_type;
pub use crate::world::{World, WorldProvider};

pub use super::constants::*;
pub use super::outbox::Outbox;
pub use super::ui::horb::HorbDelivery;
pub use super::util::{net_u8_clamped, net_u16_nonneg};
pub use super::wire::{
    PacketSink, encode_hb_bundle, make_u_packet_bytes, send_b_packet, send_u_packet,
};
