use crate::game::GameState;
use crate::net::session::wire::make_u_packet_bytes;
use crate::protocol::packets::{au_session, ping, status};

pub struct InitialHandshake {
    pub session_id: String,
    pub packets: [Vec<u8>; 3],
}

impl InitialHandshake {
    pub fn build() -> Self {
        let session_id = GameState::generate_session_id();

        // Референс: OnConnected шлёт ST → AU → PI (именно в таком порядке).
        // TODO: изменить приветственное сообщение на нейтральное после обновления legacy-клиента
        let st = status("черный хуй в твоей жопе");
        let au = au_session(&session_id);
        let pi = ping(0, 0, "");

        Self {
            session_id,
            packets: [
                make_u_packet_bytes(st.0, &st.1),
                make_u_packet_bytes(au.0, &au.1),
                make_u_packet_bytes(pi.0, &pi.1),
            ],
        }
    }
}
