//! Обработка GUI-диалогов авторизации (вход, регистрация).
//! 1:1 с C# `Auth.CallAction` / `Auth.TryToFindByNick` / `Auth.EndCreateAndInit`.
use crate::game::GameState;
use crate::net::session::connection::GuiAuthStep;
use crate::net::session::player::init::init_player;
use crate::net::session::prelude::*;
use crate::protocol::packets::auth_hash;
use serde_json::json;

/// Called from connection loop when `AuthState::GuiAuth` and a GUI_ button arrives.
/// Returns `Some(pid)` on successful login/registration (session transitions to Authenticated).
pub async fn handle_gui_auth_flow(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
    step: &mut GuiAuthStep,
) -> Result<Option<PlayerId>> {
    if button.starts_with("exit") {
        *step = GuiAuthStep::MainMenu;
        send_default_auth_window(tx);
        return Ok(None);
    }

    match step {
        GuiAuthStep::MainMenu => handle_main_menu(state, tx, button, step).await,
        GuiAuthStep::LoginPassword { nick } => {
            let nick = nick.clone();
            handle_login_password(state, tx, button, &nick, step).await
        }
        GuiAuthStep::RegisterNick => handle_register_nick(state, tx, button, step).await,
        GuiAuthStep::RegisterPassword { nick } => {
            let nick = nick.clone();
            handle_register_password(state, tx, button, &nick, step).await
        }
    }
}

/// C# ref: `def` window — main auth menu with "Новый акк" and "ok" (nick input).
fn send_default_auth_window(tx: &mpsc::UnboundedSender<Vec<u8>>) {
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

/// Handle buttons on the main auth menu.
async fn handle_main_menu(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
    step: &mut GuiAuthStep,
) -> Result<Option<PlayerId>> {
    let _ = state;
    if button == "newakk" {
        *step = GuiAuthStep::RegisterNick;
        let gui = json!({
            "title": "НОВЫЙ ИГРОК",
            "text": "Ник",
            "buttons": ["OK", "newnick:%I%"],
            "back": false,
            "input_place": " ",
            "input_console": true
        });
        send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
        return Ok(None);
    }

    if let Some(name) = button.strip_prefix("nick:") {
        return handle_find_by_nick(state, tx, name.trim(), step).await;
    }

    Ok(None)
}

/// C# ref: `TryToFindByNick` — look up player by name.
async fn handle_find_by_nick(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    name: &str,
    step: &mut GuiAuthStep,
) -> Result<Option<PlayerId>> {
    if name.is_empty() {
        send_u_packet(tx, "OK", &ok_message("auth", "Введите ник").1);
        send_default_auth_window(tx);
        return Ok(None);
    }

    let player = state.db.get_player_by_name(name).await?;
    if let Some(player) = player {
        *step = GuiAuthStep::LoginPassword { nick: player.name };
        let gui = json!({
            "title": "ВХОД",
            "text": "Пароль",
            "buttons": ["OK", "passwd:%I%"],
            "back": false,
            "input_place": " ",
            "input_console": true
        });
        send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
    } else {
        send_u_packet(tx, "OK", &ok_message("auth", "Игрок не найден").1);
        send_default_auth_window(tx);
    }
    Ok(None)
}

/// Handle password input for existing player login.
async fn handle_login_password(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
    nick: &str,
    step: &mut GuiAuthStep,
) -> Result<Option<PlayerId>> {
    let passwd = button.strip_prefix("passwd:").unwrap_or(button);

    let player = state.db.get_player_by_name(nick).await?;
    let Some(player) = player else {
        send_u_packet(tx, "OK", &ok_message("auth", "Игрок не найден").1);
        *step = GuiAuthStep::MainMenu;
        send_default_auth_window(tx);
        return Ok(None);
    };

    if player.passwd == passwd {
        return finalize_auth(state, tx, &player, step).await;
    }

    send_u_packet(tx, "OK", &ok_message("auth", "Не верный пароль").1);
    let gui = json!({
        "title": "ВХОД",
        "text": "Пароль\nВведён не верный пароль. Попробуйте ещё раз.",
        "buttons": ["OK", "passwd:%I%"],
        "back": false,
        "input_place": " ",
        "input_console": true
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
    Ok(None)
}

/// Handle nick input for new account registration.
async fn handle_register_nick(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
    step: &mut GuiAuthStep,
) -> Result<Option<PlayerId>> {
    let nick = button.strip_prefix("newnick:").unwrap_or(button).trim();

    if nick.is_empty() {
        send_u_packet(tx, "OK", &ok_message("auth", "Введите ник").1);
        return Ok(None);
    }

    if state.db.player_name_exists(nick).await? {
        send_u_packet(tx, "OK", &ok_message("auth", "Ник занят").1);
        let gui = json!({
            "title": "НОВЫЙ ИГРОК",
            "text": "Ник",
            "buttons": ["OK", "newnick:%I%"],
            "back": false,
            "input_place": " ",
            "input_console": true
        });
        send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
        return Ok(None);
    }

    *step = GuiAuthStep::RegisterPassword {
        nick: nick.to_owned(),
    };
    let gui = json!({
        "title": "НОВЫЙ ИГРОК",
        "text": "Пароль",
        "buttons": ["OK", "passwd:%I%"],
        "back": false,
        "input_place": " ",
        "input_console": true
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
    Ok(None)
}

/// Handle password input for new account — creates the player.
async fn handle_register_password(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
    nick: &str,
    step: &mut GuiAuthStep,
) -> Result<Option<PlayerId>> {
    let passwd = button.strip_prefix("passwd:").unwrap_or(button);

    if passwd.is_empty() {
        send_u_packet(tx, "OK", &ok_message("auth", "Введите пароль").1);
        return Ok(None);
    }

    let hash = GameState::generate_hash();
    let player = state.db.create_player(nick, passwd, &hash).await?;
    tracing::info!(
        "[Auth GUI] New player registered: {} (id={})",
        player.name,
        player.id
    );

    finalize_auth(state, tx, &player, step).await
}

/// Shared finalization: send AH, cf, Gu, `init_player`.
async fn finalize_auth(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    player: &crate::db::players::PlayerRow,
    step: &mut GuiAuthStep,
) -> Result<Option<PlayerId>> {
    let ah = auth_hash(player.id, &player.hash);
    send_u_packet(tx, ah.0, &ah.1);

    let w = state.world.cells_width();
    let h = state.world.cells_height();
    let world = world_info(state.world.name(), w, h, 0, "COCK", "http://pi.door/", "ok");
    send_u_packet(tx, world.0, &world.1);

    let gu = gu_close();
    send_u_packet(tx, gu.0, &gu.1);

    let pid = init_player(state, tx, player).await;

    *step = GuiAuthStep::MainMenu;
    Ok(Some(pid))
}
