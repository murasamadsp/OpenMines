use crate::game::{GameEvent, GameState};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const QUEUE_CAPACITY: usize = 4_096;
const WORLD_EFFECT_COALESCE_LIMIT: usize = 32;

#[derive(Clone)]
pub struct PresentationSender {
    tx: std::sync::mpsc::SyncSender<GameEvent>,
    state: Arc<GameState>,
    depth: Arc<AtomicUsize>,
}

impl PresentationSender {
    pub fn publish(&self, event: GameEvent) {
        let kind = event.kind();
        self.depth.fetch_add(1, Ordering::Relaxed);
        match self.tx.try_send(event) {
            Ok(()) => {
                update_depth(&self.depth);
                crate::metrics::PRESENTATION_EVENTS_TOTAL
                    .with_label_values(&[kind, "queued"])
                    .inc();
            }
            Err(std::sync::mpsc::TrySendError::Full(event)) => {
                self.depth.fetch_sub(1, Ordering::Relaxed);
                update_depth(&self.depth);
                crate::metrics::PRESENTATION_EVENTS_TOTAL
                    .with_label_values(&[kind, "saturated"])
                    .inc();
                disconnect_targets(&self.state, event);
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(event)) => {
                self.depth.fetch_sub(1, Ordering::Relaxed);
                update_depth(&self.depth);
                crate::metrics::PRESENTATION_EVENTS_TOTAL
                    .with_label_values(&[kind, "worker_closed"])
                    .inc();
                disconnect_targets(&self.state, event);
            }
        }
    }
}

pub struct PresentationRuntime {
    sender: PresentationSender,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl PresentationRuntime {
    pub fn start(state: Arc<GameState>) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel(QUEUE_CAPACITY);
        let depth = Arc::new(AtomicUsize::new(0));
        let worker_state = state.clone();
        let worker_depth = depth.clone();
        let worker = std::thread::Builder::new()
            .name("openmines-presentation".to_owned())
            .spawn(move || run_delivery(&worker_state, &rx, &worker_depth))
            .expect("spawn presentation thread");
        Self {
            sender: PresentationSender { tx, state, depth },
            worker: Some(worker),
        }
    }

    pub fn publish(&self, event: GameEvent) {
        self.sender.publish(event);
    }

    pub fn sender(&self) -> PresentationSender {
        self.sender.clone()
    }

    pub fn shutdown(mut self) {
        drop(self.sender);
        if let Some(worker) = self.worker.take() {
            worker.join().expect("presentation thread panicked");
        }
    }
}

fn run_delivery(
    state: &Arc<GameState>,
    rx: &std::sync::mpsc::Receiver<GameEvent>,
    depth: &AtomicUsize,
) {
    let mut pending = None;
    loop {
        let event = match pending.take() {
            Some(event) => event,
            None => match rx.recv() {
                Ok(event) => event,
                Err(_) => break,
            },
        };
        dequeue_event(depth);
        if let GameEvent::MovementFanout {
            player_id,
            recipients,
            data,
        } = event
        {
            let (fanouts, barrier) =
                coalesce_movement_burst((player_id, recipients, data), rx, depth);
            pending = barrier;
            for (recipients, data) in fanouts {
                state.sessions.fanout(&recipients, &data);
                crate::metrics::PRESENTATION_EVENTS_TOTAL
                    .with_label_values(&["movement_fanout", "coalesced_delivered"])
                    .inc();
            }
            continue;
        }
        if let GameEvent::WorldEffects { effects } = event {
            let (effects, barrier) = coalesce_world_effect_burst(effects, rx, depth);
            pending = barrier;
            deliver_world_effects(state, effects);
            crate::metrics::PRESENTATION_EVENTS_TOTAL
                .with_label_values(&["world_effects", "coalesced_delivered"])
                .inc();
            continue;
        }
        let kind = event.kind();
        update_depth(depth);
        deliver(state, event);
        crate::metrics::PRESENTATION_EVENTS_TOTAL
            .with_label_values(&[kind, "delivered"])
            .inc();
    }
    crate::metrics::PRESENTATION_QUEUE_DEPTH.set(0);
}

/// Merges only adjacent side-phase streams. Their effects retain FIFO order;
/// any other presentation event is a strict wire-order barrier.
fn coalesce_world_effect_burst(
    mut effects: Vec<crate::game::BroadcastEffect>,
    rx: &std::sync::mpsc::Receiver<GameEvent>,
    depth: &AtomicUsize,
) -> (Vec<crate::game::BroadcastEffect>, Option<GameEvent>) {
    for _ in 1..WORLD_EFFECT_COALESCE_LIMIT {
        let Ok(event) = rx.try_recv() else {
            break;
        };
        match event {
            GameEvent::WorldEffects {
                effects: next_effects,
            } => {
                dequeue_event(depth);
                effects.extend(next_effects);
            }
            event => return (effects, Some(event)),
        }
    }
    (effects, None)
}

type MovementFanout = (crate::game::PlayerId, Vec<crate::game::SessionId>, Vec<u8>);
type CoalescedMovementFanouts = Vec<(Vec<crate::game::SessionId>, Vec<u8>)>;

/// Collapses only an uninterrupted movement burst. A non-movement event stays a
/// delivery barrier, while the final packet order follows the last update seen
/// for each player instead of their numeric id.
fn coalesce_movement_burst(
    first: MovementFanout,
    rx: &std::sync::mpsc::Receiver<GameEvent>,
    depth: &AtomicUsize,
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
                dequeue_event(depth);
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
            crate::game::logic::player_init::deliver_player_init(state, session_id, &view);
        }
        GameEvent::SessionBatch {
            session_id,
            player_id,
            packets,
        } => crate::game::logic::player_init::deliver_initial_presentation(
            state, session_id, player_id, packets,
        ),
        GameEvent::RefreshChunks {
            session_id,
            player_id,
        } => {
            if state.sessions.session_for_player(player_id) == Some(session_id)
                && let Some(outbox) = state.sessions.outbox_for_session(session_id)
            {
                crate::game::logic::chunks::check_chunk_changed(state, &outbox, player_id);
            }
        }
        GameEvent::Fanout { recipients, data }
        | GameEvent::MovementFanout {
            recipients, data, ..
        } => {
            state.sessions.fanout(&recipients, &data);
        }
        GameEvent::ChatFanout { route, message } => {
            crate::game::logic::chat::deliver_chat_fanout(state, &route, &message);
        }
        GameEvent::WorldEffects { effects } => deliver_world_effects(state, effects),
        GameEvent::GuiView {
            session_id,
            player_id,
            view,
        } => deliver_gui_view(state, session_id, player_id, view),
    }
}

/// Delivers the exact ordered world-effect stream emitted by simulation. HB
/// aggregation cannot cross a direct/cell/block/non-HB barrier.
fn deliver_world_effects(state: &Arc<GameState>, effects: Vec<crate::game::BroadcastEffect>) {
    let mut hb_batches = HashMap::new();
    let mut nearby_recipients = HashMap::new();
    for effect in effects {
        match effect {
            crate::game::BroadcastEffect::Direct { session_id, data } => {
                flush_hb_batches(state, &mut hb_batches);
                if let Some(tx) = state.sessions.outbox_for_session(session_id) {
                    let _ = tx.send(data);
                }
            }
            crate::game::BroadcastEffect::CellUpdate(pos) => {
                flush_hb_batches(state, &mut hb_batches);
                let (x, y): (i32, i32) = pos.into();
                crate::game::broadcast_cell_update(state, x, y);
            }
            crate::game::BroadcastEffect::BlockUpdate(pos) => {
                flush_hb_batches(state, &mut hb_batches);
                let (x, y): (i32, i32) = pos.into();
                crate::game::logic::buildings::broadcast_block_at(state, x, y);
            }
            crate::game::BroadcastEffect::Nearby {
                cx,
                cy,
                data,
                exclude,
            } => {
                let Some(payload) = hb_payload(&data) else {
                    flush_hb_batches(state, &mut hb_batches);
                    state.broadcast_to_nearby(cx, cy, &data, exclude);
                    continue;
                };
                let recipients = nearby_recipients
                    .entry((cx, cy))
                    .or_insert_with(|| state.nearby_player_sessions(cx, cy));
                for &(_, session_id) in recipients
                    .iter()
                    .filter(|(player_id, _)| Some(*player_id) != exclude)
                {
                    hb_batches
                        .entry(session_id)
                        .or_insert_with(Vec::new)
                        .extend_from_slice(payload);
                }
            }
        }
    }
    flush_hb_batches(state, &mut hb_batches);
}

pub fn hb_payload(data: &[u8]) -> Option<&[u8]> {
    const HB_FRAME_HEADER_LEN: usize = 7;
    if data.len() >= HB_FRAME_HEADER_LEN && data[4] == b'B' && &data[5..7] == b"HB" {
        Some(&data[HB_FRAME_HEADER_LEN..])
    } else {
        None
    }
}

fn flush_hb_batches(
    state: &Arc<GameState>,
    batches: &mut HashMap<crate::game::SessionId, Vec<u8>>,
) {
    for (session_id, payload) in std::mem::take(batches) {
        if let Some(tx) = state.sessions.outbox_for_session(session_id) {
            let _ = tx.send(crate::net::session::wire::make_b_packet_bytes(
                "HB", &payload,
            ));
        }
    }
}

#[cfg(test)]
pub fn deliver_world_effects_for_test(
    state: &Arc<GameState>,
    effects: Vec<crate::game::BroadcastEffect>,
) {
    deliver_world_effects(state, effects);
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
            let payload = crate::game::logic::teleport::render(&view);
            crate::net::session::wire::send_u_packet(&tx, "GU", &payload);
        }
        crate::game::GuiView::Spot(view) => {
            let payload = crate::net::session::ui::spot::render(&view);
            crate::net::session::wire::send_u_packet(&tx, "GU", &payload);
        }
        crate::game::GuiView::Storage(view) => {
            let payload = crate::net::session::ui::storage::render(&view);
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
        | GameEvent::RefreshChunks { session_id, .. }
        | GameEvent::GuiView { session_id, .. } => {
            state.sessions.kick_session(session_id);
        }
        GameEvent::Fanout { recipients, .. } | GameEvent::MovementFanout { recipients, .. } => {
            for session_id in recipients {
                state.sessions.kick_session(session_id);
            }
        }
        GameEvent::WorldEffects { effects } => {
            for effect in effects {
                match effect {
                    crate::game::BroadcastEffect::Direct { session_id, .. } => {
                        state.sessions.kick_session(session_id);
                    }
                    crate::game::BroadcastEffect::Nearby {
                        cx, cy, exclude, ..
                    } => {
                        for session_id in state.nearby_session_ids(cx, cy, exclude) {
                            state.sessions.kick_session(session_id);
                        }
                    }
                    crate::game::BroadcastEffect::CellUpdate(pos)
                    | crate::game::BroadcastEffect::BlockUpdate(pos) => {
                        let (x, y): (i32, i32) = pos.into();
                        let (cx, cy) = crate::world::World::chunk_pos(x, y);
                        for session_id in state.nearby_session_ids(cx, cy, None) {
                            state.sessions.kick_session(session_id);
                        }
                    }
                }
            }
        }
        GameEvent::ChatFanout { .. } => {
            // Cannot reliably determine targets that caused failure
        }
    }
}

fn dequeue_event(depth: &AtomicUsize) {
    depth.fetch_sub(1, Ordering::Relaxed);
    update_depth(depth);
}

fn update_depth(depth: &AtomicUsize) {
    crate::metrics::PRESENTATION_QUEUE_DEPTH
        .set(i64::try_from(depth.load(Ordering::Relaxed)).unwrap_or(i64::MAX));
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

    #[test]
    fn movement_burst_keeps_latest_packet_in_last_update_order() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        tx.send(movement(2, 20)).unwrap();
        tx.send(movement(1, 10)).unwrap();
        tx.send(movement(2, 21)).unwrap();
        let depth = AtomicUsize::new(3);

        let GameEvent::MovementFanout {
            player_id,
            recipients,
            data,
        } = rx.recv().unwrap()
        else {
            panic!("expected movement fanout");
        };
        dequeue_event(&depth);
        let (fanouts, barrier) =
            coalesce_movement_burst((player_id, recipients, data), &rx, &depth);

        assert_eq!(
            fanouts,
            vec![
                (vec![SessionId::new(1)], vec![10]),
                (vec![SessionId::new(2)], vec![21])
            ]
        );
        assert!(barrier.is_none());
    }

    #[test]
    fn movement_burst_does_not_cross_a_delivery_barrier() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        tx.send(movement(1, 10)).unwrap();
        tx.send(GameEvent::Fanout {
            recipients: vec![SessionId::new(7)],
            data: vec![70],
        })
        .unwrap();
        tx.send(movement(1, 11)).unwrap();
        let depth = AtomicUsize::new(3);

        let GameEvent::MovementFanout {
            player_id,
            recipients,
            data,
        } = rx.recv().unwrap()
        else {
            panic!("expected movement fanout");
        };
        dequeue_event(&depth);
        let (fanouts, barrier) =
            coalesce_movement_burst((player_id, recipients, data), &rx, &depth);

        assert_eq!(fanouts, vec![(vec![SessionId::new(1)], vec![10])]);
        assert!(matches!(
            barrier,
            Some(GameEvent::Fanout { recipients, data })
                if recipients == vec![SessionId::new(7)] && data == vec![70]
        ));
        assert!(
            matches!(rx.recv(), Ok(GameEvent::MovementFanout { data, .. }) if data == vec![11])
        );
    }

    #[test]
    fn world_effect_burst_keeps_effect_order_and_stops_at_barrier() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        tx.send(GameEvent::WorldEffects {
            effects: vec![crate::game::BroadcastEffect::Direct {
                session_id: SessionId::new(1),
                data: vec![1],
            }],
        })
        .unwrap();
        tx.send(GameEvent::WorldEffects {
            effects: vec![crate::game::BroadcastEffect::Direct {
                session_id: SessionId::new(2),
                data: vec![2],
            }],
        })
        .unwrap();
        tx.send(GameEvent::Fanout {
            recipients: vec![SessionId::new(3)],
            data: vec![3],
        })
        .unwrap();
        let depth = AtomicUsize::new(3);

        let GameEvent::WorldEffects { effects } = rx.recv().unwrap() else {
            panic!("expected world effects");
        };
        dequeue_event(&depth);
        let (effects, barrier) = coalesce_world_effect_burst(effects, &rx, &depth);

        assert!(matches!(
            effects.as_slice(),
            [
                crate::game::BroadcastEffect::Direct { session_id, data },
                crate::game::BroadcastEffect::Direct { session_id: second_session, data: second_data },
            ] if *session_id == SessionId::new(1)
                && data == &vec![1]
                && *second_session == SessionId::new(2)
                && second_data == &vec![2]
        ));
        assert!(matches!(
            barrier,
            Some(GameEvent::Fanout { recipients, data })
                if recipients == vec![SessionId::new(3)] && data == vec![3]
        ));
    }

    #[test]
    fn world_effect_burst_is_bounded_without_losing_the_next_stream() {
        let (tx, rx) = std::sync::mpsc::sync_channel(WORLD_EFFECT_COALESCE_LIMIT + 1);
        for session in 0..=WORLD_EFFECT_COALESCE_LIMIT {
            tx.send(GameEvent::WorldEffects {
                effects: vec![crate::game::BroadcastEffect::Direct {
                    session_id: SessionId::new(u64::try_from(session).unwrap()),
                    data: vec![u8::try_from(session).unwrap()],
                }],
            })
            .unwrap();
        }
        let depth = AtomicUsize::new(WORLD_EFFECT_COALESCE_LIMIT + 1);

        let GameEvent::WorldEffects { effects } = rx.recv().unwrap() else {
            panic!("expected world effects");
        };
        dequeue_event(&depth);
        let (effects, barrier) = coalesce_world_effect_burst(effects, &rx, &depth);

        assert_eq!(effects.len(), WORLD_EFFECT_COALESCE_LIMIT);
        assert!(barrier.is_none());
        assert_eq!(depth.load(Ordering::Relaxed), 1);
        assert!(matches!(
            rx.recv().unwrap(),
            GameEvent::WorldEffects { effects }
                if matches!(
                    effects.as_slice(),
                    [crate::game::BroadcastEffect::Direct { session_id, data }]
                        if *session_id == SessionId::new(u64::try_from(WORLD_EFFECT_COALESCE_LIMIT).unwrap())
                            && data == &vec![u8::try_from(WORLD_EFFECT_COALESCE_LIMIT).unwrap()]
                )
        ));
    }
}
