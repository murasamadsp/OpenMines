//! Server delivery adapter for protocol-owned HORB documents.
//!
//! The wire model and serialization live in `openmines_protocol::gui`. This
//! module remains only while direct GUI call sites are migrated to immutable
//! `GameEvent::GuiView` delivery.

use std::sync::Arc;

use crate::game::{GameState, PlayerId};
use crate::net::session::outbox::Outbox;
use crate::net::session::wire::send_u_packet;

pub use openmines_macros::gui;
pub use openmines_protocol::gui::{Button, Horb, ListRow, RichRow, Tab};

/// Temporary server-side delivery bridge for legacy direct GUI call sites.
///
/// New flows must emit `GameEvent::GuiView`; presentation owns session guarding
/// and delivery. This trait preserves existing behavior during the migration,
/// without duplicating the GUI document or its wire encoder in the server.
pub trait HorbDelivery {
    fn send_raw(&self, tx: &Outbox);

    fn send(
        &self,
        state: &Arc<GameState>,
        tx: &Outbox,
        pid: PlayerId,
        window_tag: impl Into<String>,
    );
}

impl HorbDelivery for Horb {
    fn send_raw(&self, tx: &Outbox) {
        send_u_packet(tx, "GU", &self.payload());
    }

    fn send(
        &self,
        state: &Arc<GameState>,
        tx: &Outbox,
        pid: PlayerId,
        window_tag: impl Into<String>,
    ) {
        self.send_raw(tx);
        let window_tag = window_tag.into();
        state.modify_player(pid, |ecs, entity| {
            if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                ui.current_window = Some(window_tag);
            }
            Some(())
        });
    }
}
