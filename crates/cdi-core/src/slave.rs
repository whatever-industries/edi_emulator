// SPDX-License-Identifier: GPL-2.0-or-later
//! CD-i Mono-I SLAVE MCU high-level emulation.
//!
//! Ported from MAME `src/mame/philips/cdislavehle.cpp` (BSD-3-Clause,
//! Ryan Holtz) — see NOTICE.md. The SLAVE is an MC68HC705 handling input
//! devices, audio attenuation, the front panel, and player status; the CPU
//! talks to it over four byte channels at `$310000` (odd bytes). Responses
//! are delivered after a delay and raise IN2 (autovector 26).

/// CPU cycles (15 MHz domain) per microsecond.
const CYCLES_PER_US: u64 = 15;
/// Response latency used by MAME for most queries (100 µs).
const READBACK_DELAY: u64 = 100 * CYCLES_PER_US;
/// Input poll cadence (60 Hz).
const POLL_INTERVAL: u64 = 15_000_000 / 60;

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
pub struct SlaveHle {
    channels: [OutChannel; 4],
    in_buf: [u8; 17],
    in_index: usize,
    in_count: usize,
    polling_active: bool,
    /// Firmware `$63.4`: allow pending pointer events to notify the host.
    pointer_interrupt_enable: bool,
    xbus_interrupt_enable: bool,
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

    /// Latest audio-attenuation command payload for the CDIC, if pending.
    attenuation: Option<u32>,
    /// IRQ (IN2) line state.
    irq_asserted: bool,
    /// Cycles until the pending response raises the IRQ, if any.
    irq_countdown: Option<u64>,
    poll_countdown: u64,
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
            lcd_state: [0; 16],
            version,
            video_status: if pal { 2 } else { 1 },
            boot_status: 0,
            host_reset_requested: false,
            disc_boot_mode: false,
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
            attenuation: None,
            irq_asserted: false,
            irq_countdown: None,
            poll_countdown: POLL_INTERVAL,
        }
    }

    pub fn reset(&mut self) {
        let (version, video_status) = (self.version, self.video_status);
        *self = Self {
            version,
            video_status,
            ..Self::new("", true)
        };
    }

    /// Current IN2 line state.
    pub fn irq(&self) -> bool {
        self.irq_asserted
    }

    /// Latest audio-attenuation payload (L→L, L→R, R→R, R→L), if updated.
    pub fn take_attenuation(&mut self) -> Option<u32> {
        self.attenuation.take()
    }

    /// Consume a host-reset request raised by ch2 command 0x8A.
    pub fn take_host_reset_request(&mut self) -> bool {
        std::mem::take(&mut self.host_reset_requested)
    }

    /// Frontend input: absolute pointer position (0..767, 0..559) + buttons.
    pub fn set_pointer(&mut self, x: i32, y: i32, buttons: u8) {
        self.input_x = x;
        self.input_y = y;
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
        self.device_x = x.clamp(0, 767);
        self.device_y = y.clamp(0, 559);
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
        self.irq_countdown = delay;
    }

    /// Advance time; fires delayed response IRQs and input polling.
    pub fn tick(&mut self, cycles: u64) {
        if let Some(remaining) = self.irq_countdown {
            if remaining <= cycles {
                self.irq_asserted = true;
                self.irq_countdown = None;
            } else {
                self.irq_countdown = Some(remaining - cycles);
            }
        }
        self.poll_countdown = self.poll_countdown.saturating_sub(cycles);
        if self.poll_countdown == 0 {
            self.poll_countdown = POLL_INTERVAL;
            self.poll_inputs();
        }
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
        if x == self.last_x && y == self.last_y && btn == self.last_buttons {
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
        let delta_x = x - self.last_x;
        let delta_y = y - self.last_y;
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
            self.prepare_readback(None, 0, 4, [byte3, byte2, byte1, byte0], 0xF7);
            if self.pointer_interrupt_enable {
                self.irq_asserted = true;
            }
        }
    }

    /// Read a byte from a channel's response queue.
    pub fn read(&mut self, channel: usize) -> u8 {
        let ch = &mut self.channels[channel];
        if ch.count == 0 {
            return 0xFF;
        }
        let ret = ch.buf[ch.index.min(3)];
        log::trace!("slave: read ch{channel} -> {ret:#04x}");
        if ch.index == 0
            && matches!(
                ch.cmd,
                0xB0 | 0xB1 | 0xF0 | 0xF3 | 0xF4 | 0xF7 | 0xF9 | 0xFB..=0xFE
            )
        {
            self.irq_asserted = false;
        }
        let ch = &mut self.channels[channel];
        ch.index += 1;
        ch.count -= 1;
        if ch.count == 0 {
            ch.index = 0;
            ch.cmd = 0;
            ch.buf = [0; 4];
        }
        ret
    }

    /// Write a byte to a channel.
    pub fn write(&mut self, channel: usize, data: u8) {
        log::trace!("slave: write ch{channel} <- {data:#04x}");
        if channel == 1 && self.in_index == 0 {
            // Channel 1 single-byte writes: unknown/ignored.
            log::trace!("slave: ch1 unknown register {data:#04x}");
            self.in_buf = [0; 17];
            self.in_count = 0;
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
                self.device_x = (((self.in_buf[1] as i32 & 0x70) << 3)
                    | (self.in_buf[2] as i32 & 0x7F))
                    .min(767);
                self.device_y = (((self.in_buf[1] as i32 & 0x0F) << 6)
                    | (self.in_buf[0] as i32 & 0x3F))
                    .min(559);
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
                        self.irq_asserted = true;
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
                0x80..=0x87 | 0x89 | 0x8B..=0x8C | 0x8E..=0x8F | 0x91..=0x93 => {
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
                        let flags = if self.disc_boot_mode { 0x42 } else { 0x02 };
                        self.prepare_readback(
                            Some(15_000_000 / 4),
                            3,
                            4,
                            [0xB0, 0x00, flags, 0x15],
                            0xB0,
                        );
                    }
                    0xB1 => {
                        self.prepare_readback(
                            Some(READBACK_DELAY),
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
                    self.prepare_readback(Some(READBACK_DELAY), 2, 2, [0xF0, v0, v1, 0], 0xF0);
                    self.in_index = 0;
                }
                0xF3 => {
                    // Request pointer type (1 = relative).
                    self.prepare_readback(Some(READBACK_DELAY), 2, 2, [0xF3, 1, 0, 0], 0xF3);
                    self.in_index = 0;
                }
                0xF4 => {
                    // Request persistent boot/test-plug status. Firmware
                    // forms this byte from $55 bits 0-1; bit 1 is set by
                    // ch0 0x88 during the PLAY CD-I launch flow.
                    let status = self.boot_status;
                    self.prepare_readback(Some(READBACK_DELAY), 2, 2, [0xF4, status, 0, 0], 0xF4);
                    self.in_index = 0;
                }
                0xF6 => {
                    // Request NTSC/PAL status; response without an IRQ.
                    let vs = self.video_status;
                    self.prepare_readback(None, 2, 2, [0xF6, vs, 0, 0], 0xF6);
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
                }
                0xF9 | 0xFB..=0xFD => {
                    // Undocumented queries used by the disc-play flow;
                    // acknowledge with a polled (no-IRQ) echoed response so
                    // the driver's wait loop completes (protocol under
                    // investigation — see CODEX_HANDOVER.md).
                    log::debug!("slave: ch3 command {data:#04x} — echo-ack");
                    self.prepare_readback(None, 2, 2, [data, 0, 0, 0], data);
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
    fn empty_channel_reads_ff() {
        let mut s = SlaveHle::new("3231", true);
        assert_eq!(s.read(0), 0xFF);
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
