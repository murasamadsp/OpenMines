use crate::game::player::PlayerUI;
use crate::net::session::prelude::*;

pub(super) fn open(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, view: &PackView) {
    if view.owner_id != pid {
        state.modify_player(pid, |ecs, entity| {
            ecs.get_mut::<PlayerUI>(entity)?.current_window = None;
            Some(())
        });
        let close = gu_close();
        send_u_packet(tx, close.0, &close.1);
        return;
    }

    use super::horb::{Button, Horb};
    Horb::new("СПОТ")
        .button(Button::new(
            "Удалить",
            format!("pack_op:remove:{}:{}", view.x, view.y),
        ))
        .close_button()
        .send(state, tx, pid, format!("pack:{}:{}", view.x, view.y));
}
