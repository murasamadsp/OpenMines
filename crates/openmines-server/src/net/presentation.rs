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
    let mut pending = None;
    loop {
        let event = match pending.take() {
            Some(event) => event,
            None => match rx.recv().await {
                Some(event) => event,
                None => break,
            },
        };
        if let GameEvent::MovementFanout {
            player_id,
            recipients,
            data,
        } = event
        {
            let (fanouts, barrier) =
                coalesce_movement_burst((player_id, recipients, data), &mut rx);
            pending = barrier;
            for (recipients, data) in fanouts {
                state.sessions.fanout(&recipients, &data);
                crate::metrics::PRESENTATION_EVENTS_TOTAL
                    .with_label_values(&["movement_fanout", "coalesced_delivered"])
                    .inc();
            }
            continue;
        }
        let kind = event.kind();
        crate::metrics::PRESENTATION_QUEUE_DEPTH.set(i64::try_from(rx.len()).unwrap_or(i64::MAX));
        deliver(&state, event);
        crate::metrics::PRESENTATION_EVENTS_TOTAL
            .with_label_values(&[kind, "delivered"])
            .inc();
    }
    crate::metrics::PRESENTATION_QUEUE_DEPTH.set(0);
}

type MovementFanout = (crate::game::PlayerId, Vec<crate::game::SessionId>, Vec<u8>);
type CoalescedMovementFanouts = Vec<(Vec<crate::game::SessionId>, Vec<u8>)>;

/// Collapses only an uninterrupted movement burst. A non-movement event stays a
/// delivery barrier, while the final packet order follows the last update seen
/// for each player instead of their numeric id.
fn coalesce_movement_burst(
    first: MovementFanout,
    rx: &mut tokio::sync::mpsc::Receiver<GameEvent>,
) -> (CoalescedMovementFanouts, Option<GameEvent>) {
    let mut latest = std::collections::BTreeMap::new();
    latest.insert(first.0, (0_usize, first.1, first.2));
    let mut sequence = 1;
    let mut barrier = None;

    while let Ok(event) = rx.try_recv() {
        match event {
            GameEvent::MovementFanout {
                player_id,
                recipients,
                data,
            } => {
                latest.insert(player_id, (sequence, recipients, data));
                sequence += 1;
            }
            event => {
                barrier = Some(event);
                break;
            }
        }
    }

    let mut ordered = std::collections::BTreeMap::new();
    for (_, (sequence, recipients, data)) in latest {
        ordered.insert(sequence, (recipients, data));
    }
    (ordered.into_values().collect(), barrier)
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
        GameEvent::Fanout { recipients, data }
        | GameEvent::MovementFanout {
            recipients, data, ..
        } => {
            state.sessions.fanout(&recipients, &data);
        }
        GameEvent::ChatFanout { route, message } => {
            crate::net::session::social::chat::deliver_chat_fanout(state, &route, &message);
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
        GameEvent::Fanout { recipients, .. } | GameEvent::MovementFanout { recipients, .. } => {
            for session_id in recipients {
                state.sessions.kick_session(session_id);
            }
        }
        GameEvent::ChatFanout { .. } => {
            // Cannot reliably determine targets that caused failure
        }
    }
}

fn update_depth(tx: &tokio::sync::mpsc::Sender<GameEvent>) {
    let depth = tx.max_capacity().saturating_sub(tx.capacity());
    crate::metrics::PRESENTATION_QUEUE_DEPTH.set(i64::try_from(depth).unwrap_or(i64::MAX));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{PlayerId, SessionId};

    fn movement(player_id: i32, byte: u8) -> GameEvent {
        GameEvent::MovementFanout {
            player_id: PlayerId(player_id),
            recipients: vec![SessionId::new(u64::try_from(player_id).unwrap())],
            data: vec![byte],
        }
    }

    #[tokio::test]
    async fn movement_burst_keeps_latest_packet_in_last_update_order() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        tx.send(movement(2, 20)).await.unwrap();
        tx.send(movement(1, 10)).await.unwrap();
        tx.send(movement(2, 21)).await.unwrap();

        let GameEvent::MovementFanout {
            player_id,
            recipients,
            data,
        } = rx.recv().await.unwrap()
        else {
            panic!("expected movement fanout");
        };
        let (fanouts, barrier) = coalesce_movement_burst((player_id, recipients, data), &mut rx);

        assert_eq!(
            fanouts,
            vec![
                (vec![SessionId::new(1)], vec![10]),
                (vec![SessionId::new(2)], vec![21])
            ]
        );
        assert!(barrier.is_none());
    }

    #[tokio::test]
    async fn movement_burst_does_not_cross_a_delivery_barrier() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        tx.send(movement(1, 10)).await.unwrap();
        tx.send(GameEvent::Fanout {
            recipients: vec![SessionId::new(7)],
            data: vec![70],
        })
        .await
        .unwrap();
        tx.send(movement(1, 11)).await.unwrap();

        let GameEvent::MovementFanout {
            player_id,
            recipients,
            data,
        } = rx.recv().await.unwrap()
        else {
            panic!("expected movement fanout");
        };
        let (fanouts, barrier) = coalesce_movement_burst((player_id, recipients, data), &mut rx);

        assert_eq!(fanouts, vec![(vec![SessionId::new(1)], vec![10])]);
        assert!(matches!(
            barrier,
            Some(GameEvent::Fanout { recipients, data })
                if recipients == vec![SessionId::new(7)] && data == vec![70]
        ));
        assert!(
            matches!(rx.recv().await, Some(GameEvent::MovementFanout { data, .. }) if data == vec![11])
        );
    }
}
