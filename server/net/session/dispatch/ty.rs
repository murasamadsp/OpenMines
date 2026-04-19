//! Диспетчеризация TY-пакетов (игровые действия).
use crate::net::session::prelude::*;
use crate::net::session::play::movement::handle_move;
use crate::net::session::play::dig_build::{handle_dig, handle_build};
use crate::net::session::social::buildings::{
    handle_dpbx_crystal_box, handle_my_buildings_list, handle_programmator_pope_menu,
};
use crate::net::session::social::clans::handle_clan_menu;
use crate::net::session::social::misc::{
    handle_auto_dig_toggle, handle_channel_chat, handle_chat_init_ty, handle_death, handle_geo,
    handle_local_chat, handle_prog_ty, handle_sett_ty, handle_whoi, is_admin_command,
    send_admin_help, send_ok,
};
use crate::net::session::ui::gui_buttons::handle_gui_button;
use crate::net::session::ui::heal_inventory::{handle_heal, handle_inventory_open, handle_inventory_use, handle_inventory_choose};

pub fn dispatch_ty_packet(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    packet: &TyPacket,
) -> Result<()> {
    let event = packet.event_str();
    // Имена событий 1:1 с референсом (case-sensitive!).
    match event {
        "Xmov" => {
            if let Some(dir) = decode_xmov(&packet.sub_payload) {
                handle_move(state, tx, pid, dir);
            }
        }
        "Xdig" => {
            if let Some(dir) = decode_xdig(&packet.sub_payload) {
                handle_dig(state, tx, pid, dir);
            }
        }
        "Xbld" => {
            if let Some(bld) = XbldClient::decode(&packet.sub_payload) {
                handle_build(state, tx, pid, &bld);
            }
        }
        "GUI_" => {
            if let Some(button) = decode_gui_button(&packet.sub_payload) {
                handle_gui_button(state, tx, pid, &button);
            }
        }
        "Locl" => {
            if let Some(locl) = LoclClient::decode(&packet.sub_payload) {
                handle_local_chat(state, tx, pid, &locl.message);
            }
        }
        "Xgeo" => {
            handle_geo(state, tx, pid);
        }
        "Xhea" => {
            handle_heal(state, tx, pid);
        }
        "INVN" => {
            handle_inventory_open(state, tx, pid);
        }
        "INUS" => {
            handle_inventory_use(state, tx, pid);
        }
        "INCL" => {
            handle_inventory_choose(state, tx, pid, &packet.sub_payload);
        }
        "TADG" => {
            handle_auto_dig_toggle(state, tx, pid);
        }
        "Whoi" => {
            let ids = decode_whoi(&packet.sub_payload);
            handle_whoi(state, tx, &ids);
        }
        "Chat" => {
            handle_channel_chat(state, tx, pid, &packet.sub_payload);
        }
        "Chin" => {
            handle_chat_init_ty(state, tx, pid, &packet.sub_payload);
        }
        "RESP" => {
            handle_death(state, tx, pid);
        }
        "Pope" => {
            handle_programmator_pope_menu(state, tx, pid);
        }
        "Blds" => {
            handle_my_buildings_list(state, tx, pid);
        }
        "Clan" => {
            handle_clan_menu(state, tx, pid);
        }
        "Sett" => {
            handle_sett_ty(state, tx, pid, &packet.sub_payload);
        }
        "ADMN" => {
            if is_admin_command(state, pid) {
                send_admin_help(tx);
            } else {
                send_ok(tx, "Ошибка", "Нет прав администратора.");
            }
        }
        "DPBX" => {
            handle_dpbx_crystal_box(state, tx, pid);
        }
        "PROG" | "PDEL" | "pRST" | "PREN" => {
            handle_prog_ty(tx, packet.event_str(), &packet.sub_payload);
        }
        "Miss" | "Rndm" | "TAGR" | "TAUR" => {}
        _ => {
            tracing::warn!("Unknown TY event: {event}");
        }
    }
    Ok(())
}
