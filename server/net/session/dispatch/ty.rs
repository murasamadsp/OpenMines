//! Диспетчеризация TY-пакетов (игровые действия).
use crate::net::session::prelude::*;
use crate::net::session::play::movement::handle_move;
use crate::net::session::play::dig_build::{handle_dig, handle_build};
use crate::net::session::social::misc::{handle_local_chat, handle_auto_dig_toggle, handle_geo, handle_whoi};
use crate::net::session::ui::gui_buttons::handle_gui_button;
use crate::net::session::ui::heal_inventory::{handle_heal, handle_inventory_open, handle_inventory_use, handle_inventory_choose};

pub fn dispatch_ty_packet(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    packet: &TyPacket,
) -> Result<()> {
    let event = packet.event_str();
    match event {
        "XMOV" => {
            if let Some(dir) = decode_xmov(&packet.sub_payload) {
                handle_move(state, tx, pid, dir);
            }
        }
        "XDIG" => {
            if let Some(dir) = decode_xdig(&packet.sub_payload) {
                handle_dig(state, tx, pid, dir);
            }
        }
        "XBLD" => {
            if let Some(bld) = XbldClient::decode(&packet.sub_payload) {
                handle_build(state, tx, pid, &bld);
            }
        }
        "GUI_" => {
            if let Some(button) = decode_gui_button(&packet.sub_payload) {
                handle_gui_button(state, tx, pid, &button);
            }
        }
        "LOCL" => {
            let msg = String::from_utf8_lossy(&packet.sub_payload);
            handle_local_chat(state, tx, pid, &msg);
        }
        "GEOP" => {
            handle_geo(state, tx, pid);
        }
        "HEAL" => {
            handle_heal(state, tx, pid);
        }
        "INVO" => {
            handle_inventory_open(state, tx, pid);
        }
        "INVU" => {
            handle_inventory_use(state, tx, pid);
        }
        "INVC" => {
            handle_inventory_choose(state, tx, pid, &packet.sub_payload);
        }
        "ADIG" => {
            handle_auto_dig_toggle(state, tx, pid);
        }
        "WHOI" => {
            let ids = decode_whoi(&packet.sub_payload);
            handle_whoi(state, tx, &ids);
        }
        _ => {
            tracing::warn!("Unknown TY event: {event}");
        }
    }
    Ok(())
}
