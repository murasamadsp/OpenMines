//! Авторизация по пакету AU и начальный UI входа.
use crate::net::session::auth::gui_flow::send_initial_auth_screen;
use crate::net::session::player::init::init_player;
use crate::net::session::prelude::*;

// ─── Auth ───────────────────────────────────────────────────────────────────

pub enum AuthState {
    WaitingAu,
    ShowingGui,
    Authenticated,
}

pub fn handle_auth(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    au: &AuClientPacket,
    sid: &str,
    auth_state: &mut AuthState,
) -> Result<Option<PlayerId>> {
    tracing::trace!(uniq_len = au.client_uniq().len(), "AU auth packet");
    // Send world info
    let w = state.world.cells_width();
    let h = state.world.cells_height();
    let name = state.map_profile_name();
    let world = world_info(&name, w, h, 0, "M3R", "http://localhost/", "ok");
    send_u_packet(tx, world.0, &world.1);

    match &au.auth_type {
        AuAuthType::Regular { user_id, token } => {
            // Token auth: verify hash(hash + sid) == token
            if let Ok(Some(player)) = state.db.get_player_by_id(*user_id) {
                let expected = GameState::auth_token_hash(&format!("{}{}", player.hash, sid));
                if GameState::token_matches(token, &expected) {
                    let pid = init_player(state, tx, &player);
                    *auth_state = AuthState::Authenticated;
                    return Ok(Some(pid));
                }
            }
            // Bad auth — show GUI
            Ok(show_auth_gui(
                tx,
                state,
                au.client_uniq(),
                auth_state,
                "RegularInvalid",
            ))
        }
        AuAuthType::NoAuth => Ok(show_auth_gui(
            tx,
            state,
            au.client_uniq(),
            auth_state,
            "NoAuth",
        )),
        AuAuthType::ServerSide => Ok(show_auth_gui(
            tx,
            state,
            au.client_uniq(),
            auth_state,
            "ServerSide",
        )),
    }
}

fn show_auth_gui(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    state: &Arc<GameState>,
    uniq: &str,
    auth_state: &mut AuthState,
    source: &str,
) -> Option<PlayerId> {
    log_and_show_auth_ui(tx, state, uniq, source);
    *auth_state = AuthState::ShowingGui;
    None
}

pub fn log_and_show_auth_ui(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    state: &Arc<GameState>,
    uniq: &str,
    source: &str,
) {
    tracing::info!(uniq_len = uniq.len(), %source, "AU auth flow switched to username/password UI");
    send_u_packet(tx, "BI", &bot_info("system", 0, 0, -1).1);
    send_first_chunk(state, tx);
    send_initial_auth_screen(tx);
}

pub fn send_first_chunk(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>) {
    let cells = state.world.read_chunk_cells(0, 0);
    let sub = hb_map(0, 0, 32, 32, &cells);
    send_b_packet(tx, "HB", &hb_bundle(&[sub]).1);
}
