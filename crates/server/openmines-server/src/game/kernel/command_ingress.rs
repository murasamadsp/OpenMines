use crate::game::kernel::broadcast::BroadcastEffect;
use crate::game::kernel::ingress::CommandSenders;
use crate::game::logic::contracts::{
    CommandIngressClass, CommandSeq, GameCommand, PlayerCommand, QueuedGameCommand, SessionId,
};
use crate::simulation_waker::SimulationWaker;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

pub struct CommandIngress {
    commands_tx: CommandSenders,
    waker: SimulationWaker,
    command_seq: AtomicU64,
    command_queue_depth: AtomicUsize,
    command_queue_high_water: AtomicUsize,
    class_depth: [AtomicUsize; 3],
    class_ages: [Mutex<VecDeque<Instant>>; 3],
    command_broadcasts: Mutex<Vec<BroadcastEffect>>,
}

impl CommandIngress {
    pub fn new(commands_tx: CommandSenders, waker: SimulationWaker) -> Self {
        Self {
            commands_tx,
            waker,
            command_seq: AtomicU64::new(1),
            command_queue_depth: AtomicUsize::new(0),
            command_queue_high_water: AtomicUsize::new(0),
            class_depth: std::array::from_fn(|_| AtomicUsize::new(0)),
            class_ages: std::array::from_fn(|_| Mutex::new(VecDeque::new())),
            command_broadcasts: Mutex::new(Vec::new()),
        }
    }

    pub async fn enqueue_lifecycle(
        &self,
        player_id: crate::game::actors::player::PlayerId,
        session_id: SessionId,
        command: PlayerCommand,
    ) -> bool {
        debug_assert_eq!(command.ingress_class(), CommandIngressClass::Lifecycle);
        let kind = command.name();
        let received_at = Instant::now();
        let Ok(permit) = self.commands_tx.lifecycle.reserve().await else {
            crate::metrics::COMMANDS_TOTAL
                .with_label_values(&[kind, "ingress_closed"])
                .inc();
            return false;
        };
        let enqueued_at = Instant::now();
        let sequence = self.allocate_command_sequence();
        let class = CommandIngressClass::Lifecycle;
        let queued = QueuedGameCommand {
            player_id,
            session_id,
            ingress_class: Some(class),
            sequence,
            received_at,
            enqueued_at,
            command: GameCommand::Player(command),
        };
        let depth = self
            .command_queue_depth
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let class_depth = self.class_depth[class.index()]
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let high_water = self
            .command_queue_high_water
            .fetch_max(depth, Ordering::Relaxed)
            .max(depth);
        crate::metrics::COMMANDS_TOTAL
            .with_label_values(&[kind, "enqueued"])
            .inc();
        crate::metrics::COMMAND_QUEUE_DEPTH.set(i64::try_from(depth).unwrap_or(i64::MAX));
        crate::metrics::COMMAND_QUEUE_HIGH_WATER.set(i64::try_from(high_water).unwrap_or(i64::MAX));
        crate::metrics::COMMAND_INGRESS_DEPTH
            .with_label_values(&[class.metric_name()])
            .set(i64::try_from(class_depth).unwrap_or(i64::MAX));
        self.push_command_ingress_age(class, enqueued_at);
        permit.send(queued);
        self.waker.wake();
        true
    }

    pub async fn enqueue_internal(
        &self,
        player_id: crate::game::actors::player::PlayerId,
        session_id: SessionId,
        command: PlayerCommand,
    ) -> bool {
        debug_assert_eq!(command.ingress_class(), CommandIngressClass::Internal);
        let kind = command.name();
        let received_at = Instant::now();
        let Ok(permit) = self.commands_tx.internal.reserve().await else {
            crate::metrics::COMMANDS_TOTAL
                .with_label_values(&[kind, "ingress_closed"])
                .inc();
            return false;
        };
        let enqueued_at = Instant::now();
        let sequence = self.allocate_command_sequence();
        let class = CommandIngressClass::Internal;
        let queued = QueuedGameCommand {
            player_id,
            session_id,
            ingress_class: Some(class),
            sequence,
            received_at,
            enqueued_at,
            command: GameCommand::Player(command),
        };
        let depth = self
            .command_queue_depth
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let class_depth = self.class_depth[class.index()]
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let high_water = self
            .command_queue_high_water
            .fetch_max(depth, Ordering::Relaxed)
            .max(depth);
        crate::metrics::COMMANDS_TOTAL
            .with_label_values(&[kind, "enqueued"])
            .inc();
        crate::metrics::COMMAND_QUEUE_DEPTH.set(i64::try_from(depth).unwrap_or(i64::MAX));
        crate::metrics::COMMAND_QUEUE_HIGH_WATER.set(i64::try_from(high_water).unwrap_or(i64::MAX));
        crate::metrics::COMMAND_INGRESS_DEPTH
            .with_label_values(&[class.metric_name()])
            .set(i64::try_from(class_depth).unwrap_or(i64::MAX));
        self.push_command_ingress_age(class, enqueued_at);
        permit.send(queued);
        self.waker.wake();
        true
    }

    pub fn enqueue_command(
        &self,
        player_id: crate::game::actors::player::PlayerId,
        session_id: SessionId,
        command: GameCommand,
    ) -> bool {
        self.enqueue_command_received(player_id, session_id, command, Instant::now())
    }

    pub fn enqueue_command_received(
        &self,
        player_id: crate::game::actors::player::PlayerId,
        session_id: SessionId,
        command: GameCommand,
        received_at: Instant,
    ) -> bool {
        let GameCommand::Player(action) = &command;
        let (kind, class) = (action.name(), action.ingress_class());
        assert_ne!(
            class,
            CommandIngressClass::Internal,
            "internal follow-up must use awaitable CommandIngress::enqueue_internal"
        );
        let enqueued_at = Instant::now();
        let sequence = self.allocate_command_sequence();
        let queued = QueuedGameCommand {
            player_id,
            session_id,
            ingress_class: Some(class),
            sequence,
            received_at,
            enqueued_at,
            command,
        };
        crate::metrics::COMMAND_RECEIVE_TO_ENQUEUE_SECONDS
            .with_label_values(&[kind])
            .observe(
                enqueued_at
                    .saturating_duration_since(received_at)
                    .as_secs_f64(),
            );
        let sender = match class {
            CommandIngressClass::Lifecycle => &self.commands_tx.lifecycle,
            CommandIngressClass::Gameplay => &self.commands_tx.gameplay,
            CommandIngressClass::Internal => &self.commands_tx.internal,
        };
        let Ok(permit) = sender.try_reserve() else {
            crate::metrics::COMMANDS_TOTAL
                .with_label_values(&[kind, "ingress_rejected"])
                .inc();
            crate::metrics::COMMANDS_TOTAL
                .with_label_values(&[class.metric_name(), "ingress_rejected"])
                .inc();
            return false;
        };
        let depth = self
            .command_queue_depth
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let class_depth = self.class_depth[class.index()]
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let high_water = self
            .command_queue_high_water
            .fetch_max(depth, Ordering::Relaxed)
            .max(depth);
        crate::metrics::COMMANDS_TOTAL
            .with_label_values(&[kind, "enqueued"])
            .inc();
        crate::metrics::COMMAND_QUEUE_DEPTH.set(i64::try_from(depth).unwrap_or(i64::MAX));
        crate::metrics::COMMAND_QUEUE_HIGH_WATER.set(i64::try_from(high_water).unwrap_or(i64::MAX));
        crate::metrics::COMMAND_INGRESS_DEPTH
            .with_label_values(&[class.metric_name()])
            .set(i64::try_from(class_depth).unwrap_or(i64::MAX));
        self.push_command_ingress_age(class, enqueued_at);
        permit.send(queued);
        self.waker.wake();
        true
    }

    fn push_command_ingress_age(&self, class: CommandIngressClass, enqueued_at: Instant) {
        let mut ages = self.class_ages[class.index()].lock();
        ages.push_back(enqueued_at);
        Self::record_oldest_command_ingress_age(class, ages.front().copied());
    }

    fn pop_command_ingress_age(&self, class: CommandIngressClass) {
        let mut ages = self.class_ages[class.index()].lock();
        assert!(ages.pop_front().is_some(), "command ingress age underflow");
        Self::record_oldest_command_ingress_age(class, ages.front().copied());
    }

    pub(crate) fn refresh_command_ingress_oldest_ages(&self) {
        for class in [
            CommandIngressClass::Lifecycle,
            CommandIngressClass::Gameplay,
            CommandIngressClass::Internal,
        ] {
            let ages = self.class_ages[class.index()].lock();
            Self::record_oldest_command_ingress_age(class, ages.front().copied());
        }
    }

    fn record_oldest_command_ingress_age(class: CommandIngressClass, oldest: Option<Instant>) {
        let age = oldest.map_or(Duration::ZERO, |t| t.elapsed());
        crate::metrics::COMMAND_INGRESS_OLDEST_AGE_SECONDS
            .with_label_values(&[class.metric_name()])
            .set(age.as_secs_f64());
    }

    pub(crate) fn allocate_command_sequence(&self) -> CommandSeq {
        CommandSeq::new(self.command_seq.fetch_add(1, Ordering::Relaxed))
    }

    pub(crate) fn simulation_waker(&self) -> SimulationWaker {
        self.waker.clone()
    }
    pub fn record_command_dequeued(&self, class: CommandIngressClass) {
        let previous = self.command_queue_depth.fetch_sub(1, Ordering::Relaxed);
        debug_assert!(previous > 0, "command queue depth underflow");
        let depth = previous.saturating_sub(1);
        crate::metrics::COMMAND_QUEUE_DEPTH.set(i64::try_from(depth).unwrap_or(i64::MAX));
        let previous_class = self.class_depth[class.index()].fetch_sub(1, Ordering::Relaxed);
        debug_assert!(previous_class > 0, "command ingress depth underflow");
        crate::metrics::COMMAND_INGRESS_DEPTH
            .with_label_values(&[class.metric_name()])
            .set(i64::try_from(previous_class.saturating_sub(1)).unwrap_or(i64::MAX));
        self.pop_command_ingress_age(class);
    }

    pub fn queue_direct(&self, session_id: SessionId, data: Vec<u8>) {
        self.command_broadcasts
            .lock()
            .push(BroadcastEffect::Direct { session_id, data });
    }
    pub fn queue_nearby(
        &self,
        cx: u32,
        cy: u32,
        data: Vec<u8>,
        exclude: Option<crate::game::actors::player::PlayerId>,
    ) {
        self.command_broadcasts
            .lock()
            .push(BroadcastEffect::Nearby {
                cx,
                cy,
                data,
                exclude,
            });
    }
    pub fn drain_command_broadcasts(&self) -> Vec<BroadcastEffect> {
        std::mem::take(&mut *self.command_broadcasts.lock())
    }
}
