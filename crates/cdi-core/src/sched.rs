// SPDX-License-Identifier: GPL-3.0-or-later
//! Event scheduler.
//!
//! Generic event ordering plus the documented regional Mono-I clock constants.
//! Machine devices express deadlines in CPU-cycle units derived from the
//! selected PAL or NTSC crystal. Event targets are IDs, not closures, so the
//! queue serializes for savestates.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub type Ticks = u64;

/// Master crystal frequency (Hz) on PAL Mono-I boards.
pub const PAL_CRYSTAL_HZ: u64 = 30_000_000;

/// Master crystal frequency (Hz) on NTSC Mono-I boards.
pub const NTSC_CRYSTAL_HZ: u64 = 30_209_800;

pub const PAL_CPU_HZ: u64 = PAL_CRYSTAL_HZ / 2;
pub const NTSC_CPU_HZ: u64 = NTSC_CRYSTAL_HZ / 2;
pub const MCD212_CPU_CYCLES_PER_LINE: u64 = 960;
pub const MCD212_HALF_LINE_CPU_CYCLES: u64 = 480;
pub const PAL_FIELD_CPU_CYCLES: u64 = MCD212_CPU_CYCLES_PER_LINE * 625 / 2;
pub const NTSC_FIELD_CPU_CYCLES: u64 = MCD212_CPU_CYCLES_PER_LINE * 525 / 2;

pub const fn cpu_clock_hz(pal: bool) -> u64 {
    if pal {
        PAL_CPU_HZ
    } else {
        NTSC_CPU_HZ
    }
}

/// Legacy PAL crystal alias retained for callers that require the original
/// constant. Runtime code must use the selected regional constants above.
pub const CRYSTAL_HZ: u64 = PAL_CRYSTAL_HZ;

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

    fn rational_deadline(
        clock_hz: u64,
        edge: u64,
        rate_numerator: u64,
        rate_denominator: u64,
    ) -> u64 {
        let numerator = edge * clock_hz * rate_denominator;
        numerator.div_ceil(rate_numerator)
    }

    #[test]
    fn regional_crystals_preserve_documented_line_and_field_cadence() {
        const CPU_DIVIDER: u64 = 2;
        let pal_cpu_hz = PAL_CPU_HZ;
        let ntsc_cpu_hz = NTSC_CPU_HZ;
        let cpu_ticks_per_line = 1_920 / CPU_DIVIDER;
        let pal_cpu_ticks_per_field = PAL_FIELD_CPU_CYCLES;
        let ntsc_cpu_ticks_per_field = NTSC_FIELD_CPU_CYCLES;

        assert_eq!(pal_cpu_hz, 15_000_000);
        assert_eq!(ntsc_cpu_hz, 15_104_900);
        assert_eq!(cpu_ticks_per_line, 960);
        assert_eq!(pal_cpu_ticks_per_field, 300_000);
        assert_eq!(ntsc_cpu_ticks_per_field, 252_000);
        assert_eq!(pal_cpu_hz / pal_cpu_ticks_per_field, 50);

        // The documented NTSC crystal and 525-line cadence are approximately
        // 59.94 fields/s rather than an exact 60 Hz integer period.
        assert_eq!(
            ntsc_cpu_hz * 1_001 - ntsc_cpu_ticks_per_field * 60_000,
            4_900
        );
    }

    #[test]
    fn regional_clocks_map_independent_rates_without_integer_drift() {
        for clock_hz in [PAL_CRYSTAL_HZ / 2, NTSC_CRYSTAL_HZ / 2] {
            assert_eq!(rational_deadline(clock_hz, 75, 75, 1), clock_hz);
            assert_eq!(rational_deadline(clock_hz, 100, 100, 1), clock_hz);

            // TN 94 permits a player to have one fixed system-tick rate at
            // either edge of this range; it must not fluctuate between them.
            assert_eq!(rational_deadline(clock_hz, 499, 499, 5), clock_hz * 5);
            assert_eq!(rational_deadline(clock_hz, 501, 501, 5), clock_hz * 5);
        }
    }

    #[test]
    fn deadlines_inside_one_instruction_retain_chronological_order() {
        let mut scheduler = Scheduler::new();
        let instruction_start = 1_000;
        let instruction_end = 1_160;
        scheduler.advance_to(instruction_start);
        scheduler.schedule_at(1_096, EventId(1));
        scheduler.schedule_at(1_120, EventId(2));
        scheduler.schedule_at(1_140, EventId(3));

        scheduler.advance_to(instruction_end);
        let due: Vec<(Ticks, u32)> = std::iter::from_fn(|| scheduler.pop_due())
            .map(|(deadline, event)| (deadline, event.0))
            .collect();

        assert_eq!(due, vec![(1_096, 1), (1_120, 2), (1_140, 3)]);
        assert!(due
            .iter()
            .all(|(deadline, _)| *deadline > instruction_start && *deadline < instruction_end));
    }

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
