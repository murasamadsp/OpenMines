//! Логика взаимодействия с объектами (паками) на карте.
use crate::game::player::{PlayerPosition, PlayerStats};
use crate::net::session::prelude::*;

// TODO: will be used when pack interaction is fully wired to session dispatch
#[allow(dead_code)]
pub fn handle_pack_action(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    x: i32,
    y: i32,
) {
    let Some(pack) = state.get_pack_at(x, y).map(|p| p.clone()) else {
        return;
    };

    let p_info = state
        .query_player(pid, |ecs, entity| {
            let pos = ecs.get::<PlayerPosition>(entity)?;
            let stats = ecs.get::<PlayerStats>(entity)?;
            Some((pos.x, pos.y, stats.clan_id.unwrap_or(0)))
        })
        .flatten();

    let Some((px, py, p_clan)) = p_info else {
        return;
    };
    if validate_pack_access(&pack, (px, py), p_clan, pid).is_err() {
        return;
    }

    match pack.pack_type {
        PackType::Resp => {
            state.modify_player(pid, |ecs, entity| {
                let mut meta = ecs.get_mut::<crate::game::player::PlayerMetadata>(entity)?;
                meta.resp_x = Some(pack.x);
                meta.resp_y = Some(pack.y);
                Some(())
            });
            send_u_packet(
                tx,
                "OK",
                &ok_message("Респ", "Точка возрождения установлена").1,
            );
        }
        _ => crate::net::session::ui::gui_buttons::handle_gui_button(
            state,
            tx,
            pid,
            &format!("pack_op:open:{}:{}", x, y),
        ),
    }
}
