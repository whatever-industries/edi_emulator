// SPDX-License-Identifier: GPL-3.0-or-later
//! Philips K-mode CD-i keyboard serial peripheral.
//!
//! The keyboard is an optional device on the SCC68070 UART. It supplies a
//! power-on ID, identifies itself again after the host negates RTS for at
//! least 10 ms and asserts it, then sends two-byte packets only when key or
//! modifier state changes.

use std::collections::VecDeque;

const BAUD: u64 = 1_200;
const BITS_PER_CHARACTER: u64 = 10; // one start, eight data, one stop
const RECOGNITION_LOW_MS: u64 = 10;
const POWER_ON_ID_MS: u64 = 100;
const K_MODE_ID: u8 = 0xCB;

/// Modifier state encoded in a K-mode keyboard packet.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct KeyboardModifiers {
    pub shift: bool,
    pub caps_lock: bool,
    pub supershift: bool,
    pub control: bool,
}

impl KeyboardModifiers {
    pub(crate) fn packet_bits(self) -> u8 {
        u8::from(self.shift)
            | (u8::from(self.caps_lock) << 1)
            | (u8::from(self.supershift) << 2)
            | (u8::from(self.control) << 3)
    }
}

/// Device-side state for a K-mode keyboard attached to UART port 2.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct KeyboardKMode {
    attached: bool,
    clock_hz: u64,
    rts_asserted: bool,
    rts_low_cycles: u64,
    pending: VecDeque<u8>,
    tx_countdown: Option<u64>,
    power_on_countdown: Option<u64>,
    power_on_id_pending: bool,
    pressed_keys: Vec<u8>,
    modifiers: KeyboardModifiers,
}

impl KeyboardKMode {
    pub fn new(clock_hz: u64) -> Self {
        Self {
            attached: false,
            clock_hz,
            rts_asserted: false,
            rts_low_cycles: 0,
            pending: VecDeque::new(),
            tx_countdown: None,
            power_on_countdown: None,
            power_on_id_pending: false,
            pressed_keys: Vec::new(),
            modifiers: KeyboardModifiers::default(),
        }
    }

    pub fn attached(&self) -> bool {
        self.attached
    }

    pub fn set_attached(&mut self, attached: bool) {
        self.attached = attached;
        self.reset_serial_state();
        self.power_on_countdown = attached.then(|| self.power_on_id_cycles());
    }

    pub fn set_clock_hz(&mut self, clock_hz: u64) {
        self.clock_hz = clock_hz;
    }

    /// Reset volatile keyboard/link state while retaining physical presence.
    pub fn reset(&mut self) {
        self.reset_serial_state();
        self.power_on_countdown = self.attached.then(|| self.power_on_id_cycles());
    }

    fn reset_serial_state(&mut self) {
        self.rts_asserted = false;
        self.rts_low_cycles = 0;
        self.pending.clear();
        self.tx_countdown = None;
        self.power_on_countdown = None;
        self.power_on_id_pending = false;
        self.pressed_keys.clear();
        self.modifiers = KeyboardModifiers::default();
    }

    /// Follow the UART receiver-enable output used as the keyboard RTS line.
    pub fn set_rts(&mut self, asserted: bool) {
        if asserted == self.rts_asserted {
            return;
        }
        self.rts_asserted = asserted;
        if !self.attached {
            return;
        }
        if !asserted {
            // Recognition aborts any current transfer and the keyboard clears
            // its buffered data before returning its ID.
            self.pending.clear();
            self.tx_countdown = None;
            self.rts_low_cycles = 0;
            return;
        }

        if self.power_on_id_pending {
            self.power_on_id_pending = false;
            self.queue_identification(self.character_cycles());
            return;
        }

        if self.rts_low_cycles >= self.recognition_low_cycles() {
            // The specification requires at least one bit time between RTS
            // assertion and the beginning of the ID character.
            self.power_on_countdown = None;
            self.power_on_id_pending = false;
            self.queue_identification(self.bit_cycles() + self.character_cycles());
        }
    }

    fn queue_identification(&mut self, delay: u64) {
        self.pending.clear();
        self.pending.push_back(K_MODE_ID);
        self.tx_countdown = Some(delay);
    }

    /// Feed one host key transition into the physical keyboard model.
    pub fn set_key(&mut self, code: u8, pressed: bool, modifiers: KeyboardModifiers) {
        if !self.attached {
            return;
        }
        if pressed {
            if let Some(index) = self.pressed_keys.iter().position(|held| *held == code) {
                self.pressed_keys.remove(index);
            }
            self.pressed_keys.push(code);
        } else if let Some(index) = self.pressed_keys.iter().position(|held| *held == code) {
            self.pressed_keys.remove(index);
        }
        self.modifiers = modifiers;
        self.queue_state_packet();
    }

    /// Update only Shift/Caps/Supershift/Control state.
    pub fn set_modifiers(&mut self, modifiers: KeyboardModifiers) {
        if !self.attached || modifiers == self.modifiers {
            return;
        }
        self.modifiers = modifiers;
        self.queue_state_packet();
    }

    fn queue_state_packet(&mut self) {
        let key = self.pressed_keys.last().copied();
        let extension = if key.is_some() { 0 } else { 0x02 };
        let key = key.unwrap_or(0);
        self.pending
            .push_back(0x80 | (self.modifiers.packet_bits() << 3) | extension | ((key >> 7) & 1));
        self.pending.push_back(key & 0x7F);
        if self.rts_asserted && self.tx_countdown.is_none() {
            self.tx_countdown = Some(self.character_cycles());
        }
    }

    /// Advance the keyboard and append completed serial characters to `out`.
    pub fn tick(&mut self, cycles: u64, out: &mut Vec<u8>) {
        if !self.attached {
            return;
        }
        if let Some(countdown) = self.power_on_countdown {
            if cycles >= countdown {
                self.power_on_countdown = None;
                if self.rts_asserted {
                    out.push(K_MODE_ID);
                } else {
                    self.power_on_id_pending = true;
                }
            } else {
                self.power_on_countdown = Some(countdown - cycles);
            }
        }
        if !self.rts_asserted {
            self.rts_low_cycles = self.rts_low_cycles.saturating_add(cycles);
            return;
        }

        let mut remaining = cycles;
        while let Some(countdown) = self.tx_countdown {
            if remaining < countdown {
                self.tx_countdown = Some(countdown - remaining);
                break;
            }
            remaining -= countdown;
            if let Some(byte) = self.pending.pop_front() {
                out.push(byte);
            }
            self.tx_countdown = self.pending.front().map(|_| self.character_cycles());
        }
    }

    pub(crate) fn cycles_until_next_event(&self) -> u64 {
        if !self.attached {
            return u64::MAX;
        }
        let power_on = self.power_on_countdown.unwrap_or(u64::MAX);
        if self.rts_asserted {
            self.tx_countdown.unwrap_or(u64::MAX).min(power_on).max(1)
        } else {
            // Reaching the 10 ms recognition threshold has no asynchronous
            // effect until the host asserts RTS, so it is not a scheduler
            // boundary of its own.
            power_on.max(1)
        }
    }

    fn bit_cycles(&self) -> u64 {
        self.clock_hz.div_ceil(BAUD)
    }

    fn character_cycles(&self) -> u64 {
        self.bit_cycles() * BITS_PER_CHARACTER
    }

    fn recognition_low_cycles(&self) -> u64 {
        self.clock_hz.div_ceil(1_000) * RECOGNITION_LOW_MS
    }

    fn power_on_id_cycles(&self) -> u64 {
        self.clock_hz.div_ceil(1_000) * POWER_ON_ID_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOCK: u64 = 15_000_000;

    #[test]
    fn recognition_requires_ten_milliseconds_low_and_returns_k_mode_id() {
        let mut keyboard = KeyboardKMode::new(CLOCK);
        keyboard.set_attached(true);
        keyboard.tick(149_999, &mut Vec::new());
        keyboard.set_rts(true);
        keyboard.tick(1_000_000, &mut Vec::new());
        assert!(keyboard.pending.is_empty());

        keyboard.set_rts(false);
        keyboard.tick(150_000, &mut Vec::new());
        keyboard.set_rts(true);
        let mut bytes = Vec::new();
        keyboard.tick(
            keyboard.bit_cycles() + keyboard.character_cycles(),
            &mut bytes,
        );
        assert_eq!(bytes, [K_MODE_ID]);
        keyboard.tick(keyboard.power_on_id_cycles(), &mut bytes);
        assert_eq!(
            bytes,
            [K_MODE_ID],
            "explicit recognition replaces power-on ID"
        );
    }

    #[test]
    fn powered_keyboard_advertises_id_after_one_hundred_milliseconds() {
        let mut keyboard = KeyboardKMode::new(CLOCK);
        keyboard.set_attached(true);
        keyboard.set_rts(true);
        let mut bytes = Vec::new();
        keyboard.tick(keyboard.power_on_id_cycles() - 1, &mut bytes);
        assert!(bytes.is_empty());
        keyboard.tick(1, &mut bytes);
        assert_eq!(bytes, [K_MODE_ID]);
    }

    #[test]
    fn press_release_and_modifiers_use_documented_two_byte_packets() {
        let mut keyboard = KeyboardKMode::new(CLOCK);
        keyboard.set_attached(true);
        keyboard.tick(150_000, &mut Vec::new());
        keyboard.set_rts(true);
        let mut bytes = Vec::new();
        keyboard.tick(137_500, &mut bytes);
        assert_eq!(bytes, [K_MODE_ID]);

        let modifiers = KeyboardModifiers {
            shift: true,
            control: true,
            ..KeyboardModifiers::default()
        };
        keyboard.set_key(b'A', true, modifiers);
        keyboard.tick(250_000, &mut bytes);
        keyboard.set_key(b'A', false, KeyboardModifiers::default());
        keyboard.tick(250_000, &mut bytes);
        assert_eq!(bytes, [K_MODE_ID, 0xC8, b'A', 0x82, 0x00]);
    }

    #[test]
    fn unchanged_state_is_silent() {
        let mut keyboard = KeyboardKMode::new(CLOCK);
        keyboard.set_attached(true);
        keyboard.set_modifiers(KeyboardModifiers::default());
        keyboard.tick(CLOCK, &mut Vec::new());
        assert!(keyboard.pending.is_empty());
        assert!(keyboard.tx_countdown.is_none());
    }
}
