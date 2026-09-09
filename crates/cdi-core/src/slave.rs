// SPDX-License-Identifier: GPL-3.0-or-later
//! CD-i Mono-I SLAVE MCU high-level emulation.
//!
//! Ported from MAME `src/mame/philips/cdislavehle.cpp` (BSD-3-Clause,
//! Ryan Holtz) — see NOTICE.md. The SLAVE is an MC68HC705 handling input
//! devices, audio attenuation, the front panel, and player status; the CPU
//! talks to it over four byte channels at `$310000` (odd bytes). Responses
//! are delivered after a delay and raise IN2 (autovector 26).

use std::collections::VecDeque;

use crate::keyboard::KeyboardModifiers;

/// CPU cycles (15 MHz domain) per microsecond.
#[cfg(test)]
const CYCLES_PER_US: u64 = 15;
/// Response latency used by MAME for most queries (100 µs).
#[cfg(test)]
const READBACK_DELAY: u64 = 100 * CYCLES_PER_US;
/// Input poll cadence (60 Hz).
#[cfg(test)]
const POLL_INTERVAL: u64 = 15_000_000 / 60;

#[cfg(feature = "savestate")]
const fn default_cpu_clock_hz() -> u64 {
    crate::sched::PAL_CPU_HZ
}
/// `cdapdriv` disc-type field for a native CD-i disc.
const DISC_TYPE_CDI: u8 = 0x02;
/// `cdapdriv` disc-type field for a CD-ROM XA Bridge disc.
const DISC_TYPE_CD_ROM_XA_BRIDGE: u8 = 0x04;

fn pointer_packet_fields(packet: [u8; 4]) -> (i32, i32, u8) {
    let x = (i32::from(packet[0] & 0x07) << 7) | i32::from(packet[1] & 0x7f);
    let y = (i32::from(packet[2] & 0x07) << 7) | i32::from(packet[3] & 0x7f);
    let button_code = (packet[0] >> 3) & 0x07;
    (x, y, button_code)
}

#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
struct OutChannel {
    buf: [u8; 4],
    index: usize,
    count: usize,
    cmd: u8,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
struct PendingReadback {
    delay: Option<u64>,
    count: usize,
    data: [u8; 4],
    cmd: u8,
}

/// Minimum player-control keys defined by the Green Book for `/pck2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerControlKey {
    Play,
    Stop,
    Pause,
    Next,
    Previous,
}

impl PlayerControlKey {
    fn slave_mask(self) -> u8 {
        match self {
            Self::Play => 0x20,
            Self::Stop => 0x04,
            Self::Pause => 0x10,
            Self::Next => 0x01,
            Self::Previous => 0x02,
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct SlaveHle {
    channels: [OutChannel; 4],
    in_buf: [u8; 17],
    in_index: usize,
    in_count: usize,
    polling_active: bool,
    /// Firmware `$63.4`: allow pending pointer events to notify the host.
    pointer_interrupt_enable: bool,
    xbus_interrupt_enable: bool,
    /// Channel-1 player-control reports are enabled by the native `/pck2`
    /// driver with command $8D and disabled with $8E.
    #[cfg_attr(feature = "savestate", serde(default))]
    player_control_enabled: bool,
    #[cfg_attr(feature = "savestate", serde(default))]
    player_control_mask: u8,
    #[cfg_attr(feature = "savestate", serde(default))]
    pending_player_control: VecDeque<[u8; 2]>,
    /// A K-mode keyboard is physically present on the SLAVE-managed input
    /// port, so F3 advertises the combined keyboard/pointer device class.
    #[cfg_attr(feature = "savestate", serde(default))]
    keyboard_attached: bool,
    /// Firmware `$56.1`, set/cleared by channel-C commands $80/$81.
    #[cfg_attr(feature = "savestate", serde(default))]
    keyboard_events_enabled: bool,
    #[cfg_attr(feature = "savestate", serde(default))]
    keyboard_pressed_keys: Vec<u8>,
    #[cfg_attr(feature = "savestate", serde(default))]
    keyboard_modifiers: KeyboardModifiers,
    #[cfg_attr(feature = "savestate", serde(default))]
    pending_keyboard: VecDeque<[u8; 3]>,
    /// Ordered status-query responses sharing the CPU-visible channel C.
    /// Real firmware retains one-shot request flags until each reply has
    /// crossed the channel; a later query must not overwrite an earlier one.
    #[cfg_attr(feature = "savestate", serde(default))]
    pending_channel2_readbacks: VecDeque<PendingReadback>,
    lcd_state: [u8; 16],
    /// SLAVE revision bytes reported to the BIOS (e.g. "21" for v3231).
    version: [u8; 2],
    /// Video standard byte for the 0xF6 query (2 = PAL, 1 = NTSC).
    video_status: u8,
    /// Test-plug status returned by the 0xF4 query. Firmware forms it from
    /// `$55` bits 0-1; the 0x87/0x88 ch0 pair controls bit 1.
    boot_status: u8,
    /// Latched request for the SLAVE to reset the 68070 host.
    host_reset_requested: bool,
    /// The retained-RAM launch mode selected by ch2 0x8A. In this mode the
    /// drive-status packet advertises the follow-up B1 disc-base response.
    disc_boot_mode: bool,
    /// Bits 0-2 of byte 2 in the four-byte B0 drive-status response.
    /// `cdapdriv` exposes these through GetStat $55.
    disc_type_code: u8,
    /// Whether a medium is currently present in the drive.
    disc_present: bool,
    /// SERVO drive-status packets waiting for the host's X-Bus channel.
    pending_drive_status: VecDeque<[u8; 4]>,
    /// Firmware `$59.1`, toggled by ch2 0x88. It enables the two-byte
    /// parameter form armed by ch2 0x90.
    transport_adjust_enabled: bool,
    /// Number of parameter bytes still expected after an accepted ch2 0x90.
    transport_parameter_bytes: u8,
    /// Firmware `$D8`: repeat delay used by the transport-position controls.
    transport_repeat_delay: u8,

    // Pointer device state (absolute position, updated by the frontend).
    input_x: i32,
    input_y: i32,
    input_buttons: u8,
    device_x: i32,
    device_y: i32,
    last_x: i32,
    last_y: i32,
    last_buttons: u8,
    // Relative pointer motion queued by the frontend, merged into the device
    // position at the next input poll.
    pending_dx: i32,
    pending_dy: i32,

    /// Latest audio-attenuation command payload for the CDIC, if pending.
    attenuation: Option<u32>,
    #[cfg_attr(feature = "savestate", serde(default = "default_cpu_clock_hz"))]
    clock_hz: u64,
    /// IRQ (IN2) line state.
    irq_asserted: bool,
    /// Channels currently contributing to the shared level-sensitive IN2.
    #[cfg_attr(feature = "savestate", serde(default))]
    irq_pending_channels: u8,
    /// Per-channel cycles until a delayed response raises the shared IRQ.
    #[cfg_attr(feature = "savestate", serde(default))]
    irq_countdowns: [Option<u64>; 4],
    poll_countdown: u64,
}

/// Read-only SLAVE transport/input state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct SlaveDiagnosticSnapshot {
    pub irq_asserted: bool,
    pub polling_active: bool,
    pub pointer_interrupt_enabled: bool,
    pub xbus_interrupt_enabled: bool,
    pub input_x: i32,
    pub input_y: i32,
    pub device_x: i32,
    pub device_y: i32,
    pub buttons: u8,
    pub disc_boot_mode: bool,
    pub disc_present: bool,
    pub disc_type_code: u8,
    /// Four-byte B0 packet currently synthesized at the HLE's SERVO boundary.
    ///
    /// Real firmware builds this packet in SERVO `$178B..$181D` and the
    /// SLAVE forwards it unchanged. This is explicitly a modeled packet,
    /// not a claim that a low-level SERVO MCU executed in this HLE.
    #[cfg_attr(feature = "savestate", serde(default))]
    pub modeled_servo_b0_packet: [u8; 4],
    /// Oldest unsolicited SERVO packet waiting for channel-3 delivery.
    #[cfg_attr(feature = "savestate", serde(default))]
    pub next_forwarded_servo_packet: Option<[u8; 4]>,
    /// B0/B1 packet currently exposed in the channel-3 read window.
    ///
    /// Unlike `modeled_servo_b0_packet`, this is the packet the BIOS can
    /// actually consume now. Unlike `next_forwarded_servo_packet`, it has
    /// already crossed the HLE's SLAVE/SERVO forwarding boundary.
    #[cfg_attr(feature = "savestate", serde(default))]
    pub bios_visible_drive_packet: Option<[u8; 4]>,
    pub pending_drive_status: usize,
    pub boot_status: u8,
    pub video_status: u8,
    pub host_reset_pending: bool,
}

impl SlaveHle {
    pub fn new(version: &str, pal: bool) -> Self {
        // The model files give the version as hex byte pairs: "3231" ->
        // response bytes $32 $31 (what MAME hardcodes for Mono-I).
        let byte = |s: &str| u8::from_str_radix(s, 16).unwrap_or(0);
        let version = if version.len() >= 4 {
            [byte(&version[0..2]), byte(&version[2..4])]
        } else {
            [0x32, 0x31]
        };
        Self {
            channels: Default::default(),
            in_buf: [0; 17],
            in_index: 0,
            in_count: 0,
            polling_active: false,
            pointer_interrupt_enable: false,
            xbus_interrupt_enable: false,
            player_control_enabled: false,
            player_control_mask: 0,
            pending_player_control: VecDeque::new(),
            keyboard_attached: false,
            keyboard_events_enabled: false,
            keyboard_pressed_keys: Vec::new(),
            keyboard_modifiers: KeyboardModifiers::default(),
            pending_keyboard: VecDeque::new(),
            pending_channel2_readbacks: VecDeque::new(),
            lcd_state: [0; 16],
            version,
            video_status: if pal { 2 } else { 1 },
            boot_status: 0,
            host_reset_requested: false,
            disc_boot_mode: false,
            disc_type_code: DISC_TYPE_CDI,
            disc_present: false,
            pending_drive_status: VecDeque::new(),
            transport_adjust_enabled: false,
            transport_parameter_bytes: 0,
            transport_repeat_delay: 0x0A,
            input_x: 0,
            input_y: 0,
            input_buttons: 0,
            device_x: 0,
            device_y: 0,
            last_x: -1,
            last_y: -1,
            last_buttons: 0,
            pending_dx: 0,
            pending_dy: 0,
            attenuation: None,
            clock_hz: crate::sched::cpu_clock_hz(pal),
            irq_asserted: false,
            irq_pending_channels: 0,
            irq_countdowns: [None; 4],
            poll_countdown: crate::sched::cpu_clock_hz(pal) / 60,
        }
    }

    fn readback_delay(&self) -> u64 {
        self.clock_hz / 10_000
    }

    fn poll_interval(&self) -> u64 {
        self.clock_hz / 60
    }

    /// Select the player video standard reported by the firmware's `F6`
    /// status query. Real Mono-I players were sold in both PAL and NTSC
    /// configurations even when they shared the same main system ROM.
    pub fn set_video_standard(&mut self, pal: bool) {
        self.video_status = if pal { 2 } else { 1 };
        self.clock_hz = crate::sched::cpu_clock_hz(pal);
        self.poll_countdown = self.poll_interval();
    }

    pub fn reset(&mut self) {
        let (version, video_status, disc_type_code, disc_present, keyboard_attached, clock_hz) = (
            self.version,
            self.video_status,
            self.disc_type_code,
            self.disc_present,
            self.keyboard_attached,
            self.clock_hz,
        );
        *self = Self {
            version,
            video_status,
            disc_type_code,
            disc_present,
            keyboard_attached,
            clock_hz,
            poll_countdown: clock_hz / 60,
            ..Self::new("", true)
        };
    }

    /// Current IN2 line state.
    pub fn irq(&self) -> bool {
        self.irq_asserted
    }

    fn assert_channel_irq(&mut self, channel: usize) {
        self.irq_pending_channels |= 1 << channel;
        self.irq_asserted = true;
    }

    fn clear_channel_irq(&mut self, channel: usize) {
        self.irq_pending_channels &= !(1 << channel);
        self.irq_asserted = self.irq_pending_channels != 0;
    }

    pub fn diagnostic_snapshot(&self) -> SlaveDiagnosticSnapshot {
        SlaveDiagnosticSnapshot {
            irq_asserted: self.irq_asserted,
            polling_active: self.polling_active,
            pointer_interrupt_enabled: self.pointer_interrupt_enable,
            xbus_interrupt_enabled: self.xbus_interrupt_enable,
            input_x: self.input_x,
            input_y: self.input_y,
            device_x: self.device_x,
            device_y: self.device_y,
            buttons: self.input_buttons,
            disc_boot_mode: self.disc_boot_mode,
            disc_present: self.disc_present,
            disc_type_code: self.disc_type_code,
            modeled_servo_b0_packet: self.drive_status_packet(),
            next_forwarded_servo_packet: self.pending_drive_status.front().copied(),
            bios_visible_drive_packet: self.bios_visible_drive_packet(),
            pending_drive_status: self.pending_drive_status.len(),
            boot_status: self.boot_status,
            video_status: self.video_status,
            host_reset_pending: self.host_reset_requested,
        }
    }

    /// Latest audio-attenuation payload (L→L, L→R, R→R, R→L), if updated.
    pub fn take_attenuation(&mut self) -> Option<u32> {
        self.attenuation.take()
    }

    /// Consume a host-reset request raised by ch2 command 0x8A.
    pub fn take_host_reset_request(&mut self) -> bool {
        std::mem::take(&mut self.host_reset_requested)
    }

    /// Set the drive-status disc type from the inserted medium's volume
    /// descriptor. VMPEG's native `vcd` module requires the XA Bridge type
    /// before enabling the cartridge's White Book 13.5 MHz output converter.
    pub fn set_cd_rom_xa_bridge(&mut self, bridge: bool) {
        self.disc_type_code = if bridge {
            DISC_TYPE_CD_ROM_XA_BRIDGE
        } else {
            DISC_TYPE_CDI
        };
    }

    /// Set the medium present at power-on/reset without generating a hotplug
    /// notification. Use [`SlaveHle::notify_disc_change`] for a live drive.
    pub fn set_disc_present(&mut self, present: bool) {
        self.disc_present = present;
        self.disc_boot_mode = false;
        self.pending_drive_status.clear();
    }

    /// Forward a live SERVO media transition to the host's X-Bus channel.
    ///
    /// Replacing one mounted image represents the physical remove/insert
    /// sequence as two packets even when the frontend operation is atomic.
    /// Delivery waits until the BIOS enables ch3 command `FA` and until the
    /// previous ch3 response has been consumed.
    pub fn notify_disc_change(&mut self, present: bool, replacing: bool) {
        self.disc_boot_mode = false;
        if replacing {
            self.pending_drive_status
                .push_back([0xB0, 0x00, 0x00, 0x15]);
        }
        self.disc_present = present;
        self.pending_drive_status
            .push_back(self.drive_status_packet());
        self.service_drive_status_notification();
    }

    fn drive_status_packet(&self) -> [u8; 4] {
        let flags = if self.disc_present {
            self.disc_type_code | if self.disc_boot_mode { 0x40 } else { 0x00 }
        } else {
            0
        };
        [0xB0, 0x00, flags, 0x15]
    }

    fn bios_visible_drive_packet(&self) -> Option<[u8; 4]> {
        let channel = &self.channels[3];
        (channel.count != 0 && matches!(channel.cmd, 0xB0 | 0xB1)).then_some(channel.buf)
    }

    fn service_drive_status_notification(&mut self) {
        if !self.xbus_interrupt_enable
            || self.channels[3].count != 0
            || self.irq_countdowns[3].is_some()
        {
            return;
        }
        if let Some(packet) = self.pending_drive_status.pop_front() {
            self.prepare_readback(Some(self.readback_delay()), 3, 4, packet, 0xB0);
        }
    }

    /// Frontend input: absolute pointer position (0..767, 0..559) + buttons.
    pub fn set_pointer(&mut self, x: i32, y: i32, buttons: u8) {
        self.input_x = x;
        self.input_y = y;
        self.input_buttons = buttons;
    }

    /// Frontend input: relative pointer motion + current button state. The
    /// deltas are bounded only by the device clamp, so a host-side coordinate
    /// wall can never pin the pointer short of the screen edge — even after a
    /// title has reprogrammed the pointer position.
    pub fn move_pointer(&mut self, dx: i32, dy: i32, buttons: u8) {
        self.pending_dx += dx;
        self.pending_dy += dy;
        self.input_buttons = buttons;
    }

    /// Place the emulated pointer at an absolute device coordinate and
    /// anchor subsequent relative host motion there.
    pub fn set_pointer_absolute(&mut self, x: i32, y: i32, buttons: u8) {
        self.input_x = x;
        self.input_y = y;
        self.input_buttons = buttons;
        self.last_x = x;
        self.last_y = y;
        self.last_buttons = buttons;
        self.pending_dx = 0;
        self.pending_dy = 0;
        self.device_x = x.clamp(0, 767);
        self.device_y = y.clamp(0, 559);
    }

    /// Set one physical player-control key. The native `/pck2` driver opens
    /// and gates these channel-1 events; no guest event is synthesized while
    /// that path is disabled.
    pub fn set_player_control_key(&mut self, key: PlayerControlKey, pressed: bool) {
        let bit = key.slave_mask();
        let next = if pressed {
            self.player_control_mask | bit
        } else {
            self.player_control_mask & !bit
        };
        if next == self.player_control_mask {
            return;
        }
        self.player_control_mask = next;
        if self.player_control_enabled {
            self.pending_player_control.push_back([0x87, next]);
            self.service_player_control();
        }
    }

    /// Attach or remove a K-mode keyboard on the SLAVE-managed input port.
    /// Attachment is persistent hardware state; the event gate and queued
    /// key state are volatile firmware state.
    pub fn set_keyboard_attached(&mut self, attached: bool) {
        if self.keyboard_attached == attached {
            return;
        }
        self.keyboard_attached = attached;
        self.keyboard_events_enabled = false;
        self.keyboard_pressed_keys.clear();
        self.keyboard_modifiers = KeyboardModifiers::default();
        self.pending_keyboard.clear();
        if self.channels[2].cmd == 0xA0 {
            self.channels[2] = OutChannel::default();
            self.clear_channel_irq(2);
        }
    }

    pub fn keyboard_attached(&self) -> bool {
        self.keyboard_attached
    }

    /// Feed a physical keyboard transition into the Mono-I SLAVE input path.
    /// The native `/kb1` driver consumes channel-C `A0` plus the keyboard's
    /// documented two-byte K-mode state packet.
    pub fn set_keyboard_key(&mut self, code: u8, pressed: bool, modifiers: KeyboardModifiers) {
        if !self.keyboard_attached {
            return;
        }
        let previous = self.keyboard_packet();
        if pressed {
            if let Some(index) = self
                .keyboard_pressed_keys
                .iter()
                .position(|held| *held == code)
            {
                self.keyboard_pressed_keys.remove(index);
            }
            self.keyboard_pressed_keys.push(code);
        } else if let Some(index) = self
            .keyboard_pressed_keys
            .iter()
            .position(|held| *held == code)
        {
            self.keyboard_pressed_keys.remove(index);
        }
        self.keyboard_modifiers = modifiers;
        let packet = self.keyboard_packet();
        if packet == previous || !self.keyboard_events_enabled {
            return;
        }
        self.pending_keyboard.push_back(packet);
        self.service_keyboard();
    }

    pub fn set_keyboard_modifiers(&mut self, modifiers: KeyboardModifiers) {
        if !self.keyboard_attached || modifiers == self.keyboard_modifiers {
            return;
        }
        self.keyboard_modifiers = modifiers;
        if self.keyboard_events_enabled {
            self.pending_keyboard.push_back(self.keyboard_packet());
            self.service_keyboard();
        }
    }

    fn keyboard_packet(&self) -> [u8; 3] {
        let key = self.keyboard_pressed_keys.last().copied();
        let extension = if key.is_some() { 0 } else { 0x02 };
        let key = key.unwrap_or(0);
        [
            0xA0,
            0x80 | (self.keyboard_modifiers.packet_bits() << 3) | extension | ((key >> 7) & 1),
            key & 0x7F,
        ]
    }

    fn service_player_control(&mut self) {
        if !self.player_control_enabled || self.channels[1].count != 0 {
            return;
        }
        let Some(packet) = self.pending_player_control.pop_front() else {
            return;
        };
        let channel = &mut self.channels[1];
        channel.buf = [packet[0], packet[1], 0, 0];
        channel.index = 0;
        channel.count = 2;
        channel.cmd = 0x87;
        self.assert_channel_irq(1);
    }

    fn service_keyboard(&mut self) {
        if !self.keyboard_events_enabled
            || self.channels[2].count != 0
            || !self.pending_channel2_readbacks.is_empty()
        {
            return;
        }
        let Some(packet) = self.pending_keyboard.pop_front() else {
            return;
        };
        let channel = &mut self.channels[2];
        channel.buf = [packet[0], packet[1], packet[2], 0];
        channel.index = 0;
        channel.count = 3;
        channel.cmd = 0xA0;
        self.assert_channel_irq(2);
    }

    fn queue_channel2_readback(
        &mut self,
        delay: Option<u64>,
        count: usize,
        data: [u8; 4],
        cmd: u8,
    ) {
        self.pending_channel2_readbacks.push_back(PendingReadback {
            delay,
            count,
            data,
            cmd,
        });
        self.service_channel2();
    }

    fn service_channel2(&mut self) {
        if self.channels[2].count != 0 || self.irq_countdowns[2].is_some() {
            return;
        }
        if let Some(readback) = self.pending_channel2_readbacks.pop_front() {
            self.prepare_readback(
                readback.delay,
                2,
                readback.count,
                readback.data,
                readback.cmd,
            );
        } else {
            self.service_keyboard();
        }
    }

    fn prepare_readback(
        &mut self,
        delay: Option<u64>,
        channel: usize,
        count: usize,
        data: [u8; 4],
        cmd: u8,
    ) {
        let ch = &mut self.channels[channel];
        ch.index = 0;
        ch.count = count;
        ch.buf = data;
        ch.cmd = cmd;
        self.irq_countdowns[channel] = delay;
    }

    /// Advance time; fires delayed response IRQs and input polling.
    pub fn tick(&mut self, cycles: u64) {
        self.service_drive_status_notification();
        for channel in 0..self.irq_countdowns.len() {
            if let Some(remaining) = self.irq_countdowns[channel] {
                if remaining <= cycles {
                    self.irq_countdowns[channel] = None;
                    self.assert_channel_irq(channel);
                } else {
                    self.irq_countdowns[channel] = Some(remaining - cycles);
                }
            }
        }
        self.poll_countdown = self.poll_countdown.saturating_sub(cycles);
        if self.poll_countdown == 0 {
            self.poll_countdown = self.poll_interval();
            self.poll_inputs();
        }
    }

    /// CPU clocks until the next delayed response or pointer poll boundary.
    pub(crate) fn cycles_until_next_event(&self) -> u64 {
        self.irq_countdowns
            .iter()
            .flatten()
            .copied()
            .min()
            .unwrap_or(u64::MAX)
            .min(self.poll_countdown)
            .max(1)
    }

    fn poll_inputs(&mut self) {
        let (x, y, btn) = (self.input_x, self.input_y, self.input_buttons);
        if self.last_x < 0 || self.last_y < 0 {
            // A relative device's first host position is only an anchor. It
            // must not move the CD-i pointer away from coordinates that the
            // BIOS/title has programmed through a ch0 0xC0..=0xFF packet.
            self.last_x = x;
            self.last_y = y;
            self.last_buttons = btn;
            return;
        }
        let delta_x = (x - self.last_x) + std::mem::take(&mut self.pending_dx);
        let delta_y = (y - self.last_y) + std::mem::take(&mut self.pending_dy);
        if delta_x == 0 && delta_y == 0 && btn == self.last_buttons {
            return;
        }
        let mut button_bits: u8 = 0x01;
        if btn & 1 != 0 {
            button_bits |= 0x02;
        }
        if btn & 2 != 0 {
            button_bits |= 0x04;
        }
        if btn & 4 != 0 {
            button_bits |= 0x06;
        }
        self.last_x = x;
        self.last_y = y;
        self.last_buttons = btn;
        self.device_x = (self.device_x + delta_x).clamp(0, 767);
        self.device_y = (self.device_y + delta_y).clamp(0, 559);

        if self.polling_active {
            let byte3 = (((self.device_x as u32 & 0x380) >> 7) as u8) | (button_bits << 3);
            let byte2 = (self.device_x & 0x7F) as u8;
            let byte1 = ((self.device_y as u32 & 0x380) >> 7) as u8;
            let byte0 = (self.device_y & 0x7F) as u8;
            // The firmware records the packet and sets its `$54.4` pending
            // bit even while `$63.4` masks host notification. Keep the most
            // recent packet queued, but only assert IN2 when ch0 command 0x83
            // has enabled pointer-event notification.
            let packet = [byte3, byte2, byte1, byte0];
            let (_, _, button_code) = pointer_packet_fields(packet);
            log::debug!(
                "slave: queue pointer packet raw={packet:02x?} device=({}, {}) button_code={button_code:#03x} host_buttons={btn:#04x}",
                self.device_x,
                self.device_y
            );
            self.prepare_readback(None, 0, 4, packet, 0xF7);
            if self.pointer_interrupt_enable {
                self.assert_channel_irq(0);
            }
        }
    }

    /// Read a byte from a channel's response queue.
    pub fn read(&mut self, channel: usize) -> u8 {
        if self.channels[channel].count == 0 {
            return 0xFF;
        }
        // Native drivers may poll and consume a prepared response before its
        // notification delay expires. That read acknowledges the response,
        // so its timer must not later assert IN2 for an empty channel.
        let polled_before_irq = self.irq_countdowns[channel].take().is_some();
        let ch = &mut self.channels[channel];
        if channel == 0 && ch.index == 0 && ch.count == 4 && ch.cmd == 0xF7 {
            let (x, y, button_code) = pointer_packet_fields(ch.buf);
            log::debug!(
                "slave: consume pointer packet raw={:02x?} device=({}, {}) button_code={button_code:#03x}",
                ch.buf,
                x,
                y
            );
        }
        let ret = ch.buf[ch.index.min(3)];
        log::trace!("slave: read ch{channel} -> {ret:#04x}");
        let clear_irq = ch.index == 0
            && (matches!(
                ch.cmd,
                0xA0 | 0xA2 | 0xB0 | 0xB1 | 0xF0 | 0xF3 | 0xF4 | 0xF7 | 0xF9 | 0xFB..=0xFE
            ) || (channel == 1 && ch.cmd == 0x87));
        let ch = &mut self.channels[channel];
        ch.index += 1;
        ch.count -= 1;
        if ch.count == 0 {
            ch.index = 0;
            ch.cmd = 0;
            ch.buf = [0; 4];
        }
        if clear_irq || polled_before_irq {
            self.clear_channel_irq(channel);
        }
        if channel == 1 {
            self.service_player_control();
        } else if channel == 2 {
            self.service_channel2();
        }
        ret
    }

    /// Write a byte to a channel.
    pub fn write(&mut self, channel: usize, data: u8) {
        log::trace!("slave: write ch{channel} <- {data:#04x}");
        if channel == 1 && self.in_index == 0 {
            match data {
                0x8D => {
                    self.player_control_enabled = true;
                    self.pending_player_control.clear();
                }
                0x8E => {
                    self.player_control_enabled = false;
                    self.pending_player_control.clear();
                    if self.channels[1].cmd == 0x87 {
                        self.channels[1] = OutChannel::default();
                        self.clear_channel_irq(1);
                    }
                }
                _ => log::trace!("slave: ch1 unknown register {data:#04x}"),
            }
            self.clear_input();
            return;
        }
        self.in_buf[self.in_index.min(16)] = data;
        self.in_index += 1;
        match channel {
            0 => self.write_mouse(data),
            1 => {
                if self.in_index > 1 && self.in_index == self.in_count {
                    if self.in_buf[0] == 0xF0 {
                        self.lcd_state.copy_from_slice(&self.in_buf[1..17]);
                    }
                    self.clear_input();
                }
            }
            2 => self.write_ch2(data),
            3 => self.write_ch3(data),
            _ => {}
        }
    }

    fn clear_input(&mut self) {
        self.in_buf = [0; 17];
        self.in_index = 0;
        self.in_count = 0;
    }

    fn write_mouse(&mut self, data: u8) {
        if self.in_buf[0] >= 0xC0 {
            if self.in_index == 1 {
                log::trace!("slave: ch0 update mouse position ({data:#04x})");
                self.in_count = 3;
            } else if self.in_index == self.in_count {
                let x = (((self.in_buf[1] as i32 & 0x70) << 3) | (self.in_buf[2] as i32 & 0x7F))
                    .min(767);
                let y = (((self.in_buf[1] as i32 & 0x0F) << 6) | (self.in_buf[0] as i32 & 0x3F))
                    .min(559);
                self.device_x = x;
                self.device_y = y;
                log::debug!(
                    "slave: absolute pointer command raw={:02x?} device=({}, {})",
                    &self.in_buf[..3],
                    x,
                    y
                );
                self.clear_input();
            }
        } else {
            // Firmware BRA table at $0624. Most entries control front-panel
            // or SERVO flags that do not yet need distinct HLE state. The
            // 0x87/0x88 pair is significant: bit 1 is reported by the F4
            // test-plug status query.
            match data {
                0x83 => {
                    self.pointer_interrupt_enable = true;
                    if self.channels[0].cmd == 0xF7 && self.channels[0].count != 0 {
                        self.assert_channel_irq(0);
                    }
                }
                0x84 => self.pointer_interrupt_enable = false,
                0x87 => self.boot_status &= !0x02,
                0x88 => self.boot_status |= 0x02,
                0x80..=0x8C => {}
                _ => log::trace!("slave: ch0 unknown register {data:#04x}"),
            }
            if self.in_index == 1 {
                self.in_index = 0;
            }
        }
    }

    fn write_ch2(&mut self, data: u8) {
        if self.transport_parameter_bytes != 0 {
            self.transport_parameter_bytes -= 1;
            if self.transport_parameter_bytes == 0 && data != 0xFF {
                self.transport_repeat_delay = data;
            }
            self.clear_input();
            return;
        }

        if self.in_index > 1 {
            if self.in_index == self.in_count {
                match self.in_buf[0] {
                    0xC0..=0xCF => {
                        self.attenuation = Some(u32::from_be_bytes([
                            self.in_buf[1],
                            self.in_buf[2],
                            self.in_buf[3],
                            self.in_buf[4],
                        ]));
                        self.in_index = 0;
                        self.in_count = 0;
                    }
                    0xF0 => {
                        self.in_buf[1..17].fill(0);
                        self.in_count = 17;
                    }
                    _ => self.clear_input(),
                }
            }
        } else {
            match data {
                0x80 => {
                    self.keyboard_events_enabled = true;
                    self.pending_keyboard.clear();
                    self.clear_input();
                }
                0x81 => {
                    self.keyboard_events_enabled = false;
                    self.pending_keyboard.clear();
                    if self.channels[2].cmd == 0xA0 {
                        self.channels[2] = OutChannel::default();
                        self.clear_channel_irq(2);
                    }
                    self.clear_input();
                }
                0x82 => {
                    log::trace!("slave: mute audio");
                    self.clear_input();
                }
                0x83 => {
                    log::trace!("slave: unmute audio");
                    self.clear_input();
                }
                0x8D => {
                    log::trace!("slave: ch2 enable response notifications");
                    self.clear_input();
                }
                0x88 => {
                    self.transport_adjust_enabled = !self.transport_adjust_enabled;
                    self.clear_input();
                }
                0x90 => {
                    if self.transport_adjust_enabled {
                        self.transport_parameter_bytes = 2;
                    }
                    self.clear_input();
                }
                0x84..=0x87 | 0x89 | 0x8B..=0x8C | 0x8E..=0x8F | 0x91..=0x93 => {
                    // Immediate flag/control commands decoded from the
                    // firmware. Their lower-level SERVO effects are not
                    // required for host transport yet.
                    log::trace!("slave: ch2 control {data:#04x}");
                    self.clear_input();
                }
                0x8A => {
                    // Firmware target $0604 jumps straight to $0F0C,
                    // which asserts the 68070 RESET output. SLAVE RAM is
                    // not reset, so boot_status and the rest of this HLE
                    // deliberately survive the Machine host reset.
                    log::debug!("slave: ch2 0x8a requests host reset");
                    self.clear_input();
                    self.disc_boot_mode = true;
                    self.host_reset_requested = true;
                }
                0xC0..=0xCF => self.in_count = 5,
                0xF0 => self.in_count = 17,
                _ => {
                    log::trace!("slave: ch2 unknown register {data:#04x}");
                    self.clear_input();
                }
            }
        }
    }

    fn write_ch3(&mut self, data: u8) {
        if self.in_index > 1 {
            if self.in_index == self.in_count {
                match self.in_buf[0] {
                    0xB0 => {
                        // Request Disc Status: door closed, no disc errors.
                        // Retained launch mode sets the driver's disc-base
                        // available bit, prompting its B1 follow-up query.
                        self.prepare_readback(
                            Some(self.clock_hz / 4),
                            3,
                            4,
                            self.drive_status_packet(),
                            0xB0,
                        );
                    }
                    0xB1 => {
                        self.prepare_readback(
                            Some(self.readback_delay()),
                            3,
                            4,
                            [0xB1, 0x00, 0x00, 0x00],
                            0xB1,
                        );
                    }
                    _ => {}
                }
                self.clear_input();
            }
        } else {
            match data {
                0xB0 | 0xB1 => self.in_count = 4,
                0xF0 => {
                    // Request SLAVE revision.
                    let [v0, v1] = self.version;
                    self.queue_channel2_readback(
                        Some(self.readback_delay()),
                        2,
                        [0xF0, v0, v1, 0],
                        0xF0,
                    );
                    self.in_index = 0;
                }
                0xF3 => {
                    // Request the configuration-device class derived from
                    // pointing-port recognition. Class 5 adds `/kb1` to the
                    // ordinary class-C relative-pointer configuration.
                    let device_class = if self.keyboard_attached { 5 } else { 1 };
                    self.queue_channel2_readback(
                        Some(self.readback_delay()),
                        2,
                        [0xF3, device_class, 0xFF, 0],
                        0xF3,
                    );
                    self.in_index = 0;
                }
                0xF4 => {
                    // Request persistent boot/test-plug status. Firmware
                    // forms this byte from $55 bits 0-1; bit 1 is set by
                    // ch0 0x88 during the PLAY CD-I launch flow.
                    let status = self.boot_status;
                    self.queue_channel2_readback(
                        Some(self.readback_delay()),
                        2,
                        [0xF4, status, 0, 0],
                        0xF4,
                    );
                    self.in_index = 0;
                }
                0xF6 => {
                    // Request NTSC/PAL status; response without an IRQ.
                    let vs = self.video_status;
                    self.queue_channel2_readback(None, 2, [0xF6, vs, 0, 0], 0xF6);
                    self.in_index = 0;
                }
                0xF7 => {
                    self.polling_active = true;
                    self.in_index = 0;
                }
                0xF8 => {
                    // Firmware sets $63.6 here; this is unrelated to pointer
                    // polling. Alien Gate sends F8 while its pointer is live.
                    self.in_index = 0;
                }
                0xFE => {
                    // Firmware sets $58.3, but this is not the HLE pointer
                    // delivery gate. Titles such as Alien Gate send FE when
                    // taking over from the player shell and continue to use
                    // the same pointer channel afterwards. F7 starts the HLE
                    // poller; it remains active until reset, as in MAME.
                    self.in_index = 0;
                }
                0xFA => {
                    self.xbus_interrupt_enable = true;
                    self.in_index = 0;
                    self.service_drive_status_notification();
                }
                0xF9 => {
                    // Firmware clears the host-ready gate paired with F8.
                    self.in_index = 0;
                }
                0xFB => {
                    self.xbus_interrupt_enable = false;
                    self.in_index = 0;
                }
                0xFC | 0xFD => {
                    // Firmware drives/releases Port C bit 6. Neither command
                    // returns a channel-2 echo response.
                    self.in_index = 0;
                }
                _ => {
                    log::trace!("slave: ch3 unknown register {data:#04x}");
                    self.clear_input();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ntsc_host_clock_scales_fixed_slave_delays() {
        let mut slave = SlaveHle::new("3231", true);
        assert_eq!(slave.readback_delay(), 1_500);
        assert_eq!(slave.poll_interval(), 250_000);

        slave.set_video_standard(false);
        assert_eq!(slave.readback_delay(), 1_510);
        assert_eq!(slave.poll_interval(), 251_748);
        slave.reset();
        assert_eq!(slave.readback_delay(), 1_510);
        assert_eq!(slave.poll_countdown, 251_748);
    }

    #[test]
    fn revision_query_responds_on_channel_2() {
        let mut s = SlaveHle::new("3231", true);
        s.write(3, 0xF0);
        assert!(!s.irq());
        s.tick(READBACK_DELAY);
        assert!(s.irq());
        // First read de-asserts the IRQ and returns the echoed command.
        assert_eq!(s.read(2), 0xF0);
        assert!(!s.irq());
        assert_eq!(s.read(2), 0x32);
        // Queue exhausted afterwards.
        assert_eq!(s.read(2), 0xFF);
    }

    #[test]
    fn pal_query_no_irq() {
        let mut s = SlaveHle::new("3231", true);
        s.write(3, 0xF6);
        s.tick(1_000_000);
        assert!(!s.irq());
        assert_eq!(s.read(2), 0xF6);
        assert_eq!(s.read(2), 2);
    }

    #[test]
    fn f3_advertises_the_attached_keyboard_class() {
        let mut slave = SlaveHle::new("3231", true);
        slave.write(3, 0xF3);
        slave.tick(READBACK_DELAY);
        assert_eq!([slave.read(2), slave.read(2)], [0xF3, 1]);

        slave.set_keyboard_attached(true);
        slave.reset();
        slave.write(3, 0xF3);
        slave.tick(READBACK_DELAY);
        assert_eq!([slave.read(2), slave.read(2)], [0xF3, 5]);
    }

    #[test]
    fn back_to_back_status_queries_do_not_overwrite_keyboard_discovery() {
        let mut slave = SlaveHle::new("3231", true);
        slave.set_keyboard_attached(true);
        slave.write(3, 0xF3);
        slave.write(3, 0xF4);

        slave.tick(READBACK_DELAY);
        assert_eq!([slave.read(2), slave.read(2)], [0xF3, 5]);
        assert!(!slave.irq());

        slave.tick(READBACK_DELAY);
        assert_eq!([slave.read(2), slave.read(2)], [0xF4, 0]);
    }

    #[test]
    fn polling_a_delayed_reply_cancels_its_stale_interrupt() {
        let mut slave = SlaveHle::new("3231", true);
        slave.prepare_readback(Some(READBACK_DELAY), 3, 4, [0xB0, 0x00, 0x00, 0x00], 0xB0);

        assert_eq!(
            [slave.read(3), slave.read(3), slave.read(3), slave.read(3)],
            [0xB0, 0x00, 0x00, 0x00]
        );
        slave.tick(READBACK_DELAY);
        assert!(!slave.irq());
    }

    #[test]
    fn channel_d_control_commands_do_not_synthesize_query_replies() {
        let mut slave = SlaveHle::new("3231", true);
        for command in [0xF9, 0xFB, 0xFC, 0xFD] {
            slave.write(3, command);
            assert_eq!(slave.read(2), 0xFF, "command {command:#04x}");
        }
    }

    #[test]
    fn native_keyboard_gate_delivers_press_and_release_packets() {
        let mut slave = SlaveHle::new("3231", true);
        slave.set_keyboard_attached(true);
        let modifiers = crate::keyboard::KeyboardModifiers {
            shift: true,
            control: true,
            ..crate::keyboard::KeyboardModifiers::default()
        };

        slave.set_keyboard_key(b'A', true, modifiers);
        assert_eq!(slave.read(2), 0xFF, "disabled device stays silent");

        slave.write(2, 0x80);
        slave.set_keyboard_key(b'A', false, Default::default());
        slave.set_keyboard_key(b'A', true, modifiers);
        assert!(slave.irq());
        assert_eq!(
            [slave.read(2), slave.read(2), slave.read(2)],
            [0xA0, 0x82, 0x00]
        );
        assert!(slave.irq(), "the queued press reasserts IN2");
        assert_eq!(
            [slave.read(2), slave.read(2), slave.read(2)],
            [0xA0, 0xC8, b'A']
        );
    }

    #[test]
    fn disabling_keyboard_events_discards_pending_packets() {
        let mut slave = SlaveHle::new("3231", true);
        slave.set_keyboard_attached(true);
        slave.write(2, 0x80);
        slave.set_keyboard_key(b'A', true, Default::default());
        slave.write(2, 0x81);
        assert!(!slave.irq());
        assert_eq!(slave.read(2), 0xFF);
    }

    #[test]
    fn video_standard_can_switch_to_ntsc_and_survives_reset() {
        let mut s = SlaveHle::new("3231", true);
        s.set_video_standard(false);
        s.reset();
        s.write(3, 0xF6);
        s.tick(READBACK_DELAY);
        assert_eq!(s.read(2), 0xF6);
        assert_eq!(s.read(2), 1);
    }

    #[test]
    fn empty_channel_reads_ff() {
        let mut s = SlaveHle::new("3231", true);
        assert_eq!(s.read(0), 0xFF);
    }

    #[test]
    fn native_player_control_gate_delivers_press_and_release_packets() {
        let mut slave = SlaveHle::new("3231", true);
        slave.set_player_control_key(PlayerControlKey::Pause, true);
        assert_eq!(slave.read(1), 0xFF, "disabled device stays silent");

        slave.write(1, 0x8D);
        slave.set_player_control_key(PlayerControlKey::Pause, false);
        slave.set_player_control_key(PlayerControlKey::Pause, true);
        assert!(slave.irq());
        assert_eq!([slave.read(1), slave.read(1)], [0x87, 0x00]);
        assert!(slave.irq(), "the queued press reasserts IN2");
        assert_eq!([slave.read(1), slave.read(1)], [0x87, 0x10]);
    }

    #[test]
    fn player_control_raw_masks_match_the_bundled_pck2_driver() {
        let mut slave = SlaveHle::new("3231", true);
        slave.write(1, 0x8D);
        for (key, mask) in [
            (PlayerControlKey::Play, 0x20),
            (PlayerControlKey::Stop, 0x04),
            (PlayerControlKey::Pause, 0x10),
            (PlayerControlKey::Next, 0x01),
            (PlayerControlKey::Previous, 0x02),
        ] {
            slave.set_player_control_key(key, true);
            assert_eq!([slave.read(1), slave.read(1)], [0x87, mask]);
            slave.set_player_control_key(key, false);
            assert_eq!([slave.read(1), slave.read(1)], [0x87, 0x00]);
        }
    }

    #[test]
    fn disabling_player_controls_discards_pending_events() {
        let mut slave = SlaveHle::new("3231", true);
        slave.write(1, 0x8D);
        slave.set_player_control_key(PlayerControlKey::Play, true);
        slave.write(1, 0x8E);
        assert!(!slave.irq());
        assert_eq!(slave.read(1), 0xFF);
    }

    #[test]
    fn title_mode_commands_keep_pointer_polling_active() {
        let mut s = SlaveHle::new("3231", true);
        s.write(0, 0x83);
        s.write(3, 0xF7);
        s.set_pointer(100, 100, 0);
        s.tick(POLL_INTERVAL);
        assert_eq!(s.read(0), 0xFF, "the first relative sample is an anchor");
        s.set_pointer(101, 100, 0);
        s.tick(POLL_INTERVAL);
        for _ in 0..3 {
            assert_ne!(s.read(0), 0xFF);
        }
        assert_ne!(s.read(0), 0xFF);

        s.write(3, 0xF8);
        s.set_pointer(110, 105, 0);
        s.tick(POLL_INTERVAL);
        assert!(s.irq());
        assert_ne!(s.read(0), 0xFF);
        for _ in 0..3 {
            s.read(0);
        }

        s.write(3, 0xFE);
        s.set_pointer(120, 110, 0);
        s.tick(POLL_INTERVAL);
        assert!(s.irq());
        assert_ne!(s.read(0), 0xFF);
    }

    #[test]
    fn pointer_packet_sets_only_real_mouse_buttons() {
        let mut s = SlaveHle::new("3231", true);
        s.write(0, 0x83);
        s.write(3, 0xF7);
        s.set_pointer(90, 90, 0);
        s.tick(POLL_INTERVAL);
        s.set_pointer(100, 100, 1);
        s.tick(POLL_INTERVAL);
        assert_eq!(s.read(0) & 0x38, 0x18);

        for _ in 0..3 {
            s.read(0);
        }
        s.set_pointer(100, 100, 2);
        s.tick(POLL_INTERVAL);
        assert_eq!(s.read(0) & 0x38, 0x28);
    }

    #[test]
    fn pointer_notification_gate_retains_the_latest_packet() {
        let mut s = SlaveHle::new("3231", true);
        s.write(3, 0xF7);
        s.set_pointer(100, 100, 0);
        s.tick(POLL_INTERVAL);
        s.set_pointer(110, 110, 0);
        s.tick(POLL_INTERVAL);
        assert!(!s.irq(), "F7 polling alone must not notify the host");

        s.write(0, 0x83);
        assert!(s.irq(), "0x83 exposes the retained pointer event");
        for _ in 0..4 {
            assert_ne!(s.read(0), 0xFF);
        }

        s.write(0, 0x84);
        s.set_pointer(200, 200, 0);
        s.tick(POLL_INTERVAL);
        assert!(!s.irq(), "0x84 masks later pointer events");

        s.write(0, 0x83);
        assert!(s.irq(), "re-enabling exposes the latest retained event");
    }

    #[test]
    fn relative_motion_reaches_the_full_device_range_after_a_title_warp() {
        let mut s = SlaveHle::new("3231", true);
        s.write(3, 0xF7);
        // Anchor the absolute host channel near the right edge, then have the
        // title warp the pointer to the center: a host-side coordinate wall
        // would now stop relative motion short of the device bounds.
        s.set_pointer(700, 500, 0);
        s.tick(POLL_INTERVAL);
        s.device_x = 384;
        s.device_y = 280;

        s.move_pointer(500, 300, 0);
        s.tick(POLL_INTERVAL);
        assert_eq!((s.device_x, s.device_y), (767, 559));

        s.move_pointer(-1000, -600, 0);
        s.tick(POLL_INTERVAL);
        assert_eq!((s.device_x, s.device_y), (0, 0));
    }

    #[test]
    fn relative_input_preserves_title_programmed_pointer_position() {
        let mut s = SlaveHle::new("3231", true);
        s.device_x = 400;
        s.device_y = 300;
        s.set_pointer(100, 100, 0);
        s.tick(POLL_INTERVAL);
        assert_eq!((s.device_x, s.device_y), (400, 300));

        s.set_pointer(106, 97, 0);
        s.tick(POLL_INTERVAL);
        assert_eq!((s.device_x, s.device_y), (406, 297));
    }

    #[test]
    fn disc_boot_status_and_host_reset_latch_survive_host_handshake() {
        let mut s = SlaveHle::new("3231", true);
        s.write(0, 0x88);
        s.write(2, 0x8A);
        assert!(s.take_host_reset_request());
        assert!(!s.take_host_reset_request());

        s.write(3, 0xF4);
        s.tick(READBACK_DELAY);
        assert_eq!(s.read(2), 0xF4);
        assert_eq!(s.read(2), 0x02);
    }

    #[test]
    fn retained_disc_boot_changes_drive_status_and_answers_disc_base() {
        let mut s = SlaveHle::new("3231", true);
        s.set_disc_present(true);
        s.write(2, 0x8A);
        assert!(s.take_host_reset_request());

        for byte in [0xB0, 0, 0, 0] {
            s.write(3, byte);
        }
        s.tick(15_000_000 / 4);
        assert!(s.irq());
        assert_eq!(
            [s.read(3), s.read(3), s.read(3), s.read(3)],
            [0xB0, 0x00, 0x42, 0x15]
        );

        for byte in [0xB1, 0, 0, 0] {
            s.write(3, byte);
        }
        s.tick(READBACK_DELAY);
        assert_eq!(
            [s.read(3), s.read(3), s.read(3), s.read(3)],
            [0xB1, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn diagnostics_distinguish_modeled_b0_from_queued_forwarding() {
        let mut s = SlaveHle::new("3231", true);
        s.set_disc_present(true);
        let ordinary = s.diagnostic_snapshot();
        assert_eq!(ordinary.modeled_servo_b0_packet, [0xB0, 0x00, 0x02, 0x15]);
        assert_eq!(ordinary.next_forwarded_servo_packet, None);
        assert_eq!(ordinary.bios_visible_drive_packet, None);

        s.write(2, 0x8A);
        let retained = s.diagnostic_snapshot();
        assert_eq!(retained.modeled_servo_b0_packet, [0xB0, 0x00, 0x42, 0x15]);
        assert_eq!(retained.next_forwarded_servo_packet, None);
        assert_eq!(retained.bios_visible_drive_packet, None);

        // With X-Bus notifications disabled, an atomic image replacement
        // retains the physical remove packet ahead of the insert packet.
        s.notify_disc_change(true, true);
        let replacement = s.diagnostic_snapshot();
        assert_eq!(
            replacement.modeled_servo_b0_packet,
            [0xB0, 0x00, 0x02, 0x15]
        );
        assert_eq!(
            replacement.next_forwarded_servo_packet,
            Some([0xB0, 0x00, 0x00, 0x15])
        );
        assert_eq!(replacement.pending_drive_status, 2);
        assert_eq!(replacement.bios_visible_drive_packet, None);
    }

    #[test]
    fn diagnostic_transition_matrix_exposes_the_bios_packet_boundary() {
        const EMPTY: [u8; 4] = [0xB0, 0x00, 0x00, 0x15];
        const CDI: [u8; 4] = [0xB0, 0x00, 0x02, 0x15];
        const RETAINED_CDI: [u8; 4] = [0xB0, 0x00, 0x42, 0x15];
        const DISC_BASE: [u8; 4] = [0xB1, 0x00, 0x00, 0x00];

        let read_packet =
            |slave: &mut SlaveHle| [slave.read(3), slave.read(3), slave.read(3), slave.read(3)];
        let request_packet = |slave: &mut SlaveHle, command: u8, delay: u64| {
            for byte in [command, 0, 0, 0] {
                slave.write(3, byte);
            }
            assert_eq!(
                slave.diagnostic_snapshot().bios_visible_drive_packet,
                Some(if command == 0xB0 {
                    slave.drive_status_packet()
                } else {
                    DISC_BASE
                })
            );
            slave.tick(delay);
            read_packet(slave)
        };

        // Power-on: explicit status queries distinguish an empty drive from
        // already-mounted media without fabricating an unsolicited event.
        let mut slave = SlaveHle::new("3231", true);
        assert_eq!(request_packet(&mut slave, 0xB0, 15_000_000 / 4), EMPTY);
        slave.set_disc_present(true);
        assert_eq!(request_packet(&mut slave, 0xB0, 15_000_000 / 4), CDI);

        // Live insertion and removal become visible only after FA enables
        // forwarding. The diagnostic boundary reports the exact packet that
        // the BIOS can consume, not merely the settled modeled state.
        let mut slave = SlaveHle::new("3231", true);
        slave.write(3, 0xFA);
        slave.notify_disc_change(true, false);
        assert_eq!(
            slave.diagnostic_snapshot().bios_visible_drive_packet,
            Some(CDI)
        );
        slave.tick(READBACK_DELAY);
        assert_eq!(read_packet(&mut slave), CDI);
        slave.notify_disc_change(false, false);
        assert_eq!(
            slave.diagnostic_snapshot().bios_visible_drive_packet,
            Some(EMPTY)
        );
        slave.tick(READBACK_DELAY);
        assert_eq!(read_packet(&mut slave), EMPTY);

        // Atomic replacement preserves the physical ordering assumed by the
        // HLE: removal first, then insertion after the first packet drains.
        slave.set_disc_present(true);
        slave.notify_disc_change(true, true);
        assert_eq!(
            slave.diagnostic_snapshot().bios_visible_drive_packet,
            Some(EMPTY)
        );
        slave.tick(READBACK_DELAY);
        assert_eq!(read_packet(&mut slave), EMPTY);
        slave.tick(READBACK_DELAY);
        assert_eq!(
            slave.diagnostic_snapshot().bios_visible_drive_packet,
            Some(CDI)
        );
        assert_eq!(read_packet(&mut slave), CDI);

        // PLAY requests only a host reset. The retained SLAVE state is then
        // visible to the restarted BIOS as B0 bit 6 followed by B1.
        slave.write(2, 0x8A);
        assert!(slave.take_host_reset_request());
        assert_eq!(
            request_packet(&mut slave, 0xB0, 15_000_000 / 4),
            RETAINED_CDI
        );
        assert_eq!(request_packet(&mut slave, 0xB1, READBACK_DELAY), DISC_BASE);
    }

    #[test]
    fn xa_bridge_disc_type_is_reported_and_survives_reset() {
        let mut s = SlaveHle::new("3231", true);
        s.set_cd_rom_xa_bridge(true);
        s.set_disc_present(true);
        s.reset();
        s.write(2, 0x8A);
        assert!(s.take_host_reset_request());

        for byte in [0xB0, 0, 0, 0] {
            s.write(3, byte);
        }
        s.tick(15_000_000 / 4);
        assert_eq!(
            [s.read(3), s.read(3), s.read(3), s.read(3)],
            [0xB0, 0x00, 0x44, 0x15]
        );
    }

    #[test]
    fn disc_status_distinguishes_an_empty_drive_from_inserted_media() {
        let mut s = SlaveHle::new("3231", true);
        for byte in [0xB0, 0, 0, 0] {
            s.write(3, byte);
        }
        s.tick(15_000_000 / 4);
        assert_eq!(
            [s.read(3), s.read(3), s.read(3), s.read(3)],
            [0xB0, 0x00, 0x00, 0x15]
        );

        s.set_disc_present(true);
        for byte in [0xB0, 0, 0, 0] {
            s.write(3, byte);
        }
        s.tick(15_000_000 / 4);
        assert_eq!(
            [s.read(3), s.read(3), s.read(3), s.read(3)],
            [0xB0, 0x00, 0x02, 0x15]
        );
    }

    #[test]
    fn live_media_change_waits_for_xbus_enable() {
        let mut s = SlaveHle::new("3231", true);
        s.notify_disc_change(true, false);
        s.tick(READBACK_DELAY);
        assert!(!s.irq());
        assert_eq!(s.read(3), 0xFF);

        s.write(3, 0xFA);
        s.tick(READBACK_DELAY);
        assert!(s.irq());
        assert_eq!(
            [s.read(3), s.read(3), s.read(3), s.read(3)],
            [0xB0, 0x00, 0x02, 0x15]
        );
    }

    #[test]
    fn live_disc_swap_reports_removal_then_insertion() {
        let mut s = SlaveHle::new("3231", true);
        s.set_disc_present(true);
        s.write(3, 0xFA);
        s.notify_disc_change(true, true);

        s.tick(READBACK_DELAY);
        assert_eq!(
            [s.read(3), s.read(3), s.read(3), s.read(3)],
            [0xB0, 0x00, 0x00, 0x15]
        );
        s.tick(READBACK_DELAY);
        assert_eq!(
            [s.read(3), s.read(3), s.read(3), s.read(3)],
            [0xB0, 0x00, 0x02, 0x15]
        );
    }

    #[test]
    fn ch2_90_accepts_two_parameters_only_in_transport_adjust_mode() {
        let mut s = SlaveHle::new("3231", true);

        s.write(2, 0x90);
        s.write(2, 0xFF);
        s.write(2, 0x17);
        assert_eq!(s.transport_repeat_delay, 0x0A);

        s.write(2, 0x88);
        s.write(2, 0x90);
        s.write(2, 0xFF);
        s.write(2, 0x17);
        assert_eq!(s.transport_repeat_delay, 0x17);

        s.write(2, 0x90);
        s.write(2, 0x00);
        s.write(2, 0xFF);
        assert_eq!(s.transport_repeat_delay, 0x17);
    }
}
