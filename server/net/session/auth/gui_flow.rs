//! Регистрация и вход через формы GU до появления в мире.
use crate::net::session::auth::login::AuthState;
use crate::net::session::player::init::init_player;
use crate::net::session::prelude::*;

// ─── Auth GUI (no player_id yet) ────────────────────────────────────────────

pub fn handle_auth_gui_registration(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
) -> bool {
    handle_auth_gui_registration_with_title(state, tx, button, "Ошибка")
}

pub fn handle_auth_gui_registration_with_title(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
    error_title: &str,
) -> bool {
    if button == "newakk" {
        send_auth_gui(
            tx,
            "Регистрация",
            "Шаг 1 из 2: придумайте ник (его видят другие игроки).",
            true,
            &[("Далее", "newnick:%I%")],
            "Ник",
        );
        return true;
    }

    if let Some(nick) = button.strip_prefix("newnick:") {
        let nick = nick.trim();
        if nick.is_empty() {
            send_auth_error(tx, error_title, "Введите ник в поле и нажмите «Далее»");
            return true;
        }
        if state.db.player_name_exists(nick).unwrap_or(false) {
            send_auth_error(tx, error_title, "Такой ник уже занят");
            return true;
        }
        let action = format!("newpasswd:{nick}:%I%");
        send_auth_gui(
            tx,
            "Регистрация",
            "Шаг 2 из 2: придумайте пароль.",
            true,
            &[("Создать аккаунт", action.as_str())],
            "Пароль",
        );
        return true;
    }

    false
}

pub fn handle_auth_gui_finish(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
    auth_state: &mut AuthState,
) -> ControlFlow<Option<PlayerId>, ()> {
    handle_auth_gui_finish_with_title_internal(state, tx, button, auth_state, "Ошибка")
}

pub fn handle_auth_gui_finish_with_title(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
    auth_state: &mut AuthState,
    error_title: &str,
) -> ControlFlow<Option<PlayerId>, ()> {
    handle_auth_gui_finish_with_title_internal(state, tx, button, auth_state, error_title)
}

pub fn handle_auth_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
    _sid: &str,
    auth_state: &mut AuthState,
) -> Option<PlayerId> {
    if handle_auth_gui_registration(state, tx, button) {
        return None;
    }
    match handle_auth_gui_finish(state, tx, button, auth_state) {
        ControlFlow::Break(v) => v,
        ControlFlow::Continue(()) => {
            tracing::debug!(
                "Unknown auth GUI button during auth flow (len={} chars)",
                button.len()
            );
            None
        }
    }
}

pub fn send_initial_auth_screen(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_auth_gui(
        tx,
        "Вход в игру",
        "Введите ник и нажмите «Войти». Первый раз здесь — «Регистрация».",
        true,
        &[("Войти", "nick:%I%"), ("Регистрация", "newakk")],
        "Ник",
    );
}

fn handle_auth_gui_finish_with_title_internal(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    button: &str,
    auth_state: &mut AuthState,
    error_title: &str,
) -> ControlFlow<Option<PlayerId>, ()> {
    if let Some(rest) = button.strip_prefix("newpasswd:") {
        if let Some((nick, passwd)) = rest.split_once(':') {
            let nick = nick.trim();
            let passwd = passwd.trim();
            if nick.is_empty() || passwd.is_empty() {
                send_auth_error(tx, error_title, "Имя и пароль не должны быть пустыми");
                return ControlFlow::Break(None);
            }
            let hash = GameState::generate_hash();
            let hashed_passwd = GameState::encode_password_hash(passwd, &hash);
            match state.db.create_player(nick, &hashed_passwd, &hash) {
                Ok(player) => {
                    send_u_packet(tx, "AH", &auth_hash(player.id, &player.hash).1);
                    let pid = init_player(state, tx, &player);
                    *auth_state = AuthState::Authenticated;
                    return ControlFlow::Break(Some(pid));
                }
                Err(e) => {
                    tracing::error!("Failed to create player: {e}");
                    send_auth_error(tx, error_title, "Ошибка создания");
                }
            }
        }
        return ControlFlow::Break(None);
    }

    if let Some(nick) = button.strip_prefix("nick:") {
        let nick = nick.trim();
        if nick.is_empty() {
            send_auth_error(
                tx,
                error_title,
                "Введите ник в поле над кнопками и нажмите «Войти»",
            );
            return ControlFlow::Break(None);
        }
        match state.db.get_player_by_name(nick) {
            Ok(Some(player)) => {
                let action = format!("passwd:{}:%I%", player.id);
                send_auth_gui(
                    tx,
                    "Пароль",
                    "Введите пароль для этого аккаунта.",
                    true,
                    &[("Войти", action.as_str())],
                    "Пароль",
                );
            }
            _ => {
                send_auth_error(
                    tx,
                    error_title,
                    "Такого ника нет. Проверьте написание (регистр не важен) или нажмите «Регистрация».",
                );
            }
        }
        return ControlFlow::Break(None);
    }

    if let Some(rest) = button.strip_prefix("passwd:") {
        if let Some((id_str, passwd)) = rest.split_once(':') {
            let id_str = id_str.trim();
            if let Ok(id) = id_str.parse::<i32>() {
                match state.db.get_player_by_id(id) {
                    Ok(Some(player)) => {
                        let passwd = passwd.trim();
                        if GameState::verify_password(passwd, &player.passwd, &player.hash) {
                            let mut authenticated_player = player.clone();
                            if !player.passwd.starts_with("p$") {
                                authenticated_player.passwd =
                                    GameState::encode_password_hash(passwd, &player.hash);
                                if let Err(err) = state.db.save_player(&authenticated_player) {
                                    tracing::warn!(
                                        "Failed to migrate legacy password hash for player {}: {err}",
                                        player.id
                                    );
                                }
                            }
                            send_u_packet(tx, "AH", &auth_hash(player.id, &player.hash).1);
                            let pid = init_player(state, tx, &authenticated_player);
                            *auth_state = AuthState::Authenticated;
                            return ControlFlow::Break(Some(pid));
                        }
                        send_u_packet(tx, "OK", &ok_message("Ошибка", "Неверный пароль").1);
                    }
                    _ => {
                        send_auth_error(tx, error_title, "Неверный пароль");
                    }
                }
            }
        }
        return ControlFlow::Break(None);
    }

    ControlFlow::Continue(())
}

fn send_auth_gui(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    title: &str,
    prompt: &str,
    has_input: bool,
    buttons: &[(&str, &str)],
    input_placeholder: &str,
) {
    let button_payload = buttons
        .iter()
        .map(|(label, action)| serde_json::json!({ "t": label, "a": action }))
        .collect::<Vec<_>>();
    let mut panel = serde_json::json!({
        "tx": prompt,
        "b": button_payload
    });
    if has_input {
        let ph = if input_placeholder.trim().is_empty() {
            " "
        } else {
            input_placeholder
        };
        panel["in"] = serde_json::json!({ "ic": true, "ph": ph });
    }
    let gui = serde_json::json!({
        "t": title,
        "st": false,
        "tb": [{
            "a": "auth",
            "l": "Ник",
            "p": panel
        }]
    });
    send_u_packet(tx, "GU", gui.to_string().as_bytes());
}

fn send_auth_error(tx: &mpsc::UnboundedSender<Vec<u8>>, title: &str, text: &str) {
    send_u_packet(tx, "OK", &ok_message(title, text).1);
}
