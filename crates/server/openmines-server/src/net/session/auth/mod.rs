//! Авторизация: логин по AU и GUI до входа в мир.

pub mod gui_flow;
pub mod login;

pub(super) fn send_world_info(
    state: &std::sync::Arc<crate::game::GameState>,
    outbox: &crate::net::session::outbox::Outbox,
) {
    use crate::world::WorldProvider as _;

    let packet = crate::protocol::packets::world_info(
        state.world.name(),
        state.world.cells_width(),
        state.world.cells_height(),
        0,
        "COCK",
        "http://pi.door/",
        "ok",
    );
    crate::net::session::wire::send_u_packet(outbox, packet.0, &packet.1);
}
