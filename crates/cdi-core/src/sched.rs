// SPDX-License-Identifier: GPL-2.0-or-later
//! Event scheduler.
//!
//! One master time base (30 MHz crystal ticks). The CPU free-runs to the
//! next event deadline; devices schedule their next interesting moment and
//! are caught up lazily. Event targets are IDs, not closures, so the queue
//! serializes for savestates.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub type Ticks = u64;

/// Master crystal frequency (Hz) on Mono-I boards.
pub const CRYSTAL_HZ: u64 = 30_000_000;

/// Identifies who an event belongs to and what it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct EventId(pub u32);

#[derive(Debug, Default)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct Scheduler {
    now: Ticks,
    /// Min-heap of (deadline, sequence, id); sequence breaks ties FIFO so
    /// dispatch order is deterministic.
    #[cfg_attr(
        feature = "savestate",
        serde(
            with = "heap_serde",
            default,
            skip_serializing_if = "BinaryHeap::is_empty"
        )
    )]
    queue: BinaryHeap<Reverse<(Ticks, u64, EventId)>>,
    seq: u64,
}

impl Scheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn now(&self) -> Ticks {
        self.now
    }

    /// Advance the master clock. Callers must dispatch due events afterward.
    pub fn advance_to(&mut self, t: Ticks) {
        debug_assert!(t >= self.now);
        self.now = t;
    }

    pub fn schedule_at(&mut self, when: Ticks, id: EventId) {
        self.queue.push(Reverse((when, self.seq, id)));
        self.seq += 1;
    }

    pub fn schedule_in(&mut self, delay: Ticks, id: EventId) {
        self.schedule_at(self.now + delay, id);
    }

    /// Remove every pending occurrence of an event (e.g. a device being
    /// reconfigured mid-flight).
    pub fn cancel(&mut self, id: EventId) {
        let old = std::mem::take(&mut self.queue);
        self.queue = old
            .into_iter()
            .filter(|Reverse((_, _, e))| *e != id)
            .collect();
    }

    /// Deadline of the earliest pending event, if any.
    pub fn next_deadline(&self) -> Option<Ticks> {
        self.queue.peek().map(|Reverse((t, _, _))| *t)
    }

    /// Pop the next event due at or before the current time.
    pub fn pop_due(&mut self) -> Option<(Ticks, EventId)> {
        match self.queue.peek() {
            Some(Reverse((t, _, _))) if *t <= self.now => {
                let Reverse((t, _, id)) = self.queue.pop().unwrap();
                Some((t, id))
            }
            _ => None,
        }
    }
}

#[cfg(feature = "savestate")]
mod heap_serde {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    type Entry = Reverse<(Ticks, u64, EventId)>;

    pub fn serialize<S: Serializer>(heap: &BinaryHeap<Entry>, s: S) -> Result<S::Ok, S::Error> {
        let items: Vec<(Ticks, u64, EventId)> = heap.iter().map(|Reverse(e)| *e).collect();
        items.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BinaryHeap<Entry>, D::Error> {
        let items = Vec::<(Ticks, u64, EventId)>::deserialize(d)?;
        Ok(items.into_iter().map(Reverse).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_order_is_deterministic() {
        let mut s = Scheduler::new();
        s.schedule_at(100, EventId(1));
        s.schedule_at(50, EventId(2));
        s.schedule_at(100, EventId(3)); // same deadline as EventId(1), later insert
        s.advance_to(100);
        let order: Vec<u32> = std::iter::from_fn(|| s.pop_due())
            .map(|(_, e)| e.0)
            .collect();
        assert_eq!(order, vec![2, 1, 3]);
    }

    #[test]
    fn future_events_stay_queued() {
        let mut s = Scheduler::new();
        s.schedule_in(10, EventId(7));
        assert_eq!(s.pop_due(), None);
        assert_eq!(s.next_deadline(), Some(10));
        s.advance_to(10);
        assert_eq!(s.pop_due(), Some((10, EventId(7))));
    }

    #[test]
    fn cancel_removes_all_occurrences() {
        let mut s = Scheduler::new();
        s.schedule_at(5, EventId(1));
        s.schedule_at(6, EventId(1));
        s.schedule_at(7, EventId(2));
        s.cancel(EventId(1));
        s.advance_to(10);
        assert_eq!(s.pop_due(), Some((7, EventId(2))));
        assert_eq!(s.pop_due(), None);
    }
}
