//! Разбор пакета `TY` и вызов соответствующих обработчиков.

use crate::net::session::play::dig_build::{handle_build, handle_dig};
use crate::net::session::play::movement::handle_move;
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::handle_buildings_menu;
use crate::net::session::social::clans::handle_clan_menu;
use crate::net::session::social::misc::{
    handle_auto_dig_toggle, handle_channel_chat, handle_chat_message, handle_chat_switch,
    handle_death, handle_geo, handle_local_chat, handle_whoi,
};
use crate::net::session::ui::gui_buttons::handle_gui_button;
use crate::net::session::ui::heal_inventory::{
    handle_heal, handle_inventory_choose, handle_inventory_open, handle_inventory_use,
};

const TY_SUBPAYLOAD_PREVIEW_LEN: usize = 80;

pub fn handle_ty(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    ty: &TyPacket,
) {
    let ev = ty.event_str();
    crate::metrics::TY_EVENTS_TOTAL
        .with_label_values(&[ev])
        .inc();
    tracing::trace!(event = %ev, client_t = ty.client_timestamp(), x = ty.x, y = ty.y, "TY");
    match ev {
        "Xmov" => {
            handle_xmov(state, tx, pid, ty);
        }
        "Xdig" => {
            handle_xdig(state, tx, pid, ty);
        }
        "Xgeo" => {
            handle_geo(state, tx, pid);
        }
        "Xhea" => {
            handle_heal(state, tx, pid);
        }
        "Xbld" => {
            handle_xbld(state, tx, pid, ty);
        }
        "Locl" => {
            handle_locl(state, tx, pid, ty);
        }
        "Whoi" => {
            let ids = decode_whoi(&ty.sub_payload);
            tracing::debug!("TY Whoi pid={pid} ids_count={}", ids.len());
            handle_whoi(state, tx, &ids);
        }
        "TADG" => {
            tracing::debug!("TY TADG pid={pid}");
            handle_auto_dig_toggle(state, tx, pid);
        }
        "GUI_" => {
            if let Some(button) = decode_gui_button(&ty.sub_payload) {
                tracing::debug!("TY GUI_ pid={pid} button_len={}", button.len());
                handle_gui_button(state, tx, pid, &button);
            }
        }
        "RESP" => {
            tracing::debug!("TY RESP pid={pid}");
            handle_death(state, tx, pid);
        }
        "INVN" => {
            tracing::debug!("TY INVN pid={pid}");
            handle_inventory_open(state, tx, pid);
        }
        "INUS" => {
            tracing::debug!("TY INUS pid={pid}");
            handle_inventory_use(state, tx, pid);
        }
        "INCL" => {
            tracing::debug!(
                "TY INCL pid={pid} payload={:?}",
                ty_sub_payload_preview(&ty.sub_payload)
            );
            handle_inventory_choose(state, tx, pid, &ty.sub_payload);
        }
        "Blds" => {
            tracing::debug!("TY Blds pid={pid}");
            handle_buildings_menu(state, tx, pid);
        }
        "Msg" => {
            tracing::debug!("TY Msg pid={pid}");
            handle_chat_message(state, tx, pid, &ty.sub_payload);
        }
        "Clan" => {
            tracing::debug!("TY Clan pid={pid}");
            handle_clan_menu(state, tx, pid);
        }
        "Chat" => {
            tracing::debug!("TY Chat pid={pid}");
            handle_channel_chat(state, tx, pid, &ty.sub_payload);
        }
        "Chin" => {
            tracing::debug!("TY Chin pid={pid}");
            handle_chat_switch(state, tx, pid, &ty.sub_payload);
        }
        "Sett" | "DPBX" | "ADMN" => {
            send_ty_not_implemented(
                tx,
                pid,
                ev,
                "Unimplemented TY event",
                "Событие",
                "Команда пока не реализована",
            );
        }
        "Pope" => {
            tracing::debug!("GUI POPE event pid={pid}");
            send_u_packet(tx, "@P", b"0");
        }
        "PROG" | "PDEL" | "pRST" | "PREN" | "TAGR" | "TAUR" => {
            send_ty_not_implemented(
                tx,
                pid,
                ev,
                "Stub TY event",
                "Программатор",
                "В этой версии серверной логики ещё не поддерживается",
            );
        }
        _ => {
            tracing::debug!("Unknown TY: {ev}");
        }
    }
}

fn warn_ty_parse_failure(event: &str, pid: PlayerId, x: i32, y: i32, payload: &[u8]) {
    tracing::warn!(
        "Failed to parse {event} pid={pid} payload={:?} x={x} y={y}",
        ty_sub_payload_preview(payload),
    );
}

fn ty_sub_payload_preview(payload: &[u8]) -> String {
    String::from_utf8_lossy(&payload[..payload.len().min(TY_SUBPAYLOAD_PREVIEW_LEN)]).into()
}

fn send_ty_not_implemented(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    event: &str,
    debug_prefix: &str,
    title: &str,
    msg: &str,
) {
    tracing::debug!("{debug_prefix}: {event} from player {pid}");
    send_u_packet(tx, "OK", &ok_message(title, msg).1);
}

fn handle_xmov(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    ty: &TyPacket,
) {
    let raw_dir = String::from_utf8_lossy(&ty.sub_payload);
    if let Some(dir) = decode_xmov(&ty.sub_payload) {
        tracing::debug!("TY Xmov pid={pid} raw_dir={raw_dir} x={} y={}", ty.x, ty.y);
        if let (Ok(x), Ok(y)) = (i32::try_from(ty.x), i32::try_from(ty.y)) {
            handle_move(state, tx, pid, x, y, dir);
        } else {
            tracing::warn!("TY Xmov out of i32 range pid={pid} x={} y={}", ty.x, ty.y);
        }
    } else {
        tracing::warn!(
            "Failed to parse Xmov pid={pid} payload={raw_dir:?} x={} y={}",
            ty.x,
            ty.y
        );
    }
}

fn handle_xdig(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    ty: &TyPacket,
) {
    if let Some(dir) = decode_xdig(&ty.sub_payload) {
        tracing::debug!("TY Xdig pid={pid} dir={} x={} y={}", dir, ty.x, ty.y);
        handle_dig(state, tx, pid, dir);
    } else if let (Ok(x), Ok(y)) = (i32::try_from(ty.x), i32::try_from(ty.y)) {
        warn_ty_parse_failure("Xdig", pid, x, y, &ty.sub_payload);
    } else {
        tracing::warn!("TY Xdig out of i32 range pid={pid} x={} y={}", ty.x, ty.y);
    }
}

fn handle_xbld(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    ty: &TyPacket,
) {
    if let Some(bld) = XbldClient::decode(&ty.sub_payload) {
        tracing::debug!(
            "TY Xbld pid={pid} dir={} block={} x={} y={}",
            bld.direction,
            bld.block_type,
            ty.x,
            ty.y
        );
        handle_build(state, tx, pid, &bld);
    } else if let (Ok(x), Ok(y)) = (i32::try_from(ty.x), i32::try_from(ty.y)) {
        warn_ty_parse_failure("Xbld", pid, x, y, &ty.sub_payload);
    } else {
        tracing::warn!("TY Xbld out of i32 range pid={pid} x={} y={}", ty.x, ty.y);
    }
}

fn handle_locl(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    ty: &TyPacket,
) {
    if let Some(locl) = LoclClient::decode(&ty.sub_payload) {
        let actual = i32::try_from(locl.message.len()).unwrap_or(i32::MAX);
        if actual != locl.length {
            tracing::warn!(declared = locl.length, actual, "Locl length mismatch");
        }
        handle_local_chat(state, tx, pid, &locl.message);
    } else if let (Ok(x), Ok(y)) = (i32::try_from(ty.x), i32::try_from(ty.y)) {
        warn_ty_parse_failure("Locl", pid, x, y, &ty.sub_payload);
    } else {
        tracing::warn!("TY Locl out of i32 range pid={pid} x={} y={}", ty.x, ty.y);
    }
}
