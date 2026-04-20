//! Логика авторизации игрока (пакет AU).
use crate::game::player::PlayerId;
use crate::net::session::player::init::init_player;
use crate::net::session::prelude::*;
use serde_json::json;

/// Неуспешная авторизация: референс `Auth.TryToAuth` — `cf` → `BI` (гость) → `HB` → `GU`.
fn send_auth_failure(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, au: &AuClientPacket) {
    let w = state.world.cells_width();
    let h = state.world.cells_height();
    // 1:1 ref: WorldInfoPacket(World.W.name, ...)
    // ref: WorldInfoPacket(..., 0, "COCK", "http://pi.door/", "ok")
    let world = world_info(state.world.name(), w, h, 0, "COCK", "http://pi.door/", "ok");
    send_u_packet(tx, world.0, &world.1);

    // 1:1 ref: BotInfoPacket("pidor", 0, 0, -1)
    let bi = bot_info("pidor", 0, 0, -1);
    send_u_packet(tx, bi.0, &bi.1);

    let cells = state.world.read_chunk_cells(0, 0);
    let sub = hb_map(0, 0, 32, 32, &cells);
    // 1:1 ref: SendU(new HBPacket(...)) — HB payload, но outer data_type = "U"
    let bundle = hb_bundle(&[sub]).1;
    send_u_packet(tx, "HB", &bundle);

    // 1:1 ref: `authwin = def; initiator.SendWin(authwin.ToString());`
    // Window.ToString() builds `horb:{...}` with `buttons` as alternating label/action entries.
    let gui = json!({
        "title": "ВХОД",
        "buttons": [
            "Новый акк", "newakk",
            "ok", "nick:%I%",
            "ВЫЙТИ", "exit"
        ],
        "back": false,
        "text": "Авторизация",
        "input_place": " ",
        "input_console": true
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

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
                println!("[Auth] DB hash for id={}: '{}'", user_id, player.hash);
                if GameState::token_matches_legacy_auth(token, &player.hash, sid) {
                    Some(player)
                } else {
                    println!(
                        "[Auth] Token mismatch for id={}. md5={} sha256={}",
                        user_id,
                        GameState::auth_token_hash_md5(&player.hash, sid),
                        GameState::auth_token_hash_sha256(&player.hash, sid)
                    );
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

        // 1. Сначала CF (world_info) — клиент в OnWorldConfig вызывает ServerController.Init(),
        //    который регистрирует ВСЕ остальные обработчики пакетов. Без CF клиент мёртв.
        let w = state.world.cells_width();
        let h = state.world.cells_height();
        // 1:1 ref: WorldInfoPacket(World.W.name, ...)
        // ref: WorldInfoPacket(..., 0, "COCK", "http://pi.door/", "ok")
        let world = world_info(state.world.name(), w, h, 0, "COCK", "http://pi.door/", "ok");
        send_u_packet(tx, world.0, &world.1);

        // 2. Gu (закрыть окно авторизации) — референс: SendU(new GuPacket()) перед Init()
        let gu = gu_close();
        send_u_packet(tx, gu.0, &gu.1);

        // 3. Player.Init() — в `server_reference/Auth.TryToAuth` при токене `AH` не шлётся (только после GUI-пароля / регистрации).
        let pid = init_player(state, tx, &player);
        *auth_state = crate::net::session::connection::AuthState::Authenticated;

        return Ok(Some(pid));
    }

    println!("[Auth] Sending auth-failure sequence (cf+BI+HB+GU)");
    send_auth_failure(state, tx, au);
    Ok(None)
}
