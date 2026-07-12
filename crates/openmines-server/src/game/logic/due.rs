use super::consumables::{BoomDueAction, ProtectorDueAction, RazDueAction};
use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DueAction {
    Boom(BoomDueAction),
    Protector(ProtectorDueAction),
    Raz(RazDueAction),
}

impl DueAction {
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Boom(_) => "boom",
            Self::Protector(_) => "protector",
            Self::Raz(_) => "raz",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScheduledDueAction {
    pub due_at: Instant,
    pub sequence: u64,
    pub action: DueAction,
}

pub struct DueActionQueue {
    actions: BTreeMap<(Instant, u64), DueAction>,
    capacity: usize,
    reserved: usize,
    next_sequence: u64,
}

#[must_use = "dropping a due action reservation cancels it"]
pub struct DueActionReservation<'a> {
    queue: &'a mut DueActionQueue,
    sequence: u64,
    active: bool,
}

impl DueActionQueue {
    #[must_use]
    pub const fn new(capacity: usize) -> Self {
        Self {
            actions: BTreeMap::new(),
            capacity,
            reserved: 0,
            next_sequence: 0,
        }
    }

    pub fn try_reserve(&mut self) -> Option<DueActionReservation<'_>> {
        if self.actions.len().saturating_add(self.reserved) >= self.capacity {
            return None;
        }

        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.checked_add(1)?;
        self.reserved += 1;

        Some(DueActionReservation {
            queue: self,
            sequence,
            active: true,
        })
    }

    pub fn pop_due(&mut self, now: Instant) -> Option<ScheduledDueAction> {
        let (&(due_at, sequence), _) = self.actions.first_key_value()?;
        if due_at > now {
            return None;
        }

        let (_, action) = self.actions.pop_first()?;
        Some(ScheduledDueAction {
            due_at,
            sequence,
            action,
        })
    }

    #[must_use]
    pub fn next_due_at(&self) -> Option<Instant> {
        self.actions
            .first_key_value()
            .map(|(&(due_at, _), _)| due_at)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

impl DueActionReservation<'_> {
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn publish(mut self, due_at: Instant, action: DueAction) {
        let previous = self.queue.actions.insert((due_at, self.sequence), action);
        debug_assert!(previous.is_none(), "due action sequence must be unique");
        self.queue.reserved -= 1;
        self.active = false;
    }
}

impl Drop for DueActionReservation<'_> {
    fn drop(&mut self) {
        if self.active {
            self.queue.reserved -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openmines_core::{PlayerId, WorldPos};
    use std::time::Duration;

    fn boom(rng_seed: u64) -> DueAction {
        DueAction::Boom(BoomDueAction {
            center: WorldPos(10, 20),
            rng_seed,
        })
    }

    fn protector(player_id: i32) -> DueAction {
        DueAction::Protector(ProtectorDueAction {
            center: WorldPos(11, 21),
            trigger_player_id: PlayerId(player_id),
        })
    }

    fn raz(player_id: i32) -> DueAction {
        DueAction::Raz(RazDueAction {
            center: WorldPos(12, 22),
            trigger_player_id: PlayerId(player_id),
        })
    }

    #[test]
    fn action_kinds_are_stable_metric_labels() {
        assert_eq!(boom(1).kind(), "boom");
        assert_eq!(protector(2).kind(), "protector");
        assert_eq!(raz(3).kind(), "raz");
    }

    #[test]
    fn capacity_rejects_new_reservations_when_saturated() {
        let now = Instant::now();
        let mut queue = DueActionQueue::new(2);

        queue.try_reserve().unwrap().publish(now, boom(1));
        queue.try_reserve().unwrap().publish(now, boom(2));

        assert_eq!(queue.len(), 2);
        assert!(queue.try_reserve().is_none());
    }

    #[test]
    fn dropping_reservation_releases_capacity() {
        let now = Instant::now();
        let mut queue = DueActionQueue::new(1);

        let canceled_sequence;
        {
            let reservation = queue.try_reserve().unwrap();
            canceled_sequence = reservation.sequence();
        }
        let reservation = queue.try_reserve().unwrap();
        assert_eq!(reservation.sequence(), canceled_sequence + 1);
        reservation.publish(now, boom(1));

        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn mixed_actions_with_equal_deadlines_pop_in_admission_order() {
        let due_at = Instant::now();
        let mut queue = DueActionQueue::new(3);

        queue.try_reserve().unwrap().publish(due_at, boom(11));
        queue.try_reserve().unwrap().publish(due_at, protector(22));
        queue.try_reserve().unwrap().publish(due_at, raz(33));

        let first = queue.pop_due(due_at).unwrap();
        let second = queue.pop_due(due_at).unwrap();
        let third = queue.pop_due(due_at).unwrap();
        assert_eq!(first.sequence, 0);
        assert_eq!(first.action, boom(11));
        assert_eq!(second.sequence, 1);
        assert_eq!(second.action, protector(22));
        assert_eq!(third.sequence, 2);
        assert_eq!(third.action, raz(33));
    }

    #[test]
    fn mixed_actions_are_not_popped_before_their_deadlines() {
        let now = Instant::now();
        let first_due_at = now + Duration::from_secs(1);
        let second_due_at = now + Duration::from_secs(2);
        let mut queue = DueActionQueue::new(2);
        queue
            .try_reserve()
            .unwrap()
            .publish(first_due_at, protector(7));
        queue.try_reserve().unwrap().publish(second_due_at, raz(8));

        assert_eq!(queue.next_due_at(), Some(first_due_at));
        assert!(queue.pop_due(now).is_none());
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.pop_due(first_due_at).unwrap().action, protector(7));
        assert!(queue.pop_due(first_due_at).is_none());
        assert_eq!(queue.next_due_at(), Some(second_due_at));
        assert_eq!(queue.pop_due(second_due_at).unwrap().action, raz(8));
        assert_eq!(queue.len(), 0);
    }
}
