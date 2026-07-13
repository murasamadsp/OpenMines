use crate::game::{GameEvent, GameState};
use std::sync::Arc;

const QUEUE_CAPACITY: usize = 4_096;

pub struct PresentationRuntime {
    tx: tokio::sync::mpsc::Sender<GameEvent>,
    state: Arc<GameState>,
}

impl PresentationRuntime {
    pub fn start(state: Arc<GameState>) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(QUEUE_CAPACITY);
        state.tokio_handle.spawn(run_delivery(state.clone(), rx));
        Self { tx, state }
    }

    pub fn publish(&self, event: GameEvent) {
        let kind = event.kind();
        match self.tx.try_send(event) {
            Ok(()) => {
                update_depth(&self.tx);
                crate::metrics::PRESENTATION_EVENTS_TOTAL
                    .with_label_values(&[kind, "queued"])
                    .inc();
            }
            Err(tokio::sync::mpsc::error::TrySendError::Full(event)) => {
                crate::metrics::PRESENTATION_EVENTS_TOTAL
                    .with_label_values(&[kind, "saturated"])
                    .inc();
                disconnect_targets(&self.state, event);
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(event)) => {
                crate::metrics::PRESENTATION_EVENTS_TOTAL
                    .with_label_values(&[kind, "worker_closed"])
                    .inc();
                disconnect_targets(&self.state, event);
            }
        }
    }
}

async fn run_delivery(state: Arc<GameState>, mut rx: tokio::sync::mpsc::Receiver<GameEvent>) {
    while let Some(event) = rx.recv().await {
        let kind = event.kind();
        crate::metrics::PRESENTATION_QUEUE_DEPTH.set(i64::try_from(rx.len()).unwrap_or(i64::MAX));
        deliver(&state, event);
        crate::metrics::PRESENTATION_EVENTS_TOTAL
            .with_label_values(&[kind, "delivered"])
            .inc();
    }
    crate::metrics::PRESENTATION_QUEUE_DEPTH.set(0);
}

fn deliver(state: &Arc<GameState>, event: GameEvent) {
    match event {
        GameEvent::PlayerInit { session_id, view } => {
            crate::net::session::player::init::deliver_player_init(state, session_id, &view);
        }
        GameEvent::SessionBatch {
            session_id,
            player_id,
            packets,
        } => crate::net::session::player::init::deliver_initial_presentation(
            state, session_id, player_id, packets,
        ),
        GameEvent::Fanout { recipients, data } => {
            state.sessions.fanout(&recipients, &data);
        }
        GameEvent::GuiView {
            session_id,
            player_id,
            view,
        } => deliver_gui_view(state, session_id, player_id, view),
    }
}

fn deliver_gui_view(
    state: &GameState,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    view: crate::game::GuiView,
) {
    if state.sessions.session_for_player(player_id) != Some(session_id) {
        return;
    }
    let Some(tx) = state.sessions.outbox_for_session(session_id) else {
        return;
    };
    match view {
        crate::game::GuiView::Close => {
            let packet = crate::protocol::packets::gu_close();
            crate::net::session::wire::send_u_packet(&tx, packet.0, &packet.1);
        }
        crate::game::GuiView::Teleport(view) => {
            let payload = crate::net::session::ui::teleport::render(&view);
            crate::net::session::wire::send_u_packet(&tx, "GU", &payload);
        }
    }
}

#[cfg(test)]
pub fn deliver_gui_view_for_test(
    state: &GameState,
    session_id: crate::game::SessionId,
    player_id: crate::game::PlayerId,
    view: crate::game::GuiView,
) {
    deliver_gui_view(state, session_id, player_id, view);
}

fn disconnect_targets(state: &GameState, event: GameEvent) {
    match event {
        GameEvent::PlayerInit { session_id, .. }
        | GameEvent::SessionBatch { session_id, .. }
        | GameEvent::GuiView { session_id, .. } => {
            state.sessions.kick_session(session_id);
        }
        GameEvent::Fanout { recipients, .. } => {
            for session_id in recipients {
                state.sessions.kick_session(session_id);
            }
        }
    }
}

fn update_depth(tx: &tokio::sync::mpsc::Sender<GameEvent>) {
    let depth = tx.max_capacity().saturating_sub(tx.capacity());
    crate::metrics::PRESENTATION_QUEUE_DEPTH.set(i64::try_from(depth).unwrap_or(i64::MAX));
}
