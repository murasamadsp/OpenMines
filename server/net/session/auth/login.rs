//! Логика авторизации игрока (пакет AU).
use crate::db::players::PlayerRow;
use crate::game::player::PlayerId;
use crate::net::session::player::init::init_player;
use crate::net::session::prelude::*;

pub async fn handle_auth(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    au: &AuClientPacket,
    sid: &str,
    auth_state: &mut crate::net::session::connection::AuthState,
) -> Result<Option<PlayerId>> {
    tracing::trace!(uniq_len = au.client_uniq().len(), "AU auth packet");
    
    let w = state.world.cells_width();
    let h = state.world.cells_height();
    let name = GameState::map_profile_name(au.client_uniq());
    let world = world_info(&name, w, h, 0, "M3R", "http://localhost/", "ok");
    send_u_packet(tx, world.0, &world.1);

    match &au.auth_type {
        AuAuthType::Regular { user_id, token } => {
            if let Ok(Some(player)) = state.db.get_player_by_id(*user_id) {
                let expected = GameState::auth_token_hash(&player.hash);
                if GameState::token_matches(token, &expected) {
                    let pid = init_player(state, tx, &player);
                    *auth_state = crate::net::session::connection::AuthState::Authenticated;
                    return Ok(Some(pid));
                }
            }
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Неверный ID или токен").1);
        }
        AuAuthType::ServerSide => {
            if let Ok(Some(player)) = state.db.get_player_by_name(au.client_uniq()) {
                let pid = init_player(state, tx, &player);
                *auth_state = crate::net::session::connection::AuthState::Authenticated;
                return Ok(Some(pid));
            }
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Аккаунт не найден").1);
        }
        AuAuthType::NoAuth => {
            send_u_packet(tx, "OK", &ok_message("Ошибка", "Анонимный вход отключен").1);
        }
    }
    
    Ok(None)
}
