//! Обработка GUI-диалогов авторизации (вход, регистрация).
use crate::net::session::connection::AuthState;
use crate::net::session::player::init::init_player;
use crate::net::session::prelude::*;

pub async fn handle_gui_auth_flow(
    _state: &Arc<GameState>,
    _tx: &mpsc::UnboundedSender<Vec<u8>>,
    _button: &str,
    _auth_state: &mut AuthState,
) -> Result<Option<PlayerId>> {
    // TODO: Implement GUI registration/login flow
    Ok(None)
}
