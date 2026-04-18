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
    println!("[Auth] Attempting auth for uniq={}", au.client_uniq());
    
    let result = match &au.auth_type {
        AuAuthType::Regular { user_id, token } => {
            println!("[Auth] Regular auth: id={}, token={}", user_id, token);
            if let Ok(Some(player)) = state.db.get_player_by_id(*user_id) {
                let expected = GameState::auth_token_hash(&player.hash, sid);
                println!("[Auth] DB hash for id={}: '{}'", user_id, player.hash);
                if GameState::token_matches(token, &expected) {
                    Some(player)
                } else {
                    println!("[Auth] Token mismatch for id={}. Expected: {}", user_id, expected);
                    None
                }
            } else {
                println!("[Auth] Player not found: id={}", user_id);
                None
            }
        }
        AuAuthType::ServerSide => {
            println!("[Auth] ServerSide auth for name={}", au.client_uniq());
            state.db.get_player_by_name(au.client_uniq()).ok().flatten()
        }
        AuAuthType::NoAuth => {
            println!("[Auth] NoAuth attempt denied");
            None
        }
    };

    if let Some(player) = result {
        println!("[Auth] Success! Player={} (id={})", player.name, player.id);
        
        let w = state.world.cells_width();
        let h = state.world.cells_height();
        let name = GameState::map_profile_name(au.client_uniq());
        let world = world_info(&name, w, h, 0, "M3R", "http://localhost/", "ok");
        send_u_packet(tx, world.0, &world.1);
        
        let pid = init_player(state, tx, &player);
        *auth_state = crate::net::session::connection::AuthState::Authenticated;
        return Ok(Some(pid));
    }

    // Если не авторизован, пробуем помочь клиенту (пакет AH)
    // ВНИМАНИЕ: Временно хардкодим id=2 для отладки, так как мы не знаем ник из NO_AUTH пакета.
    if let Ok(Some(player)) = state.db.get_player_by_id(2) {
        println!("[Auth] DEBUG: Sending AH repair packet for id=2 ({})", player.name);
        let ah_payload = format!("{}_{}", player.id, player.hash);
        send_u_packet(tx, "AH", ah_payload.as_bytes());
    }

    println!("[Auth] Sending failure message to client");
    send_u_packet(tx, "OK", &ok_message("Ошибка", "Авторизация не удалась").1);
    Ok(None)
}
