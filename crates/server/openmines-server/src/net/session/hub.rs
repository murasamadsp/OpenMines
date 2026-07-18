use crate::game::{PlayerId, SessionId};
use crate::net::session::outbox::{Outbox, channel};
use dashmap::DashMap;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{mpsc, oneshot, watch};

struct SessionEntry {
    outbox: Outbox,
    player_id: Mutex<Option<PlayerId>>,
    kick: Mutex<Option<oneshot::Sender<()>>>,
}

pub struct OpenSession {
    pub id: SessionId,
    pub outbox: Outbox,
    pub receiver: mpsc::Receiver<Vec<u8>>,
    pub overflow: watch::Receiver<bool>,
    pub kick: oneshot::Receiver<()>,
}

pub struct SessionHub {
    next_id: AtomicU64,
    sessions: DashMap<SessionId, SessionEntry>,
    players: DashMap<PlayerId, SessionId>,
}

impl Default for SessionHub {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            sessions: DashMap::new(),
            players: DashMap::new(),
        }
    }
}

impl SessionHub {
    pub fn open(&self) -> OpenSession {
        let id = SessionId::new(self.next_id.fetch_add(1, Ordering::Relaxed));
        let (outbox, receiver) = channel();
        let overflow = outbox.overflow_receiver();
        let (kick_tx, kick) = oneshot::channel();
        self.sessions.insert(
            id,
            SessionEntry {
                outbox: outbox.clone(),
                player_id: Mutex::new(None),
                kick: Mutex::new(Some(kick_tx)),
            },
        );
        OpenSession {
            id,
            outbox,
            receiver,
            overflow,
            kick,
        }
    }

    pub fn bind_player(&self, session_id: SessionId, player_id: PlayerId) -> bool {
        let Some(session) = self.sessions.get(&session_id) else {
            return false;
        };
        *session.player_id.lock() = Some(player_id);
        drop(session);

        if let Some(previous) = self.players.insert(player_id, session_id)
            && previous != session_id
        {
            self.kick_session(previous);
        }
        true
    }

    pub fn close(&self, session_id: SessionId) -> Option<PlayerId> {
        let (_, session) = self.sessions.remove(&session_id)?;
        let player_id = *session.player_id.lock();
        if let Some(player_id) = player_id {
            match self.players.entry(player_id) {
                dashmap::mapref::entry::Entry::Occupied(entry) if *entry.get() == session_id => {
                    entry.remove();
                }
                dashmap::mapref::entry::Entry::Occupied(_)
                | dashmap::mapref::entry::Entry::Vacant(_) => {}
            }
        }
        player_id
    }

    pub fn outbox_for_session(&self, session_id: SessionId) -> Option<Outbox> {
        self.sessions
            .get(&session_id)
            .map(|session| session.outbox.clone())
    }

    pub fn outbox_for_player(&self, player_id: PlayerId) -> Option<Outbox> {
        let session_id = *self.players.get(&player_id)?;
        self.outbox_for_session(session_id)
    }

    pub fn session_for_player(&self, player_id: PlayerId) -> Option<SessionId> {
        self.players.get(&player_id).map(|entry| *entry)
    }

    pub fn fanout(&self, recipients: &[SessionId], data: &[u8]) {
        for &session_id in recipients {
            if let Some(outbox) = self.outbox_for_session(session_id) {
                let _ = outbox.send(data.to_vec());
            }
        }
    }

    pub fn is_player_connected(&self, player_id: PlayerId) -> bool {
        self.players.contains_key(&player_id)
    }

    pub fn kick_player(&self, player_id: PlayerId) -> bool {
        let Some(session_id) = self.session_for_player(player_id) else {
            return false;
        };
        self.kick_session(session_id)
    }

    pub(crate) fn kick_session(&self, session_id: SessionId) -> bool {
        let Some(session) = self.sessions.get(&session_id) else {
            return false;
        };
        session.kick.lock().take().is_some()
    }

    #[cfg(test)]
    pub fn register_test_outbox(&self, session_id: SessionId, outbox: Outbox) {
        let (kick_tx, _) = oneshot::channel();
        self.sessions.insert(
            session_id,
            SessionEntry {
                outbox,
                player_id: Mutex::new(None),
                kick: Mutex::new(Some(kick_tx)),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::SessionHub;
    use crate::game::PlayerId;

    #[test]
    fn reconnect_rebinds_player_without_old_close_removing_new_session() {
        let hub = SessionHub::default();
        let old = hub.open();
        let new = hub.open();
        let player_id = PlayerId::from(7);

        assert!(hub.bind_player(old.id, player_id));
        assert!(hub.bind_player(new.id, player_id));
        assert_eq!(hub.session_for_player(player_id), Some(new.id));

        assert_eq!(hub.close(old.id), Some(player_id));
        assert_eq!(hub.session_for_player(player_id), Some(new.id));
    }

    #[test]
    fn session_target_does_not_follow_player_reconnect() {
        let hub = SessionHub::default();
        let mut old = hub.open();
        let mut new = hub.open();
        let player_id = PlayerId::from(7);

        assert!(hub.bind_player(old.id, player_id));
        assert!(hub.bind_player(new.id, player_id));
        hub.outbox_for_session(old.id)
            .expect("old session remains until its connection closes")
            .send(vec![1])
            .expect("old outbox open");
        hub.outbox_for_player(player_id)
            .expect("new player session")
            .send(vec![2])
            .expect("new outbox open");

        assert_eq!(old.receiver.try_recv(), Ok(vec![1]));
        assert_eq!(new.receiver.try_recv(), Ok(vec![2]));
    }
}
