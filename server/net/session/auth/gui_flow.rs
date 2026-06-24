//! Обработка GUI-диалогов авторизации (вход, регистрация).
//! 1:1 с C# `Auth.CallAction` / `Auth.TryToFindByNick` / `Auth.EndCreateAndInit`.
use crate::game::GameState;
use crate::net::session::connection::GuiAuthStep;
use crate::net::session::player::init::init_player;
use crate::net::session::prelude::*;
use crate::net::session::ui::horb::{Button, Horb};
use crate::protocol::packets::auth_hash;

fn hash_password(passwd: &str, user_hash: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(user_hash.as_bytes());
    h.update(b":");
    h.update(passwd.as_bytes());
    format!("sha256:{:x}", h.finalize())
}

fn verify_password(passwd: &str, stored: &str, user_hash: &str) -> bool {
    stored.strip_prefix("sha256:").map_or_else(
        || stored == passwd,
        |hex| {
            hash_password(passwd, user_hash)
                .strip_prefix("sha256:")
                .unwrap_or("")
                == hex
        },
    )
}

/// Called from connection loop when `AuthState::GuiAuth` and a GUI_ button arrives.
/// Returns `Some(pid)` on successful login/registration (session transitions to Authenticated).
pub async fn handle_gui_auth_flow(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
    session_token: u64,
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
            handle_login_password(state, tx, button, &nick, session_token, step).await
        }
        GuiAuthStep::RegisterNick => handle_register_nick(state, tx, button, step).await,
        GuiAuthStep::RegisterPassword { nick } => {
            let nick = nick.clone();
            handle_register_password(state, tx, button, &nick, session_token, step).await
        }
    }
}

/// C# ref: `def` window — main auth menu with "Новый акк" and "ok" (nick input).
pub fn send_default_auth_window(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    Horb::new("ВХОД")
        .text("Авторизация")
        .input(" ", true)
        .button(Button::new("Новый акк", "newakk"))
        .button(Button::new("ok", "nick:%I%"))
        .close_button()
        .send_raw(tx);
}

fn send_auth_input_window(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    title: &str,
    text: &str,
    action: &str,
) {
    Horb::new(title)
        .text(text)
        .input(" ", true)
        .button(Button::new("OK", action))
        .send_raw(tx);
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
        send_auth_input_window(tx, "НОВЫЙ ИГРОК", "Ник", "newnick:%I%");
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
        send_auth_input_window(tx, "ВХОД", "Пароль", "passwd:%I%");
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
    session_token: u64,
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

    if verify_password(passwd, &player.passwd, &player.hash) {
        if !player.passwd.starts_with("sha256:") {
            let hashed = hash_password(passwd, &player.hash);
            let _ = state.db.update_player_passwd(player.id, &hashed).await;
        }
        return finalize_auth(state, tx, &player, session_token, step);
    }

    send_u_packet(tx, "OK", &ok_message("auth", "Не верный пароль").1);
    send_auth_input_window(
        tx,
        "ВХОД",
        "Пароль\nВведён не верный пароль. Попробуйте ещё раз.",
        "passwd:%I%",
    );
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
        send_auth_input_window(tx, "НОВЫЙ ИГРОК", "Ник", "newnick:%I%");
        return Ok(None);
    }

    *step = GuiAuthStep::RegisterPassword {
        nick: nick.to_owned(),
    };
    send_auth_input_window(tx, "НОВЫЙ ИГРОК", "Пароль", "passwd:%I%");
    Ok(None)
}

/// Handle password input for new account — creates the player.
async fn handle_register_password(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
    nick: &str,
    session_token: u64,
    step: &mut GuiAuthStep,
) -> Result<Option<PlayerId>> {
    let passwd = button.strip_prefix("passwd:").unwrap_or(button);

    if passwd.is_empty() {
        send_u_packet(tx, "OK", &ok_message("auth", "Введите пароль").1);
        return Ok(None);
    }

    let hash = GameState::generate_hash();
    let hashed_passwd = hash_password(passwd, &hash);
    let player = state.db.create_player(nick, &hashed_passwd, &hash).await?;
    tracing::info!(
        "[Auth GUI] New player registered: {} (id={})",
        player.name,
        player.id
    );

    finalize_auth(state, tx, &player, session_token, step)
}

/// Shared finalization: send AH, cf, Gu, `init_player`.
fn finalize_auth(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    player: &crate::db::players::PlayerRow,
    session_token: u64,
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

    let pid = init_player(state, tx, player, session_token);

    *step = GuiAuthStep::MainMenu;
    Ok(Some(pid))
}
