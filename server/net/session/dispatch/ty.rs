//! Диспетчеризация TY-пакетов (игровые действия).
use crate::net::session::play::death::handle_death;
use crate::net::session::play::dig_build::{handle_build, handle_dig};
use crate::net::session::play::geo::handle_geo;
use crate::net::session::play::movement::handle_move;
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::{
    handle_dpbx_crystal_box, handle_my_buildings_list, handle_programmator_pope_menu,
};
use crate::net::session::social::chat::{
    handle_channel_chat, handle_chat_choose, handle_chat_menu, handle_chat_private,
    handle_chat_resync, handle_chat_settings, handle_local_chat,
};
use crate::net::session::social::clans::handle_clan_menu;
use crate::net::session::social::commands::{is_admin_command, send_admin_help, send_ok};
use crate::net::session::social::misc::{
    handle_auto_dig_toggle, handle_prog_ty, handle_sett_ty, handle_whoi,
};
use crate::net::session::ui::gui_buttons::handle_gui_button;
use crate::net::session::ui::heal_inventory::{
    handle_heal, handle_inventory_choose, handle_inventory_use, handle_invn_toggle,
};

pub async fn dispatch_ty_packet(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    packet: &TyPacket,
) -> Result<()> {
    let event = packet.event_str();
    let __d0 = std::time::Instant::now();
    // Имена событий 1:1 с референсом (case-sensitive!).
    match event {
        "Xmov" => {
            if let Some(dir) = decode_xmov(&packet.sub_payload) {
                handle_move(
                    state,
                    tx,
                    pid,
                    packet.time,
                    packet.x as i32,
                    packet.y as i32,
                    dir,
                    false, // ручной ход игрока (не программатор)
                );
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
                handle_gui_button(state, tx, pid, button.as_ref()).await;
            }
        }
        "Locl" => {
            if let Some(locl) = LoclClient::decode(&packet.sub_payload) {
                handle_local_chat(state, tx, pid, locl.message).await;
            }
        }
        "Xgeo" => {
            handle_geo(state, tx, pid);
        }
        "Xhea" => {
            handle_heal(state, tx, pid);
        }
        "INVN" => {
            handle_invn_toggle(state, tx, pid);
        }
        "INUS" => {
            handle_inventory_use(state, tx, pid).await;
        }
        "INCL" => {
            handle_inventory_choose(state, tx, pid, &packet.sub_payload);
        }
        "TADG" => {
            handle_auto_dig_toggle(state, tx, pid);
        }
        "Whoi" => {
            if let Some(ids) = decode_whoi(&packet.sub_payload) {
                handle_whoi(state, tx, &ids).await;
            }
        }
        "Chat" => {
            handle_channel_chat(state, tx, pid, &packet.sub_payload).await;
        }
        "Chin" => {
            // Ресинк чата по `getLasts()` клиента. Реф `Session.Chin` ПУСТ
            // (реф неполон — клиент шлёт `lasts` для инкрементальной
            // догрузки, реф это не реализовал). НИКОГДА не слать `mL`
            // (это ломало вход в чат — прежняя итерация). История —
            // здесь, не в login (иначе дубли на реконнекте).
            // docs/CLIENT_PROTOCOL_GAPS.md §2.
            handle_chat_resync(state, tx, pid, &packet.sub_payload).await;
        }
        // Навигация чата — НЕТ в server_reference (Session.cs только пустой
        // Chin; TYPacket.cs декодит, но `default: //Invalid`). Контракт
        // восстановлен по клиенту. docs/CLIENT_PROTOCOL_GAPS.md §3–6.
        "Cmen" => {
            handle_chat_menu(state, tx, pid, &packet.sub_payload).await;
        }
        "Choo" => {
            handle_chat_choose(state, tx, pid, &packet.sub_payload).await;
        }
        "Cset" => {
            handle_chat_settings(state, tx, pid, &packet.sub_payload).await;
        }
        "Cpri" => {
            handle_chat_private(state, tx, pid, &packet.sub_payload).await;
        }
        "RESP" => {
            handle_death(state, tx, pid);
        }
        "Pope" => {
            handle_programmator_pope_menu(state, tx, pid).await;
        }
        "Blds" => {
            handle_my_buildings_list(state, tx, pid).await;
        }
        "Clan" => {
            handle_clan_menu(state, tx, pid).await;
        }
        "Sett" => {
            handle_sett_ty(state, tx, pid, &packet.sub_payload);
        }
        "ADMN" => {
            // C# ref: ADMN triggers AdminButton() on current window (gear icon).
            // Check if player has a building window open with admin support.
            let handled = state.query_player_expected(pid, "ADMN packet", |ecs, entity| {
                let ui = ecs.get::<crate::game::player::PlayerUI>(entity)?;
                ui.current_window.clone()
            });
            if let Some(ref window) = handled {
                if let Some(coords) = window.strip_prefix("resp:") {
                    let parts: Vec<&str> = coords.split(':').collect();
                    if parts.len() == 2 {
                        if let (Ok(x), Ok(y)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                            crate::net::session::play::packs::open_resp_admin_gui(
                                state, tx, pid, x, y,
                            );
                            return Ok(());
                        }
                    }
                }
                // Market admin: gear icon opens admin panel (RichList with hp + profit)
                if let Some(rest) = window.strip_prefix("market:") {
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() >= 2 {
                        if let (Ok(x), Ok(y)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                            crate::net::session::ui::gui_buttons::open_market_admin_gui(
                                state, tx, pid, x, y,
                            );
                            return Ok(());
                        }
                    }
                }
                // Up admin: C# Up.OnAdmin opens a small page with hp/maxhp.
                if let Some(rest) = window.strip_prefix("up:") {
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() >= 2 {
                        if let (Ok(x), Ok(y)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                            crate::net::session::ui::up_building::open_up_admin_gui(
                                state, tx, pid, x, y,
                            );
                            return Ok(());
                        }
                    }
                }
                // Generic pack admin: шестерёнка любого пака → единая панель
                // (прочность/заряд/стоимость/закланить/прибыль).
                if let Some(rest) = window.strip_prefix("pack:") {
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() == 2 {
                        if let (Ok(x), Ok(y)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                            crate::net::session::ui::gui_buttons::open_pack_admin_gui(
                                state, tx, pid, x, y,
                            );
                            return Ok(());
                        }
                    }
                }
            }
            if is_admin_command(state, pid) {
                send_admin_help(tx);
            } else {
                send_ok(tx, "Ошибка", "Нет прав администратора.");
            }
        }
        "DPBX" => {
            handle_dpbx_crystal_box(state, tx, pid);
        }
        // `GDon` — кнопка БОНУСЫ (донат в C# = заглушка; перепрофилировано в
        // ежедневный бонус по требованию пользователя). См. `play::bonus`.
        "GDon" => {
            crate::net::session::play::bonus::handle_bonus_claim(state, tx, pid);
        }
        // Client Help button disables movement before sending `Help`; C# decodes
        // the packet but has no TY handler. Make the disabled feature explicit
        // instead of leaving the UI with a silent click.
        "Help" => {
            send_ok(tx, "Справка", "Справка пока не подключена на сервере.");
        }
        // Mission panel click. We do not have mission content yet, but the client
        // has a standard MM empty-text contract for hiding the panel.
        "Miso" => {
            send_mission_panel_hide(tx);
        }
        // Tutorial marker hidden client-side; keep the event known for telemetry.
        "THID" => {
            let marker = String::from_utf8_lossy(&packet.sub_payload);
            tracing::debug!(pid = %pid, marker = %marker, "tutorial marker hidden");
        }
        "PROG" | "PDEL" | "pRST" | "PREN" | "PCOP" => {
            handle_prog_ty(state, tx, pid, packet.event_str(), &packet.sub_payload).await;
        }
        // `Miss` / `Rndm`: в `Session.TY` нет case — падают в default (как здесь).
        // `TAGR` / `TAUR`: `Agr` / `Taur` в референсе пустые.
        "Miss" | "Rndm" | "TAGR" | "TAUR" => {
            tracing::debug!(pid = %pid, event, "known no-op TY event");
        }
        _ => {
            tracing::warn!(event, "Unknown TY event");
        }
    }
    let __el = __d0.elapsed();
    if __el > std::time::Duration::from_millis(50) {
        tracing::warn!(target: "tickprof", event, player_id = %pid, elapsed = ?__el, "Slow TY packet handler");
    }
    Ok(())
}

fn send_mission_panel_hide(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(tx, "MM", b"#0#0#0#");
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn mission_panel_hide_sends_empty_mm_payload() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        send_mission_panel_hide(&tx);

        let frame = rx.try_recv().expect("MM frame");
        let mut buf = BytesMut::from(&frame[..]);
        let packet = crate::protocol::Packet::try_decode(&mut buf)
            .expect("valid packet")
            .expect("decoded packet");

        assert_eq!(packet.data_type, b'U');
        assert_eq!(packet.event_str(), "MM");
        assert_eq!(&packet.payload[..], b"#0#0#0#");
        assert!(buf.is_empty());
    }
}
