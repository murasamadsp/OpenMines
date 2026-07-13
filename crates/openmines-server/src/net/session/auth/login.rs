//! Логика авторизации игрока (пакет AU).
use crate::game::player::PlayerId;
use crate::net::session::auth::gui_flow::send_default_auth_window;
use crate::net::session::player::init::init_player;
use crate::net::session::prelude::*;

/// Неуспешная авторизация: референс `Auth.TryToAuth` — `cf` → `BI` (гость) → `HB` → `GU`.
fn send_auth_failure(state: &Arc<GameState>, tx: &Outbox, _au: &AuClientPacket<'_>) {
    // 1:1 ref: WorldInfoPacket(World.W.name, ...)
    super::send_world_info(state, tx);

    // 1:1 ref: BotInfoPacket("pidor", 0, 0, -1)
    let bi = bot_info("pidor", 0, 0, -1);
    send_u_packet(tx, bi.0, &bi.1);

    let cells = state.world.read_chunk_cells(0, 0);
    let sub = hb_map(0, 0, 32, 32, &cells);
    // 1:1 ref: SendU(new HBPacket(...)) — HB payload, но outer data_type = "U"
    let bundle = hb_bundle(&[sub]).1;
    send_b_packet(tx, "HB", &bundle);

    // 1:1 ref: `authwin = def; initiator.SendWin(authwin.ToString());`
    send_default_auth_window(tx);
}

pub async fn handle_auth(
    state: &Arc<GameState>,
    tx: &Outbox,
    au: &AuClientPacket<'_>,
    sid: &str,
    session_id: SessionId,
    auth_state: &mut crate::net::session::connection::AuthState,
) -> Result<Option<PlayerId>> {
    tracing::debug!(uniq = %au.client_uniq(), "Attempting auth");

    let result = match &au.auth_type {
        AuAuthType::Regular { user_id, token } => {
            tracing::debug!(user_id = *user_id, "Regular auth");
            if let Ok(Some(player)) = state.db.get_player_by_id(*user_id).await {
                if GameState::token_matches_legacy_auth(token, &player.hash, sid) {
                    Some(player)
                } else {
                    tracing::debug!(user_id = *user_id, "Token mismatch");
                    None
                }
            } else {
                tracing::debug!(user_id = *user_id, "Player not found");
                None
            }
        }
        AuAuthType::NoAuth => {
            tracing::debug!("NoAuth attempt denied");
            None
        }
    };

    if let Some(mut player) = result {
        tracing::info!(player_id = %player.id, username = %player.name, "Auth success");

        // M3R_GRANT_ADMIN на ЛОГИНЕ: `bootstrap_grant_admin` при старте не находит
        // игроков свежесгенерированного мира (таблица пуста до первого входа), поэтому
        // на fresh-мире 1337 иначе НИКОГДА не админ. Грантим здесь — когда игрок есть.
        if let Ok(raw) = std::env::var("M3R_GRANT_ADMIN") {
            if raw.split(',').map(str::trim).any(|n| n == player.name)
                && state
                    .db
                    .set_player_role(player.id, crate::db::Role::Admin)
                    .await
                    .unwrap_or(false)
            {
                player.role = crate::db::Role::Admin as i32;
                tracing::info!(
                    "M3R_GRANT_ADMIN: Role::Admin для {:?} на логине",
                    player.name
                );
            }
        }

        // 1. Сначала CF (world_info) — клиент в OnWorldConfig вызывает ServerController.Init(),
        //    который регистрирует ВСЕ остальные обработчики пакетов. Без CF клиент мёртв.
        // 1:1 ref: WorldInfoPacket(World.W.name, ...)
        super::send_world_info(state, tx);

        // 2. Gu (закрыть окно авторизации) — референс: SendU(new GuPacket()) перед Init()
        let gu = gu_close();
        send_u_packet(tx, gu.0, &gu.1);

        // 3. Player.Init() — в `server_reference/Auth.TryToAuth` при токене `AH` не шлётся (только после GUI-пароля / регистрации).
        let pid = init_player(state, &player, session_id).await;
        *auth_state = crate::net::session::connection::AuthState::Authenticated { player_id: pid };

        return Ok(Some(pid));
    }

    tracing::debug!("Sending auth-failure sequence (cf+BI+HB+GU)");
    send_auth_failure(state, tx, au);
    // Transition to GUI auth state — client now sees the auth window and can interact via GUI_ buttons.
    *auth_state = crate::net::session::connection::AuthState::GuiAuth(
        crate::net::session::connection::GuiAuthStep::MainMenu,
    );
    Ok(None)
}
