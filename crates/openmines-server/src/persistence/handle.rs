use crossbeam_utils::CachePadded;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
#[allow(unused_imports)]
use std::time::{Duration, Instant};

use crate::game::{SaveCommand, SaveKind};

pub struct PersistenceEnvelope {
    pub(crate) command: SaveCommand,
    pub(crate) enqueued_at: Instant,
    pub(crate) completion:
        Option<tokio::sync::mpsc::OwnedPermit<crate::game::PersistenceCompletion>>,
}

#[derive(Default)]
pub struct PersistenceStatus {
    pub(crate) accepted: CachePadded<AtomicU64>,
    pub(crate) completed: CachePadded<AtomicU64>,
    pub(crate) high_water: CachePadded<AtomicU64>,
}

impl PersistenceStatus {
    pub(crate) fn mark_accepted(&self) {
        let accepted = self.accepted.fetch_add(1, Ordering::AcqRel) + 1;
        let completed = self.completed.load(Ordering::Acquire);
        let backlog = accepted.saturating_sub(completed);
        self.high_water.fetch_max(backlog, Ordering::AcqRel);
        crate::metrics::PERSISTENCE_QUEUE_DEPTH.set(i64::try_from(backlog).unwrap_or(i64::MAX));
        crate::metrics::PERSISTENCE_QUEUE_HIGH_WATER
            .set(i64::try_from(self.high_water.load(Ordering::Acquire)).unwrap_or(i64::MAX));
    }

    pub(crate) fn mark_completed(&self, count: usize) {
        self.completed
            .fetch_add(u64::try_from(count).unwrap_or(u64::MAX), Ordering::AcqRel);
        crate::metrics::PERSISTENCE_QUEUE_DEPTH
            .set(i64::try_from(self.backlog()).unwrap_or(i64::MAX));
    }

    pub(crate) fn backlog(&self) -> u64 {
        self.accepted
            .load(Ordering::Acquire)
            .saturating_sub(self.completed.load(Ordering::Acquire))
    }
}

#[derive(Clone)]
pub struct PersistenceHandle {
    pub(crate) tx: tokio::sync::mpsc::Sender<PersistenceEnvelope>,
    pub(crate) completion_tx: tokio::sync::mpsc::Sender<crate::game::PersistenceCompletion>,
    pub(crate) status: Arc<PersistenceStatus>,
}

pub struct PersistencePermit {
    pub(crate) permit: tokio::sync::mpsc::OwnedPermit<PersistenceEnvelope>,
    pub(crate) status: Arc<PersistenceStatus>,
    pub(crate) kind: SaveKind,
    pub(crate) completion:
        Option<tokio::sync::mpsc::OwnedPermit<crate::game::PersistenceCompletion>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistenceAdmissionError {
    Full,
    Closed,
}

impl PersistenceHandle {
    pub fn check_capacity(&self, kind: SaveKind) -> Result<(), PersistenceAdmissionError> {
        if self.tx.is_closed()
            || (matches!(
                kind,
                SaveKind::Program
                    | SaveKind::ProgramCreate
                    | SaveKind::BuildingDelete
                    | SaveKind::ChatColorCycle
            ) && self.completion_tx.is_closed())
        {
            return Err(PersistenceAdmissionError::Closed);
        }
        if self.tx.capacity() == 0
            || (matches!(
                kind,
                SaveKind::Program
                    | SaveKind::ProgramCreate
                    | SaveKind::BuildingDelete
                    | SaveKind::ChatColorCycle
            ) && self.completion_tx.capacity() == 0)
        {
            return Err(PersistenceAdmissionError::Full);
        }
        Ok(())
    }

    pub fn try_reserve(
        &self,
        kind: SaveKind,
    ) -> Result<PersistencePermit, PersistenceAdmissionError> {
        let completion = if matches!(
            kind,
            SaveKind::Program
                | SaveKind::ProgramCreate
                | SaveKind::BuildingDelete
                | SaveKind::ChatColorCycle
                | SaveKind::AdminMoneyAll
                | SaveKind::AdminRole
                | SaveKind::AdminSkill
                | SaveKind::ClanCommand
                | SaveKind::ChatResync
                | SaveKind::ChatMenu
                | SaveKind::ChatPrivate
                | SaveKind::Whois
                | SaveKind::ClanMenu
                | SaveKind::ProgramMenu
                | SaveKind::ProgramCopy
                | SaveKind::BuildingMenu
        ) {
            match self.completion_tx.clone().try_reserve_owned() {
                Ok(permit) => Some(permit),
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[kind.name(), "completion_saturated"])
                        .inc();
                    return Err(PersistenceAdmissionError::Full);
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                        .with_label_values(&[kind.name(), "completion_closed"])
                        .inc();
                    return Err(PersistenceAdmissionError::Closed);
                }
            }
        } else {
            None
        };
        match self.tx.clone().try_reserve_owned() {
            Ok(permit) => Ok(PersistencePermit {
                permit,
                status: self.status.clone(),
                kind,
                completion,
            }),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                    .with_label_values(&[kind.name(), "saturated"])
                    .inc();
                Err(PersistenceAdmissionError::Full)
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                crate::metrics::PERSISTENCE_COMMANDS_TOTAL
                    .with_label_values(&[kind.name(), "closed"])
                    .inc();
                Err(PersistenceAdmissionError::Closed)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn backlog(&self) -> u64 {
        self.status.backlog()
    }

    #[cfg(test)]
    pub(crate) fn test_channel(capacity: usize) -> (Self, PersistenceTestReceiver) {
        let (tx, rx) = tokio::sync::mpsc::channel(capacity);
        let (completion_tx, completion_rx) = tokio::sync::mpsc::channel(capacity);
        let status = Arc::new(PersistenceStatus::default());
        (
            Self {
                tx,
                completion_tx,
                status,
            },
            PersistenceTestReceiver { rx, completion_rx },
        )
    }
}

#[cfg(test)]
pub struct PersistenceTestReceiver {
    pub(crate) rx: tokio::sync::mpsc::Receiver<PersistenceEnvelope>,
    pub(crate) completion_rx: tokio::sync::mpsc::Receiver<crate::game::PersistenceCompletion>,
}

#[cfg(test)]
impl PersistenceTestReceiver {
    pub(crate) fn try_recv(&mut self) -> Option<SaveCommand> {
        self.rx.try_recv().ok().map(|envelope| envelope.command)
    }

    pub(crate) fn completion_capacity(&self) -> usize {
        self.completion_rx.capacity()
    }
}

impl PersistencePermit {
    pub fn publish(self, command: SaveCommand) {
        let kind = command.kind();
        assert_eq!(
            self.kind, kind,
            "persistence permit kind must match published command"
        );
        self.permit.send(PersistenceEnvelope {
            command,
            enqueued_at: Instant::now(),
            completion: self.completion,
        });
        self.status.mark_accepted();
        crate::metrics::PERSISTENCE_COMMANDS_TOTAL
            .with_label_values(&[kind.name(), "accepted"])
            .inc();
    }
}
