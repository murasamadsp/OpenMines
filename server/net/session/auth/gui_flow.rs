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
    // C# ref: `if (text.StartsWith("exit")) { reset to default window }`
    if button.starts_with("exit") {
        *step = GuiAuthStep::MainMenu;
        send_default_auth_window(tx);
        return Ok(None);
    }

    match step {
        GuiAuthStep::MainMenu => handle_main_menu(state, tx, button, step),
        GuiAuthStep::LoginPassword { nick } => {
            let nick = nick.clone();
            handle_login_password(state, tx, button, &nick, step)
        }
        GuiAuthStep::RegisterNick => handle_register_nick(state, tx, button, step),
        GuiAuthStep::RegisterPassword { nick } => {
            let nick = nick.clone();
            handle_register_password(state, tx, button, &nick, step)
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
fn handle_main_menu(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
    step: &mut GuiAuthStep,
) -> Result<Option<PlayerId>> {
    let _ = state;
    if button == "newakk" {
        // C# ref: `CreateNew()` — open nick prompt for new account.
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

    // `nick:SomeName` — client substitutes %I% with input text.
    if let Some(name) = button.strip_prefix("nick:") {
        return handle_find_by_nick(state, tx, name.trim(), step);
    }

    // Unknown button in main menu — ignore.
    Ok(None)
}

/// C# ref: `TryToFindByNick` — look up player by name.
fn handle_find_by_nick(
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

    let player = state.db.get_player_by_name(name)?;
    if let Some(player) = player {
        // Player found — ask for password.
        *step = GuiAuthStep::LoginPassword {
            nick: player.name.clone(),
        };
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
        // C# ref: `SendU(new OKPacket("auth", "Игрок не найден"))`
        send_u_packet(tx, "OK", &ok_message("auth", "Игрок не найден").1);
        send_default_auth_window(tx);
    }
    Ok(None)
}

/// Handle password input for existing player login.
fn handle_login_password(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
    nick: &str,
    step: &mut GuiAuthStep,
) -> Result<Option<PlayerId>> {
    let passwd = button.strip_prefix("passwd:").unwrap_or(button);

    let player = state.db.get_player_by_name(nick)?;
    let Some(player) = player else {
        send_u_packet(tx, "OK", &ok_message("auth", "Игрок не найден").1);
        *step = GuiAuthStep::MainMenu;
        send_default_auth_window(tx);
        return Ok(None);
    };

    if player.passwd == passwd {
        // C# ref: `TryToAuthByPlayer` success path.
        return finalize_auth(state, tx, &player, step);
    }

    // Wrong password — C# ref: `SendU(new OKPacket("auth", "Не верный пароль"))` + resend window.
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
fn handle_register_nick(
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

    // C# ref: check if name is taken.
    if state.db.player_name_exists(nick)? {
        send_u_packet(tx, "OK", &ok_message("auth", "Ник занят").1);
        // Re-show nick input (C# ref: `CreateNew()` again).
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

    // Nick available — ask for password.
    // C# ref: `SetPasswdForNew(args.Input!)`
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
fn handle_register_password(
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

    // C# ref: `EndCreateAndInit` — create player in DB.
    let hash = GameState::generate_hash();
    let player = state.db.create_player(nick, passwd, &hash)?;
    tracing::info!(
        "[Auth GUI] New player registered: {} (id={})",
        player.name,
        player.id
    );

    finalize_auth(state, tx, &player, step)
}

/// Shared finalization: send AH, cf, Gu, init_player.
/// C# ref: `SendU(new AHPacket(temp.id, temp.hash))` then `player.Init()`.
fn finalize_auth(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    player: &crate::db::players::PlayerRow,
    step: &mut GuiAuthStep,
) -> Result<Option<PlayerId>> {
    // 1. AH packet — client stores hash for future reconnection.
    let ah = auth_hash(player.id, &player.hash);
    send_u_packet(tx, ah.0, &ah.1);

    // 2. WorldInfo (cf) — must come before Init so client registers handlers.
    let w = state.world.cells_width();
    let h = state.world.cells_height();
    let world = world_info(state.world.name(), w, h, 0, "COCK", "http://pi.door/", "ok");
    send_u_packet(tx, world.0, &world.1);

    // 3. Close auth window.
    let gu = gu_close();
    send_u_packet(tx, gu.0, &gu.1);

    // 4. Player.Init() — full init sequence.
    let pid = init_player(state, tx, player);

    *step = GuiAuthStep::MainMenu; // Reset (won't be used, session transitions to Authenticated).
    Ok(Some(pid))
}
