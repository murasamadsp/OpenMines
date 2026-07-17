#[allow(unused_imports)]
use std::time::Instant;
use tokio::sync::mpsc;

use super::super::logic::contracts::{CommandIngressClass, QueuedGameCommand};

pub struct CommandSenders {
    pub(crate) lifecycle: mpsc::Sender<QueuedGameCommand>,
    pub(crate) gameplay: mpsc::Sender<QueuedGameCommand>,
    pub(crate) internal: mpsc::Sender<QueuedGameCommand>,
}

pub struct CommandReceivers {
    pub(crate) lifecycle: mpsc::Receiver<QueuedGameCommand>,
    pub(crate) gameplay: mpsc::Receiver<QueuedGameCommand>,
    pub(crate) internal: mpsc::Receiver<QueuedGameCommand>,
    #[allow(dead_code)]
    pub(crate) next_class: usize,
}

impl CommandReceivers {
    pub fn try_recv_class(
        &mut self,
        class: CommandIngressClass,
    ) -> Result<QueuedGameCommand, mpsc::error::TryRecvError> {
        match class {
            CommandIngressClass::Lifecycle => self.lifecycle.try_recv(),
            CommandIngressClass::Gameplay => self.gameplay.try_recv(),
            CommandIngressClass::Internal => self.internal.try_recv(),
        }
    }

    #[cfg(test)]
    pub fn try_recv(&mut self) -> Result<QueuedGameCommand, mpsc::error::TryRecvError> {
        for offset in 0..3 {
            let class = (self.next_class + offset) % 3;
            let result = self.try_recv_class(match class {
                0 => CommandIngressClass::Lifecycle,
                1 => CommandIngressClass::Gameplay,
                2 => CommandIngressClass::Internal,
                _ => unreachable!(),
            });
            if let Ok(command) = result {
                self.next_class = (class + 1) % 3;
                return Ok(command);
            }
        }
        Err(mpsc::error::TryRecvError::Empty)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.lifecycle
            .len()
            .saturating_add(self.gameplay.len())
            .saturating_add(self.internal.len())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn close(&mut self) {
        self.lifecycle.close();
        self.gameplay.close();
        self.internal.close();
    }

    #[cfg(test)]
    pub(crate) fn test_with_gameplay(gameplay: mpsc::Receiver<QueuedGameCommand>) -> Self {
        let (lifecycle_tx, lifecycle) = mpsc::channel(1);
        let (internal_tx, internal) = mpsc::channel(1);
        drop(lifecycle_tx);
        drop(internal_tx);
        Self {
            lifecycle,
            gameplay,
            internal,
            next_class: 0,
        }
    }

    #[cfg(test)]
    pub(crate) const fn test_with_ingress(
        lifecycle: mpsc::Receiver<QueuedGameCommand>,
        gameplay: mpsc::Receiver<QueuedGameCommand>,
        internal: mpsc::Receiver<QueuedGameCommand>,
    ) -> Self {
        Self {
            lifecycle,
            gameplay,
            internal,
            next_class: 0,
        }
    }
}

#[cfg(test)]
mod command_ingress_tests {
    use super::*;
    use crate::game::actors::player::PlayerId;
    use crate::game::logic::contracts::{CommandSeq, GameCommand, PlayerCommand, SessionId};

    fn queued(sequence: u64) -> QueuedGameCommand {
        let now = Instant::now();
        QueuedGameCommand {
            player_id: PlayerId(1),
            session_id: SessionId::new(1),
            ingress_class: Some(CommandIngressClass::Gameplay),
            sequence: CommandSeq::new(sequence),
            received_at: now,
            enqueued_at: now,
            command: GameCommand::Player(PlayerCommand::KnownNoopTy {
                event: "test".to_owned(),
                payload: bytes::Bytes::new(),
            }),
        }
    }

    #[test]
    fn command_receivers_rotate_ready_workload_classes() {
        let (lifecycle_tx, lifecycle) = mpsc::channel(2);
        let (gameplay_tx, gameplay) = mpsc::channel(2);
        let (internal_tx, internal) = mpsc::channel(2);
        lifecycle_tx.try_send(queued(1)).unwrap();
        gameplay_tx.try_send(queued(2)).unwrap();
        internal_tx.try_send(queued(3)).unwrap();
        let mut receivers = CommandReceivers {
            lifecycle,
            gameplay,
            internal,
            next_class: 0,
        };

        assert_eq!(receivers.try_recv().unwrap().sequence.get(), 1);
        assert_eq!(receivers.try_recv().unwrap().sequence.get(), 2);
        assert_eq!(receivers.try_recv().unwrap().sequence.get(), 3);
    }

    #[tokio::test]
    async fn full_gameplay_ingress_rejects_without_consuming_lifecycle_reserve() {
        let mut gameplay = crate::config::GameplayConfig::runtime_baseline();
        gameplay.simulation.gameplay_ingress_capacity = 1;
        gameplay.simulation.lifecycle_ingress_capacity = 1;
        let test = crate::test_support::ServerTestHarness::with_gameplay(
            "bounded_ingress",
            "bounded-ingress-user",
            gameplay,
        )
        .await;
        let gameplay_command = || {
            GameCommand::Player(PlayerCommand::KnownNoopTy {
                event: "test".to_owned(),
                payload: bytes::Bytes::new(),
            })
        };

        assert!(test.state.enqueue_command(
            PlayerId(test.player.id),
            SessionId::new(1),
            gameplay_command()
        ));
        assert!(!test.state.enqueue_command(
            PlayerId(test.player.id),
            SessionId::new(1),
            gameplay_command()
        ));
        assert!(
            test.state
                .enqueue_lifecycle(
                    PlayerId(test.player.id),
                    SessionId::new(1),
                    PlayerCommand::Disconnect
                )
                .await
        );
    }
}
