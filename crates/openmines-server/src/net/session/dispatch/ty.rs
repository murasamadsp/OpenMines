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
    handle_aggression_toggle, handle_auto_dig_toggle, handle_prog_ty, handle_sett_ty, handle_whoi,
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
    crate::metrics::TY_EVENTS_TOTAL
        .with_label_values(&[event])
        .inc();
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
                handle_dig(state, tx, pid, dir, false);
            }
        }
        "Xbld" => {
            if let Some(bld) = XbldClient::decode(&packet.sub_payload) {
                handle_build(state, tx, pid, &bld, false);
            }
        }
        "GUI_" => {
            if !state.check_gui_rate(pid) {
                tracing::debug!(player_id = %pid, "gui rate limited (GUI_)");
                return Ok(());
            }
            if let Some(button) = decode_gui_button(&packet.sub_payload) {
                handle_gui_button(state, tx, pid, button.as_ref()).await;
            }
        }
        "Locl" => {
            if !state.check_chat_rate(pid) {
                tracing::debug!(player_id = %pid, "chat rate limited (Locl)");
                return Ok(());
            }
            if let Some(locl) = LoclClient::decode(&packet.sub_payload) {
                tracing::debug!(player_id = %pid, len = locl.length, "Local chat payload decoded");
                handle_local_chat(state, tx, pid, locl.message).await;
            }
        }
        "Xgeo" => {
            handle_geo(state, tx, pid, false);
        }
        "Xhea" => {
            handle_heal(state, tx, pid, false);
        }
        "Xhur" => {
            if is_unit_payload(&packet.sub_payload) {
                tracing::debug!(pid = %pid, "known no-op TY event: self-hurt");
            } else {
                tracing::warn!(pid = %pid, payload = ?packet.sub_payload, "invalid Xhur payload");
            }
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
        "FINV" => {
            if let Some(index) = decode_finv_index(&packet.sub_payload) {
                tracing::debug!(pid = %pid, index, "known no-op TY event: inventory filter hotkey");
            } else {
                tracing::warn!(pid = %pid, payload = ?packet.sub_payload, "invalid FINV payload");
            }
        }
        "TADG" => {
            handle_auto_dig_toggle(state, tx, pid);
        }
        "TAGR" => {
            handle_aggression_toggle(state, tx, pid);
        }
        "Whoi" => {
            if let Some(ids) = decode_whoi(&packet.sub_payload) {
                handle_whoi(state, tx, &ids).await;
            }
        }
        "Chat" => {
            if !state.check_chat_rate(pid) {
                tracing::debug!(player_id = %pid, "chat rate limited (Chat)");
                return Ok(());
            }
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
            if !state.check_chat_rate(pid) {
                tracing::debug!(player_id = %pid, "chat rate limited (Cpri)");
                return Ok(());
            }
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
        // `Miss` / `Rndm`: в `Session.TY` нет case — падают в default.
        // `TAUR`: `Taur` в референсе пустой. Форматы payload всё равно
        // валидируем по packet decoder/client contract.
        "Miss" => {
            if let Some(enabled) = decode_miss_enabled(&packet.sub_payload) {
                tracing::debug!(pid = %pid, enabled, "known no-op TY event: mission init");
            } else {
                tracing::warn!(pid = %pid, payload = ?packet.sub_payload, "invalid Miss payload");
            }
        }
        "Rndm" => {
            if let Some(hash) = decode_rndm_hash(&packet.sub_payload) {
                tracing::debug!(pid = %pid, hash_len = hash.len(), "known no-op TY event: device hash");
            } else {
                tracing::warn!(pid = %pid, payload = ?packet.sub_payload, "invalid Rndm payload");
            }
        }
        "TAUR" => {
            if is_unit_payload(&packet.sub_payload) {
                tracing::debug!(pid = %pid, "known no-op TY event: auto-respawn toggle");
            } else {
                tracing::warn!(pid = %pid, payload = ?packet.sub_payload, "invalid TAUR payload");
            }
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
    let (event, payload) = crate::protocol::packets::mission_panel("", 0, 0, 0, "");
    send_u_packet(tx, event, &payload);
}

fn decode_finv_index(payload: &[u8]) -> Option<u8> {
    match payload {
        [b'0'..=b'9'] => Some(payload[0] - b'0'),
        _ => None,
    }
}

fn is_unit_payload(payload: &[u8]) -> bool {
    payload == b"_"
}

fn decode_miss_enabled(payload: &[u8]) -> Option<bool> {
    match payload {
        b"0" => Some(false),
        b"1" => Some(true),
        _ => None,
    }
}

fn decode_rndm_hash(payload: &[u8]) -> Option<&str> {
    const PREFIX: &[u8] = b"hash=";
    let hash = payload.strip_prefix(PREFIX)?;
    std::str::from_utf8(hash).ok()
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

    #[test]
    fn finv_decodes_only_unity_numeric_hotkeys() {
        for index in 0..=9 {
            let payload = index.to_string();
            assert_eq!(decode_finv_index(payload.as_bytes()), Some(index));
        }

        assert_eq!(decode_finv_index(b""), None);
        assert_eq!(decode_finv_index(b"10"), None);
        assert_eq!(decode_finv_index(b"_"), None);
    }

    #[test]
    fn unit_ty_payload_matches_reference_underscore_packets() {
        assert!(is_unit_payload(b"_"));
        assert!(!is_unit_payload(b""));
        assert!(!is_unit_payload(b"0"));
    }

    #[test]
    fn mission_init_payload_matches_reference_bool_wire() {
        assert_eq!(decode_miss_enabled(b"0"), Some(false));
        assert_eq!(decode_miss_enabled(b"1"), Some(true));
        assert_eq!(decode_miss_enabled(b"_"), None);
        assert_eq!(decode_miss_enabled(b"true"), None);
    }

    #[test]
    fn rndm_payload_requires_hash_prefix() {
        assert_eq!(decode_rndm_hash(b"hash=device-id"), Some("device-id"));
        assert_eq!(decode_rndm_hash(b"device-id"), None);
        assert_eq!(decode_rndm_hash(b"hash=\xff"), None);
    }
}
