// SPDX-License-Identifier: GPL-3.0-or-later
//! CDIC (CD Interface Controller) high-level emulation.
//!
//! Ported from MAME `src/mame/philips/cdicdic.cpp` (BSD-3-Clause,
//! Ryan Holtz, Vincent Halver) — see NOTICE.md. The CDIC owns 16 KB of
//! buffer RAM at `$300000`, a register file at `$303C00`/`$303FF4`+, a
//! 75 Hz sector pump, XA-ADPCM audio decoding, and raises IN4 with its own
//! programmable vector.
//!
//! CD-fed audio receipt/playback and AUDCTL status semantics are corrected
//! from independent Mono-I hardware captures in `Slamy/CDIC_BlackBoxAnalyzer`
//! revision `e861f76`; see `docs/specification-research.md`.
//!
//! Decoded audio is pushed into `audio_out` as 44.1 kHz interleaved stereo
//! (naive rate conversion from 18.9/37.8 kHz XA rates); the frontend drains
//! it.

use cdi_disc::scramble::descramble_in_place;
use cdi_disc::DiscImage;

// Raw-sector byte offsets.
const SECTOR_SIZE: usize = 2352;
const SECTOR_HEADER: usize = 12;
const SECTOR_MINUTES: usize = 12;
const SECTOR_SECONDS: usize = 13;
const SECTOR_FRACS: usize = 14;
const SECTOR_MODE: usize = 15;
const SECTOR_FILE1: usize = 16;
const SECTOR_CHAN1: usize = 17;
const SECTOR_SUBMODE1: usize = 18;
const SECTOR_CODING1: usize = 19;
const SECTOR_FILE2: usize = 20;
const SECTOR_CHAN2: usize = 21;
const SECTOR_SUBMODE2: usize = 22;
const SECTOR_CODING2: usize = 23;
const SECTOR_DATA: usize = 24;
const SECTOR_AUDIO_SIZE: usize = 2304;

// Submode bits.
const SUBMODE_EOR: u8 = 0x01;
const SUBMODE_VIDEO: u8 = 0x02;
const SUBMODE_AUDIO: u8 = 0x04;
const SUBMODE_DATA: u8 = 0x08;
const SUBMODE_TRIG: u8 = 0x10;
const SUBMODE_FORM: u8 = 0x20;
const SUBMODE_EOF: u8 = 0x80;

// Coding byte fields.
const CODING_BPS_MASK: u8 = 0x30;
const CODING_4BPS: u8 = 0x00;
const CODING_8BPS: u8 = 0x10;
const CODING_16BPS: u8 = 0x20;
const CODING_BPS_MPEG: u8 = 0x30;
const CODING_RATE_MASK: u8 = 0x0C;
const CODING_37KHZ: u8 = 0x00;
const CODING_18KHZ: u8 = 0x04;
const CODING_RATE_RESV: u8 = 0x08;
const CODING_44KHZ: u8 = 0x0C;
const CODING_CHAN_MASK: u8 = 0x03;
const CODING_MONO: u8 = 0x00;
const CODING_STEREO: u8 = 0x01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
enum DiscMode {
    Idle,
    Mode1,
    Mode2,
    Cdda,
    Toc,
}

/// Read-only CDIC state used by deterministic compatibility diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct CdicDiagnosticSnapshot {
    pub command: u16,
    /// 0 idle, 1 Mode 1, 2 Mode 2, 3 CDDA, 4 TOC.
    pub disc_mode: u8,
    pub disc_read_enabled: bool,
    pub current_lba: u32,
    pub selected_file: u16,
    pub selected_channels: u32,
    pub audio_channel: u16,
    pub audio_buffer: u16,
    pub x_buffer: u16,
    pub z_buffer: u16,
    pub data_buffer: u16,
    pub interrupt_vector: u16,
    pub interrupt_asserted: bool,
    pub queued_audio_samples: usize,
}

/// XA-ADPCM prediction filter coefficients.
const XA_FILTER_COEF: [[i32; 2]; 4] = [
    [0x000, 0x000],
    [0x0F0, 0x000],
    [0x1CC, -0x0D0],
    [0x188, -0x0DC],
];

#[derive(Debug, Clone)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct Cdic {
    /// 16 KB buffer RAM, stored in CPU (big-endian word) byte order.
    #[cfg_attr(feature = "savestate", serde(skip, default = "default_ram"))]
    pub ram: Vec<u8>,

    // Registers
    command: u16,
    time: u32,
    file: u16,
    channel: u32,
    audio_channel: u16,
    audio_buffer: u16,
    x_buffer: u16,
    z_buffer: u16,
    interrupt_vector: u16,
    data_buffer: u16,

    // Disc state
    disc_command: u8,
    disc_mode: DiscMode,
    /// DBUF bit 14 gates sector delivery without discarding the configured
    /// read mode. The optical position continues advancing while gated.
    #[cfg_attr(feature = "savestate", serde(default))]
    disc_read_enabled: bool,
    /// Commands $23/$24 stop the active transport after the sector already
    /// under the optical head has completed.
    #[cfg_attr(feature = "savestate", serde(default))]
    stop_after_next_sector: bool,
    disc_spinup_counter: u8,
    curr_lba: u32,
    /// Physical frame of LBA 0 (150 normally; 0 when track 1 stores its
    /// pregap, i.e. CD-i Ready rips — matches MAME's track layout).
    lba_base: u32,

    // Audio state
    audio_sector_counter: u8,
    audio_format_sectors: u8,
    decoding_audio_map: bool,
    audio_map_stop_requested: bool,
    decode_addr: u16,
    /// Next CD-fed XA buffer: 0 = $2800, 1 = $3200.
    realtime_audio_next_write: u8,
    realtime_audio_ready: [bool; 2],
    realtime_audio_next_play: u8,
    realtime_audio_counter: u8,
    realtime_audio_playing: bool,
    cdda_pending: Option<Vec<u8>>,
    atten: [u8; 4],
    xa_last: [i16; 4],

    /// Decoded audio, interleaved stereo i16 at 44.1 kHz; drained by the
    /// frontend.
    #[cfg_attr(feature = "savestate", serde(skip))]
    pub audio_out: Vec<i16>,

    // Timing (75 Hz pumps @ 15 MHz CPU clock)
    sector_accum: u64,
    audio_accum: u64,

    int_line: bool,
    /// Word-assembly latches for byte-wide bus access to 16-bit registers.
    read_latch: u8,
    write_latch: u8,
}

#[cfg(feature = "savestate")]
fn default_ram() -> Vec<u8> {
    vec![0; 0x4000]
}

const PUMP_PERIOD: u64 = 15_000_000 / 75;

impl Default for Cdic {
    fn default() -> Self {
        Self::new()
    }
}

impl Cdic {
    pub fn new() -> Self {
        Self {
            ram: vec![0; 0x4000],
            command: 0,
            time: 0,
            file: 0,
            channel: 0xFFFF_FFFF,
            audio_channel: 0xFFFF,
            audio_buffer: 0,
            x_buffer: 0,
            z_buffer: 0,
            interrupt_vector: 0x0F,
            data_buffer: 0,
            disc_command: 0,
            disc_mode: DiscMode::Idle,
            disc_read_enabled: false,
            stop_after_next_sector: false,
            disc_spinup_counter: 0,
            curr_lba: 0,
            lba_base: 150,
            audio_sector_counter: 0,
            audio_format_sectors: 0,
            decoding_audio_map: false,
            audio_map_stop_requested: false,
            decode_addr: 0,
            realtime_audio_next_write: 0,
            realtime_audio_ready: [false; 2],
            realtime_audio_next_play: 0,
            realtime_audio_counter: 0,
            realtime_audio_playing: false,
            cdda_pending: None,
            atten: [0; 4],
            xa_last: [0; 4],
            audio_out: Vec::new(),
            sector_accum: 0,
            audio_accum: 0,
            int_line: false,
            read_latch: 0,
            write_latch: 0,
        }
    }

    pub fn reset(&mut self) {
        let ram = std::mem::take(&mut self.ram);
        *self = Self { ram, ..Self::new() };
    }

    /// Remove power from the CD interface, clearing its volatile RAM.
    pub fn power_cycle(&mut self) {
        *self = Self::new();
    }

    pub fn int_line(&self) -> bool {
        self.int_line
    }

    pub fn diagnostic_snapshot(&self) -> CdicDiagnosticSnapshot {
        let disc_mode = match self.disc_mode {
            DiscMode::Idle => 0,
            DiscMode::Mode1 => 1,
            DiscMode::Mode2 => 2,
            DiscMode::Cdda => 3,
            DiscMode::Toc => 4,
        };
        CdicDiagnosticSnapshot {
            command: self.command,
            disc_mode,
            disc_read_enabled: self.disc_read_enabled,
            current_lba: self.curr_lba,
            selected_file: self.file,
            selected_channels: self.channel,
            audio_channel: self.audio_channel,
            audio_buffer: self.audio_buffer,
            x_buffer: self.x_buffer,
            z_buffer: self.z_buffer,
            data_buffer: self.data_buffer,
            interrupt_vector: self.interrupt_vector,
            interrupt_asserted: self.int_line,
            queued_audio_samples: self.audio_out.len(),
        }
    }

    /// IN4 interrupt-acknowledge vector.
    pub fn intack(&self) -> u8 {
        (self.interrupt_vector & 0xFF) as u8
    }

    pub fn set_attenuation(&mut self, state: u32) {
        self.atten = state.to_be_bytes();
    }

    /// Called when a disc is (un)loaded to set the LBA-0 physical base.
    pub fn set_disc_layout(&mut self, disc: Option<&DiscImage>) {
        self.lba_base = disc.and_then(|d| d.tracks().first()).map_or(150, |t| {
            if t.region_start == 0 {
                0
            } else {
                150
            }
        });
    }

    /// Stop drive-backed transport when the mounted medium changes without
    /// resetting the host or the CDIC register interface.
    pub fn media_changed(&mut self, disc: Option<&DiscImage>) {
        self.cancel_disc_read();
        self.stop_realtime_audio();
        self.audio_out.clear();
        self.xa_last = [0; 4];
        self.set_disc_layout(disc);
    }

    fn update_interrupt_state(&mut self) {
        // AUDCTL bit 13 enables sound-map buffer interrupts.  Mono-I
        // captures leave ABUF bit 15 set after an AUDCTL-reset abort, but do
        // not raise an IRQ because that write also cleared the enable bit.
        let audio_map_irq = self.audio_buffer & 0x8000 != 0 && self.z_buffer & 0x2000 != 0;
        self.int_line = self.x_buffer & 0x8000 != 0 || audio_map_irq;
    }

    // --- Register access (16-bit registers at $303C00 / $303FF4+) --------

    pub fn read16(&mut self, offset: u32) -> u16 {
        log::trace!("cdic: read16 +{offset:#06x}");
        match offset {
            0x3C00 => self.command,
            0x3C02 => (self.time >> 16) as u16,
            0x3C04 => self.time as u16,
            0x3C06 => self.file,
            0x3C08 => (self.channel >> 16) as u16,
            0x3C0A => self.channel as u16,
            0x3C0C => self.audio_channel,
            0x3FF4 => {
                let v = self.audio_buffer;
                self.audio_buffer &= 0x7FFF;
                self.update_interrupt_state();
                v
            }
            0x3FF6 => {
                let v = self.x_buffer;
                self.x_buffer &= 0x7FFF;
                self.update_interrupt_state();
                v
            }
            0x3FFA => {
                let value = self.z_buffer;
                // CDIC_BlackBoxAnalyzer e861f76, test_audiomap_play_stop:
                // bit 0 reports an $ff-coded stop once; reads never invent
                // the alternating value used by the old MAME-derived HLE.
                self.z_buffer &= !0x0001;
                value
            }
            0x3FFE => self.data_buffer,
            _ => {
                log::trace!("cdic: read16 +{offset:#06x} (unknown)");
                0
            }
        }
    }

    /// Write; DMA control (0x3FF8) is handled by the caller (needs bus
    /// access) via [`Cdic::dma_request`].
    pub fn write16(&mut self, offset: u32, data: u16) {
        log::trace!("cdic: write16 +{offset:#06x} = {data:#06x}");
        match offset {
            0x3C00 => self.command = data,
            0x3C02 => self.time = (self.time & 0x0000_FFFF) | (u32::from(data) << 16),
            0x3C04 => self.time = (self.time & 0xFFFF_0000) | u32::from(data),
            0x3C06 => self.file = data,
            0x3C08 => self.channel = (self.channel & 0x0000_FFFF) | (u32::from(data) << 16),
            0x3C0A => self.channel = (self.channel & 0xFFFF_0000) | u32::from(data),
            0x3C0C => self.audio_channel = data,
            0x3FF4 => self.audio_buffer = data,
            0x3FF6 => self.x_buffer = data,
            0x3FFA => {
                let stop_status = self.z_buffer & 0x0001;
                self.z_buffer = (data & !0x0001) | stop_status;
                log::debug!(
                    "cdic: Z-buffer write {data:#06x} (map={}, decode={:#06x})",
                    self.decoding_audio_map,
                    self.decode_addr
                );
                if data & 0x0800 == 0 {
                    self.stop_realtime_audio();
                    if self.decoding_audio_map {
                        // AUDCTL cannot stop the sector already being
                        // decoded. Latch the request and report completion
                        // only when that sector's playback time elapses.
                        self.audio_map_stop_requested = true;
                    } else {
                        self.decode_addr = 0xFFFF;
                    }
                } else if data & 0x2000 != 0 && !self.decoding_audio_map {
                    // CD-RTOS gives memory sound maps priority over disc
                    // audio. Starting one aborts CD-DA; real-time ADPCM keeps
                    // streaming but remains inaudible until the map ends.
                    if self.disc_mode == DiscMode::Cdda {
                        self.cancel_disc_read();
                    }
                    self.decode_addr = self.z_buffer & 0x3A00;
                    self.audio_format_sectors = 0;
                    self.audio_sector_counter = 1;
                    self.decoding_audio_map = true;
                    self.audio_map_stop_requested = false;
                    self.xa_last = [0; 4];
                } else if data & 0x2000 == 0 && !self.decoding_audio_map {
                    // Hardware traces start CDDA and CD-fed XA only after
                    // software writes AUDCTL $0800. Sector receipt merely
                    // fills the CDIC's audio buffer.
                    self.start_realtime_audio();
                }
                self.update_interrupt_state();
            }
            0x3FFC => self.interrupt_vector = data,
            0x3FFE => {
                log::debug!(
                    "cdic: DBUF write {data:#06x} (configured command {:#04x}, mode {:?}, enabled={}, lba={})",
                    self.disc_command,
                    self.disc_mode,
                    self.disc_read_enabled,
                    self.curr_lba
                );
                self.data_buffer = data;
                if self.data_buffer & 0x8000 != 0 {
                    self.handle_command();
                } else {
                    // CDIC_BlackBoxAnalyzer e861f76,
                    // test_mode2_read_stop_read: DBUF $0000 pauses an active
                    // read and $4000 resumes later in the same continuous
                    // read. It does not reset the command, mode, or filters;
                    // the physical trace shows the optical position advancing
                    // while delivery is disabled.
                    self.disc_read_enabled =
                        self.data_buffer & 0x4000 != 0 && self.disc_command != 0;
                }
            }
            _ => log::trace!("cdic: write16 +{offset:#06x} = {data:#06x} (unknown)"),
        }
    }

    /// Byte-lane access: a 68k word access arrives as even (high) then odd
    /// (low) byte; the 16-bit handler fires on the even read / odd write.
    pub fn read8(&mut self, offset: u32) -> u8 {
        if offset & 1 == 0 {
            let v = self.read16(offset);
            self.read_latch = v as u8;
            (v >> 8) as u8
        } else {
            self.read_latch
        }
    }

    pub fn write8(&mut self, offset: u32, val: u8) -> Option<u16> {
        if offset & 1 == 0 {
            self.write_latch = val;
            None
        } else {
            let word = (u16::from(self.write_latch) << 8) | u16::from(val);
            let reg = offset & !1;
            if reg == 0x3FF8 {
                // DMA control: the machine performs the copy.
                return Some(word);
            }
            self.write16(reg, word);
            None
        }
    }

    pub fn ram_read8(&self, offset: u32) -> u8 {
        self.ram[(offset as usize) & 0x3FFF]
    }

    pub fn ram_write8(&mut self, offset: u32, val: u8) {
        let idx = (offset as usize) & 0x3FFF;
        self.ram[idx] = val;
    }

    // --- Commands ---------------------------------------------------------

    fn lba_from_time(&self) -> u32 {
        let from_bcd = |v: u8| u32::from(v >> 4) * 10 + u32::from(v & 0xF);
        let mins = from_bcd((self.time >> 24) as u8);
        let secs = from_bcd((self.time >> 16) as u8);
        let mut lba = (mins * 60 + secs) * 75;
        let frac_bcd = (self.time >> 8) as u8;
        if frac_bcd & 0x80 == 0 {
            lba += from_bcd(frac_bcd);
        }
        lba.saturating_sub(150)
    }

    fn init_disc_read(&mut self, mode: DiscMode) {
        self.disc_command = self.command as u8;
        self.disc_mode = mode;
        self.disc_read_enabled = self.data_buffer & 0x4000 != 0;
        self.stop_after_next_sector = false;
        self.curr_lba = self.lba_from_time();
        // Spinup delay; MAME uses >= 6 ticks to avoid softlocks.
        self.disc_spinup_counter = 6;
        match mode {
            DiscMode::Mode2 => {
                self.realtime_audio_next_write = 0;
                self.realtime_audio_ready = [false; 2];
                self.realtime_audio_next_play = 0;
                self.realtime_audio_counter = 0;
            }
            DiscMode::Cdda => self.cdda_pending = None,
            _ => {}
        }
    }

    fn cancel_disc_read(&mut self) {
        self.disc_command = 0;
        self.disc_mode = DiscMode::Idle;
        self.disc_read_enabled = false;
        self.stop_after_next_sector = false;
        self.curr_lba = 0;
        self.disc_spinup_counter = 0;
    }

    fn handle_command(&mut self) {
        log::debug!(
            "cdic: command {:#06x} time {:#010x}",
            self.command,
            self.time
        );
        match self.command {
            // CDIC_BlackBoxAnalyzer e861f76 and the CD-i 220 cdapdriv at ROM
            // $429e12 agree that $23/$24 are stop operations. The captured
            // transport completes the sector already under the head before
            // stopping; SS_Cont subsequently starts a fresh $2a read.
            0x23 | 0x24 => {
                if self.disc_command != 0 {
                    self.disc_read_enabled = self.data_buffer & 0x4000 != 0;
                    self.stop_after_next_sector = true;
                }
            }
            0x2B => self.cancel_disc_read(), // Stop CDDA
            // The channel/configuration registers are live. cdapdriv issues
            // Update after changing them, but no additional CDIC-side latch
            // is observable (see docs/icdia-archive-assessment.md).
            0x2E => {}
            0x27 => self.init_disc_read(DiscMode::Toc),
            0x28 => self.init_disc_read(DiscMode::Cdda),
            0x29 | 0x2C => self.init_disc_read(DiscMode::Mode1), // Read Mode 1 / Seek
            0x2A => self.init_disc_read(DiscMode::Mode2),
            other => log::debug!("cdic: unknown command {other:#06x}"),
        }
        self.data_buffer &= !0x8000;
    }

    // --- Sector pump ------------------------------------------------------

    /// Advance by `cycles` CPU cycles; needs the loaded disc (if any).
    pub fn tick(&mut self, cycles: u64, disc: Option<&DiscImage>) {
        self.sector_accum += cycles;
        while self.sector_accum >= PUMP_PERIOD {
            self.sector_accum -= PUMP_PERIOD;
            self.sector_tick(disc);
        }
        self.audio_accum += cycles;
        while self.audio_accum >= PUMP_PERIOD {
            self.audio_accum -= PUMP_PERIOD;
            self.audio_tick();
        }
    }

    fn sector_tick(&mut self, disc: Option<&DiscImage>) {
        if self.disc_command == 0 {
            return;
        }
        if self.disc_spinup_counter != 0 {
            self.disc_spinup_counter -= 1;
            return;
        }
        if !self.disc_read_enabled {
            self.curr_lba += 1;
            return;
        }
        let Some(disc) = disc else {
            return;
        };
        self.process_disc_sector(disc);
        if self.disc_command == 0 {
            self.cancel_disc_read();
            return;
        }
        self.curr_lba += 1;
    }

    fn expected_msf_bcd(&self) -> [u8; 3] {
        let real = self.curr_lba + 150;
        let bcd = |v: u32| (((v / 10) << 4) | (v % 10)) as u8;
        [bcd(real / (60 * 75)), bcd((real / 75) % 60), bcd(real % 75)]
    }

    fn is_valid_sector(&self, buf: &[u8]) -> bool {
        let msf = self.expected_msf_bcd();
        buf[SECTOR_MINUTES] == msf[0]
            && buf[SECTOR_SECONDS] == msf[1]
            && buf[SECTOR_FRACS] == msf[2]
            && (buf[SECTOR_MODE] == 1 || buf[SECTOR_MODE] == 2)
            && buf[SECTOR_FILE1] == buf[SECTOR_FILE2]
            && buf[SECTOR_CHAN1] == buf[SECTOR_CHAN2]
            && buf[SECTOR_SUBMODE1] == buf[SECTOR_SUBMODE2]
            && buf[SECTOR_CODING1] == buf[SECTOR_CODING2]
    }

    fn is_mode2_sector_selected(&mut self, buf: &[u8]) -> bool {
        if u16::from(buf[SECTOR_FILE2]) << 8 != self.file {
            return false;
        }
        let submode = buf[SECTOR_SUBMODE2];
        if submode & SUBMODE_EOF != 0 {
            self.disc_command = 0;
        }
        // The CDIC delivers event sectors after file selection. cdapdriv then
        // applies the Green Book's finer rule by clearing EOR when the channel
        // was not selected, while retaining EOF/TRIG (cdi220b ROM $2A804).
        if submode & (SUBMODE_EOF | SUBMODE_TRIG | SUBMODE_EOR) != 0 {
            return true;
        }
        if submode & (SUBMODE_DATA | SUBMODE_AUDIO | SUBMODE_VIDEO) == 0 {
            return false;
        }
        self.channel & (1 << buf[SECTOR_CHAN2]) != 0
    }

    fn is_mode2_audio_selected(&self, buf: &[u8]) -> bool {
        if buf[SECTOR_SUBMODE2] & SUBMODE_FORM == 0 || buf[SECTOR_SUBMODE2] & SUBMODE_AUDIO == 0 {
            return false;
        }
        self.audio_channel & (1 << buf[SECTOR_CHAN2]) != 0
    }

    fn sector_count_for_coding(coding: u8) -> u8 {
        let mut count: u8 = 2;
        match coding & CODING_BPS_MASK {
            CODING_4BPS => count *= 2,
            CODING_8BPS | CODING_16BPS => {}
            _ => count = 0, // MPEG unsupported
        }
        match coding & CODING_RATE_MASK {
            CODING_18KHZ => count = count.saturating_mul(2),
            CODING_37KHZ | CODING_44KHZ => {}
            _ => count = 0,
        }
        match coding & CODING_CHAN_MASK {
            CODING_MONO => count = count.saturating_mul(2),
            CODING_STEREO => {}
            _ => count = 0,
        }
        count
    }

    fn process_disc_sector(&mut self, disc: &DiscImage) {
        let abs = self.curr_lba + self.lba_base;
        let Some(mut buffer) = disc.read_sector_raw(abs) else {
            log::debug!("cdic: read past disc end at abs {abs}");
            self.cancel_disc_read();
            return;
        };

        if !self.is_valid_sector(&buffer) {
            let mut candidate = buffer;
            descramble_in_place(&mut candidate);
            if self.is_valid_sector(&candidate) {
                buffer = candidate;
            }
        }

        let mode2_read = self.disc_mode == DiscMode::Mode2;
        if buffer[SECTOR_MODE] == 2 && mode2_read {
            if !self.is_mode2_sector_selected(&buffer) {
                return;
            }
        } else if self.disc_mode == DiscMode::Cdda {
            self.audio_sector_counter = 2;
            self.decoding_audio_map = false;
            self.receive_cdda_sector(&buffer);
            if (self.curr_lba + 150) % 75 != 0 {
                return;
            }
        }

        // Subcode-Q synthesis.
        let msf = self.expected_msf_bcd();
        let mut q = [0u8; 12];
        if self.disc_mode == DiscMode::Toc {
            self.build_toc_into_buffer(disc, &mut buffer, &mut q, msf);
        } else {
            // Q reports the track the head is actually over, the index, and
            // time relative to that track's INDEX 01 — not the absolute time.
            // Multi-track discs (VCD MPEG tracks, mixed-mode audio) position
            // themselves with this, and reporting track 1 for every sector
            // makes a player treat correct reads as errors.
            let bcd = |v: u32| (((v / 10) << 4) | (v % 10)) as u8;
            let msf_bcd = |frames: u32| {
                [
                    bcd(frames / (60 * 75)),
                    bcd((frames / 75) % 60),
                    bcd(frames % 75),
                ]
            };
            let (control, tno, index, relative) = match disc.track_at(abs) {
                Some(track) => {
                    let control = if track.mode.is_data() { 0x41 } else { 0x01 };
                    // Inside a pregap the relative time counts down to INDEX 01.
                    let (index, relative) = if abs >= track.start {
                        (1, abs - track.start)
                    } else {
                        (0, track.start - abs)
                    };
                    (control, u32::from(track.number), index, relative)
                }
                None => {
                    let control = if self.disc_mode == DiscMode::Cdda {
                        0x01
                    } else {
                        0x41
                    };
                    (control, 1, 1, abs.saturating_sub(150))
                }
            };
            q[0] = control;
            q[1] = bcd(tno);
            q[2] = bcd(index);
            q[3..6].copy_from_slice(&msf_bcd(relative));
            q[6] = 0x00;
            q[7..10].copy_from_slice(&msf);
        }
        let crc = crc_ccitt(&q[..10]);
        q[10] = (crc >> 8) as u8;
        q[11] = crc as u8;

        self.deliver_sector(&buffer, &q);
    }

    fn build_toc_into_buffer(
        &self,
        disc: &DiscImage,
        buffer: &mut [u8],
        q: &mut [u8; 12],
        msf: [u8; 3],
    ) {
        let bcd = |v: u32| (((v / 10) << 4) | (v % 10)) as u8;
        let mut entries: Vec<[u8; 5]> = Vec::new();

        let audio_tracks: Vec<u32> = disc
            .tracks()
            .iter()
            .filter(|t| !t.mode.is_data())
            .map(|t| t.start)
            .collect();
        let has_data = disc.tracks().iter().any(|t| t.mode.is_data());

        for (i, &start) in audio_tracks.iter().enumerate() {
            let entry = [
                0x01,
                bcd((i + 1) as u32),
                bcd(start / (60 * 75)),
                bcd((start / 75) % 60),
                bcd(start % 75),
            ];
            for _ in 0..3 {
                entries.push(entry);
            }
        }
        for _ in 0..3 {
            entries.push([
                if has_data { 0x41 } else { 0x01 },
                0xA0,
                0x01,
                if has_data { 0x10 } else { 0x00 },
                0x00,
            ]);
        }
        for _ in 0..3 {
            let last_audio = audio_tracks.len().saturating_sub(1) as u32;
            entries.push([
                if audio_tracks.is_empty() { 0x41 } else { 0x01 },
                0xA1,
                bcd(last_audio),
                0x00,
                0x00,
            ]);
        }
        let leadout = disc.leadout();
        for _ in 0..3 {
            entries.push([
                if audio_tracks.is_empty() { 0x41 } else { 0x01 },
                0xA2,
                bcd(leadout / (60 * 75)),
                bcd((leadout / 75) % 60),
                bcd(leadout % 75),
            ]);
        }

        for (i, e) in entries.iter().enumerate() {
            let at = i * 5;
            if at + 5 <= buffer.len() {
                buffer[at..at + 5].copy_from_slice(e);
            }
        }

        let pick = &entries[(self.curr_lba as usize) % entries.len()];
        q[0] = pick[0];
        q[1] = 0x00;
        q[2] = pick[1];
        q[3] = 0xA0;
        q[4] = msf[1];
        q[5] = msf[2];
        q[6] = 0x00;
        q[7] = pick[2];
        q[8] = pick[3];
        q[9] = pick[4];
    }

    /// Copy a processed sector + subcode into the ping-pong buffers and
    /// raise the data-ready interrupt.
    fn deliver_sector(&mut self, buffer: &[u8], q: &[u8; 12]) {
        let realtime_audio =
            self.disc_mode == DiscMode::Mode2 && self.is_mode2_audio_selected(buffer);
        let audio_slot = if realtime_audio {
            let slot = self.realtime_audio_next_write;
            self.realtime_audio_next_write ^= 1;
            self.data_buffer = (self.data_buffer & !0x0005) | 0x0004 | u16::from(slot);
            Some(usize::from(slot))
        } else {
            self.data_buffer ^= 0x0001;
            self.data_buffer &= !0x0004;
            None
        };

        let put = |ram: &mut [u8], idx: &mut usize, word: u16| {
            let at = (*idx * 2) & 0x3FFE;
            ram[at] = (word >> 8) as u8;
            ram[at + 1] = word as u8;
            *idx += 1;
        };

        let slot = usize::from(self.data_buffer & 1);
        let mut header_word = slot * 0x0A00 / 2;
        // A selected XA sector still updates the ordinary data-buffer header.
        // Mono-I captures in CDIC_BlackBoxAnalyzer test_xa_play observe the
        // timestamp/mode words at $0000/$0a00 while DBUF reports audio slot
        // 4/5.  CD-RTOS inspects this header even though the sector body is
        // routed to the corresponding ADPCM buffer.
        for i in (SECTOR_HEADER..SECTOR_FILE2).step_by(2) {
            let word = (u16::from(buffer[i]) << 8) | u16::from(buffer[i + 1]);
            put(&mut self.ram, &mut header_word, word);
        }

        // The ADPCM buffer contains the complete post-sync sector image: its
        // coding byte is at offset 11 and its 2304-byte sample payload begins
        // at offset 12 (CDIC_BlackBoxAnalyzer cdic_manual, "Playing CD-I
        // ADPCM from CPU").  It is not a continuation of the short header
        // copy above.
        let mut word_idx = if realtime_audio {
            (0x2800 + slot * 0x0A00) / 2
        } else {
            header_word
        };
        let body_start = if realtime_audio {
            SECTOR_HEADER
        } else {
            SECTOR_FILE2
        };
        for i in (body_start..SECTOR_SIZE).step_by(2) {
            let w = (u16::from(buffer[i]) << 8) | u16::from(buffer[i + 1]);
            put(&mut self.ram, &mut word_idx, w);
        }
        for &b in q.iter() {
            put(&mut self.ram, &mut word_idx, u16::from(b));
        }

        self.x_buffer |= 0x8000;
        self.data_buffer |= 0x4000;
        self.update_interrupt_state();
        if let Some(slot) = audio_slot {
            self.realtime_audio_ready[slot] = true;
            self.try_play_realtime_audio();
        }
        log::debug!(
            "cdic: sector delivered (lba {}, buffer {:#06x}, vector {:#04x}, file/channel/submode {:02x}/{:02x}/{:02x}, filters {:#06x}/{:#010x}/{:#06x})",
            self.curr_lba,
            self.data_buffer,
            self.interrupt_vector,
            buffer[SECTOR_FILE2],
            buffer[SECTOR_CHAN2],
            buffer[SECTOR_SUBMODE2],
            self.file,
            self.channel,
            self.audio_channel,
        );
        if self.stop_after_next_sector {
            self.cancel_disc_read();
        }
    }

    // --- Audio ------------------------------------------------------------

    fn audio_tick(&mut self) {
        if self.realtime_audio_counter > 0 {
            self.realtime_audio_counter -= 1;
            if self.realtime_audio_counter == 0 {
                self.try_play_realtime_audio();
            }
        }
        if self.audio_sector_counter > 0 {
            self.audio_sector_counter -= 1;
            if self.audio_sector_counter > 0 {
                return;
            }
        }
        if self.decoding_audio_map {
            if self.audio_map_stop_requested {
                self.audio_map_stop_requested = false;
                self.decode_addr = 0xFFFF;
                self.audio_format_sectors = 0;
                self.decoding_audio_map = false;
                self.audio_buffer |= 0x8000;
                self.update_interrupt_state();
                return;
            }
            self.process_audio_map();
        }
    }

    fn process_audio_map(&mut self) {
        if self.decode_addr == 0xFFFF {
            let was_decoding = self.decoding_audio_map;
            self.audio_sector_counter = 0;
            self.audio_format_sectors = 0;
            self.decoding_audio_map = false;
            if was_decoding {
                self.audio_buffer |= 0x8000;
                self.update_interrupt_state();
            }
            return;
        }
        let base = usize::from(self.decode_addr & 0x3FFE);
        self.decode_addr ^= 0x1A00;

        let was_decoding = self.audio_format_sectors != 0;
        let coding = self.ram[(base + (SECTOR_CODING2 - SECTOR_HEADER)) & 0x3FFF];
        log::trace!(
            "cdic: sound-map buffer {base:#06x}, coding {coding:#04x}, next {:#06x}",
            self.decode_addr
        );
        if coding != 0xFF {
            self.decoding_audio_map = true;
            self.audio_format_sectors = Self::sector_count_for_coding(coding);
            self.audio_sector_counter = self.audio_format_sectors;
            let data_start = base + (SECTOR_DATA - SECTOR_HEADER);
            let mut sector_data = vec![0u8; SECTOR_AUDIO_SIZE];
            for (i, b) in sector_data.iter_mut().enumerate() {
                *b = self.ram[(data_start + i) & 0x3FFF];
            }
            self.play_audio_data(coding, &sector_data);
        } else {
            log::debug!("cdic: sound map ended at buffer {base:#06x}");
            self.decode_addr = 0xFFFF;
            self.audio_sector_counter = 0;
            self.audio_format_sectors = 0;
            self.decoding_audio_map = false;
            self.z_buffer = (self.z_buffer & !0x0800) | 0x0001;
        }
        // The completion bit reports transfer of the preceding sound-map
        // buffer to the audio processor, not the end of the whole map.  The
        // CD-RTOS driver relies on this per-buffer IRQ to refill the inactive
        // half or write its 0xff terminator.  Waiting for the terminator here
        // deadlocks software and repeats the same two buffers forever.
        if was_decoding {
            self.audio_buffer |= 0x8000;
            self.update_interrupt_state();
        }
    }

    fn play_cdda_sector(&mut self, data: &[u8]) {
        // Red-book audio: 16-bit little-endian stereo at 44.1 kHz.
        for pair in data.chunks_exact(4) {
            let l = i16::from_le_bytes([pair[0], pair[1]]);
            let r = i16::from_le_bytes([pair[2], pair[3]]);
            self.audio_out.push(l);
            self.audio_out.push(r);
        }
    }

    fn receive_cdda_sector(&mut self, data: &[u8]) {
        if self.realtime_audio_playing && !self.decoding_audio_map {
            self.play_cdda_sector(data);
        } else if self.cdda_pending.is_none() {
            self.cdda_pending = Some(data.to_vec());
        }
    }

    fn start_realtime_audio(&mut self) {
        if self.realtime_audio_playing {
            return;
        }
        self.realtime_audio_playing = true;
        if self.disc_mode == DiscMode::Cdda {
            if let Some(sector) = self.cdda_pending.take() {
                self.play_cdda_sector(&sector);
            }
            return;
        }
        self.realtime_audio_next_play = 0;
        self.try_play_realtime_audio();
    }

    fn stop_realtime_audio(&mut self) {
        self.realtime_audio_playing = false;
        self.realtime_audio_counter = 0;
        self.cdda_pending = None;
    }

    fn try_play_realtime_audio(&mut self) {
        if !self.realtime_audio_playing
            || self.decoding_audio_map
            || self.realtime_audio_counter != 0
        {
            return;
        }
        let slot = usize::from(self.realtime_audio_next_play);
        if !self.realtime_audio_ready[slot] {
            return;
        }
        self.realtime_audio_ready[slot] = false;
        self.realtime_audio_next_play ^= 1;

        let base = 0x2800 + slot * 0x0A00;
        let coding = self.ram[base + (SECTOR_CODING2 - SECTOR_HEADER)];
        if coding == 0xFF {
            self.realtime_audio_playing = false;
            self.z_buffer = (self.z_buffer & !0x0800) | 0x0001;
            return;
        }
        let data_start = base + (SECTOR_DATA - SECTOR_HEADER);
        let mut sector_data = vec![0; SECTOR_AUDIO_SIZE];
        sector_data.copy_from_slice(&self.ram[data_start..data_start + SECTOR_AUDIO_SIZE]);
        self.realtime_audio_counter = Self::sector_count_for_coding(coding);
        self.play_audio_data(coding, &sector_data);
    }

    fn play_audio_data(&mut self, coding: u8, data: &[u8]) {
        if coding & CODING_CHAN_MASK > CODING_STEREO
            || coding & CODING_BPS_MASK == CODING_BPS_MPEG
            || coding & CODING_RATE_MASK == CODING_RATE_RESV
            || coding & CODING_RATE_MASK == CODING_44KHZ
            || coding & CODING_BPS_MASK == CODING_16BPS
        {
            log::debug!("cdic: unsupported audio coding {coding:#04x}");
            return;
        }
        let stereo = coding & CODING_CHAN_MASK == CODING_STEREO;
        let source_rate: u32 = if coding & CODING_RATE_MASK == CODING_18KHZ {
            18_900
        } else {
            37_800
        };

        let bps8 = coding & CODING_BPS_MASK == CODING_8BPS;
        let group_samples = if bps8 { 4 } else { 8 } / if stereo { 2 } else { 1 };
        let samples_per_group = 28 * group_samples;
        let group_count = SECTOR_AUDIO_SIZE / 128;
        let total = samples_per_group * group_count;
        let mut left = vec![0i16; total];
        let mut right = vec![0i16; total];

        for (g, group) in data.chunks_exact(128).enumerate().take(group_count) {
            let idx = g * samples_per_group;
            self.play_xa_group(coding, group, idx, &mut left, &mut right);
        }
        if !stereo {
            right.copy_from_slice(&left);
        }

        // Attenuation matrix (L->L, L->R, R->R, R->L), dB steps.
        let scale = |a: u8| 10f32.powf(-f32::from(a) / 20.0);
        let (sll, slr, srr, srl) = (
            scale(self.atten[0]),
            scale(self.atten[1]),
            scale(self.atten[2]),
            scale(self.atten[3]),
        );

        // Naive resample to 44.1 kHz.
        let mut pos: u32 = 0;
        for i in 0..total {
            let l = f32::from(left[i]);
            let r = f32::from(right[i]);
            let out_l = ((l * sll + r * srl) * 0.25) as i16;
            let out_r = ((l * slr + r * srr) * 0.25) as i16;
            pos += 44_100;
            while pos >= source_rate {
                pos -= source_rate;
                self.audio_out.push(out_l);
                self.audio_out.push(out_r);
            }
        }
    }

    fn play_xa_group(
        &mut self,
        coding: u8,
        group: &[u8],
        idx: usize,
        left: &mut [i16],
        right: &mut [i16],
    ) {
        const HDR4: [usize; 8] = [4, 5, 6, 7, 12, 13, 14, 15];
        const HDR8: [usize; 4] = [4, 5, 6, 7];
        const DATA4: [usize; 8] = [16, 16, 17, 17, 18, 18, 19, 19];
        const DATA8: [usize; 4] = [16, 17, 18, 19];

        let bps8 = coding & CODING_BPS_MASK == CODING_8BPS;
        let stereo = coding & CODING_CHAN_MASK == CODING_STEREO;
        let units = if bps8 { 4 } else { 8 };

        for i in 0..units {
            let (param, data_at, shift) = if bps8 {
                (group[HDR8[i]], DATA8[i], 0)
            } else {
                (group[HDR4[i]], DATA4[i], if i & 1 != 0 { 4 } else { 0 })
            };
            let channel = if stereo { i & 1 } else { 0 };
            let out_idx = if stereo {
                idx + (i >> 1) * 28
            } else {
                idx + i * 28
            };
            let out = if stereo && channel == 1 {
                &mut right[out_idx..out_idx + 28]
            } else {
                &mut left[out_idx..out_idx + 28]
            };

            let mut s0 = self.xa_last[channel * 2];
            let mut s1 = self.xa_last[channel * 2 + 1];
            let filter = XA_FILTER_COEF[usize::from((param >> 4) & 3)];
            let range = (param & 0xF).min(12);
            for (j, slot) in out.iter_mut().enumerate() {
                let raw = group[data_at + j * 4];
                let sample: i16 = if bps8 {
                    i16::from_le_bytes([0, raw])
                } else {
                    i16::from((raw >> shift) & 0xF) << 12
                };
                let mut s32 = i32::from(sample) >> range;
                s32 += (filter[0] * i32::from(s0) + filter[1] * i32::from(s1) + 128) >> 8;
                let clipped = s32.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
                s1 = s0;
                s0 = clipped;
                *slot = clipped;
            }
            self.xa_last[channel * 2] = s0;
            self.xa_last[channel * 2 + 1] = s1;
        }
    }
}

/// CRC-16/CCITT over the Q-channel data (poly 0x1021), complemented.
fn crc_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TRANSPORT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    fn mode2_disc_for_transport_test() -> DiscImage {
        let dir = std::env::temp_dir().join(format!(
            "cdi-core-cdic-transport-test-{}-{}",
            std::process::id(),
            TRANSPORT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let bcd = |v: u8| ((v / 10) << 4) | (v % 10);
        let mut image = Vec::with_capacity(3 * SECTOR_SIZE);
        for frame in 0..3u8 {
            let mut sector = [0u8; SECTOR_SIZE];
            sector[SECTOR_MINUTES] = 0;
            sector[SECTOR_SECONDS] = bcd(2);
            sector[SECTOR_FRACS] = bcd(frame);
            sector[SECTOR_MODE] = 2;
            sector[SECTOR_SUBMODE1] = SUBMODE_DATA;
            sector[SECTOR_SUBMODE2] = SUBMODE_DATA;
            image.extend_from_slice(&sector);
        }
        std::fs::write(dir.join("transport.bin"), image).unwrap();
        std::fs::write(
            dir.join("transport.cue"),
            "FILE \"transport.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n",
        )
        .unwrap();
        DiscImage::load(&dir.join("transport.cue")).unwrap()
    }

    #[test]
    fn command_starts_disc_read() {
        let mut c = Cdic::new();
        // Seek to MSF 00:02:16 (LBA 16).
        c.write16(0x3C02, 0x0002);
        c.write16(0x3C04, 0x1600);
        c.write16(0x3C00, 0x0029); // Read Mode 1
        c.write16(0x3FFE, 0xC000); // begin processing, keep disc running
        assert_eq!(c.disc_command, 0x29);
        assert_eq!(c.curr_lba, 16);
        assert_eq!(c.disc_mode, DiscMode::Mode1);
    }

    #[test]
    fn stop_commands_do_not_start_reset_mode_reads() {
        let mut c = Cdic::new();
        c.write16(0x3C00, 0x0023);
        c.write16(0x3FFE, 0xC000);
        assert_eq!(c.disc_command, 0);
        assert_eq!(c.disc_mode, DiscMode::Idle);
        assert!(!c.disc_read_enabled);

        c.write16(0x3C00, 0x0024);
        c.write16(0x3FFE, 0xC000);
        assert_eq!(c.disc_command, 0);
        assert_eq!(c.disc_mode, DiscMode::Idle);
        assert!(!c.disc_read_enabled);
    }

    #[test]
    fn byte_lane_word_assembly() {
        let mut c = Cdic::new();
        c.write8(0x3C06, 0x12);
        assert_eq!(c.write8(0x3C07, 0x34), None);
        assert_eq!(c.read16(0x3C06), 0x1234);
        // Reads: even fetches+latches, odd returns latch.
        assert_eq!(c.read8(0x3C06), 0x12);
        assert_eq!(c.read8(0x3C07), 0x34);
    }

    #[test]
    fn xbuf_read_clears_interrupt() {
        let mut c = Cdic::new();
        c.x_buffer = 0x8123;
        c.update_interrupt_state();
        assert!(c.int_line());
        assert_eq!(c.read16(0x3FF6), 0x8123);
        assert!(!c.int_line());
        assert_eq!(c.read16(0x3FF6), 0x0123);
    }

    #[test]
    fn audctl_ff_status_is_returned_once_instead_of_toggled() {
        let mut c = Cdic::new();
        c.z_buffer = 0x0801;

        assert_eq!(c.read16(0x3FFA), 0x0801);
        assert_eq!(c.read16(0x3FFA), 0x0800);
        assert_eq!(c.read16(0x3FFA), 0x0800);
    }

    #[test]
    fn clearing_audctl_sound_map_enable_masks_a_pending_abuf_interrupt() {
        let mut c = Cdic::new();
        c.z_buffer = 0x2800;
        c.audio_buffer = 0x8000;
        c.update_interrupt_state();
        assert!(c.int_line());

        c.write16(0x3FFA, 0x0000);

        assert_eq!(c.audio_buffer & 0x8000, 0x8000);
        assert!(!c.int_line());
    }

    #[test]
    fn first_realtime_audio_sector_uses_buffer_2800() {
        let mut c = Cdic::new();
        c.command = 0x2A;
        c.disc_mode = DiscMode::Mode2;
        c.audio_channel = 1;
        let mut sector = mode2_sector(0, 0, SUBMODE_FORM | SUBMODE_AUDIO);
        sector[SECTOR_CODING2] = CODING_STEREO;
        sector[SECTOR_DATA] = 0x5A;

        c.deliver_sector(&sector, &[0; 12]);

        assert_eq!(c.data_buffer & 0x000F, 4);
        assert_eq!(c.ram[0], sector[SECTOR_HEADER]);
        assert_eq!(c.ram[1], sector[SECTOR_HEADER + 1]);
        assert_eq!(
            c.ram[0x2800 + (SECTOR_CODING2 - SECTOR_HEADER)],
            CODING_STEREO
        );
        assert_eq!(c.ram[0x2800 + (SECTOR_DATA - SECTOR_HEADER)], 0x5A);
        assert!(
            c.audio_out.is_empty(),
            "sector receipt must not start playback"
        );

        sector[SECTOR_DATA] = 0xA5;
        c.deliver_sector(&sector, &[0; 12]);
        assert_eq!(c.data_buffer & 0x000F, 5);
        assert_eq!(c.ram[0x0A00], sector[SECTOR_HEADER]);
        assert_eq!(c.ram[0x0A01], sector[SECTOR_HEADER + 1]);
        assert_eq!(c.ram[0x3200 + (SECTOR_DATA - SECTOR_HEADER)], 0xA5);

        c.write16(0x3FFA, 0x0800);
        assert!(!c.audio_out.is_empty(), "AUDCTL $0800 starts buffered XA");
        let first_buffer_samples = c.audio_out.len();
        for _ in 0..Cdic::sector_count_for_coding(CODING_STEREO) {
            c.audio_tick();
        }
        assert_eq!(c.audio_out.len(), first_buffer_samples * 2);
    }

    #[test]
    fn sound_map_requires_play_bit_and_ff_sets_one_shot_status() {
        let mut c = Cdic::new();
        c.write16(0x3FFA, 0x2000);
        assert!(
            !c.decoding_audio_map,
            "IRQ enable alone must not start playback"
        );

        c.ram[0x2800 + (SECTOR_CODING2 - SECTOR_HEADER)] = 0xFF;
        c.write16(0x3FFA, 0x2800);
        c.audio_tick();

        assert!(!c.decoding_audio_map);
        assert_eq!(c.audio_buffer & 0x8000, 0);
        assert!(!c.int_line());
        assert_eq!(c.read16(0x3FFA), 0x2001);
        assert_eq!(c.read16(0x3FFA), 0x2000);
    }

    #[test]
    fn cdda_waits_for_audctl_before_pushing_samples() {
        let mut c = Cdic::new();
        c.disc_mode = DiscMode::Cdda;
        let mut sector = [0u8; 2352];
        sector[0] = 0x34;
        sector[1] = 0x12;
        sector[2] = 0x78;
        sector[3] = 0x56;
        c.receive_cdda_sector(&sector);
        assert!(c.audio_out.is_empty());

        c.write16(0x3FFA, 0x0800);
        assert_eq!(c.audio_out[0], 0x1234);
        assert_eq!(c.audio_out[1], 0x5678);
        assert_eq!(c.audio_out.len(), 2 * (2352 / 4));
    }

    #[test]
    fn sound_map_takes_priority_over_realtime_adpcm() {
        let mut c = Cdic::new();
        c.command = 0x2A;
        c.disc_mode = DiscMode::Mode2;
        c.audio_channel = 1;
        let mut sector = mode2_sector(0, 0, SUBMODE_FORM | SUBMODE_AUDIO);
        sector[SECTOR_CODING2] = CODING_STEREO;

        c.decoding_audio_map = true;
        c.deliver_sector(&sector, &[0; 12]);
        c.write16(0x3FFA, 0x0800);
        assert!(c.audio_out.is_empty());
        assert!(
            c.decoding_audio_map,
            "disc audio must not cancel a sound map"
        );

        c.decoding_audio_map = false;
        c.write16(0x3FFA, 0x0800);
        assert!(!c.audio_out.is_empty());
    }

    #[test]
    fn starting_sound_map_aborts_cdda() {
        let mut c = Cdic::new();
        c.disc_command = 0x28;
        c.disc_mode = DiscMode::Cdda;

        c.write16(0x3FFA, 0x2800);

        assert!(c.decoding_audio_map);
        assert_eq!(c.disc_command, 0);
        assert_eq!(c.disc_mode, DiscMode::Idle);
    }

    #[test]
    fn aborting_sound_map_waits_for_sector_without_raising_irq() {
        let mut c = Cdic::new();
        c.decoding_audio_map = true;
        c.audio_sector_counter = 2;
        c.decode_addr = 0x2800;

        c.write16(0x3FFA, 0x0000);
        assert!(c.decoding_audio_map);
        assert_eq!(c.audio_buffer & 0x8000, 0);

        c.audio_tick();
        assert!(c.decoding_audio_map);
        c.audio_tick();
        assert!(!c.decoding_audio_map);
        assert_eq!(c.audio_buffer & 0x8000, 0x8000);
        assert!(!c.int_line());
    }

    #[test]
    fn sound_map_interrupts_after_each_consumed_buffer() {
        let mut c = Cdic::new();
        let coding = CODING_18KHZ | CODING_MONO;
        c.ram[0x2800 + (SECTOR_CODING2 - SECTOR_HEADER)] = coding;
        c.ram[0x3200 + (SECTOR_CODING2 - SECTOR_HEADER)] = coding;

        c.write16(0x3FFA, 0x2800);
        c.audio_tick(); // Prime the first half; there is no preceding buffer.
        assert!(c.decoding_audio_map);
        assert!(!c.int_line());

        for _ in 0..Cdic::sector_count_for_coding(coding) {
            c.audio_tick();
        }
        assert!(c.decoding_audio_map, "the second half remains playable");
        assert_eq!(c.decode_addr, 0x2800);
        assert_eq!(c.audio_buffer & 0x8000, 0x8000);
        assert!(c.int_line(), "software must be told that a half is free");
    }

    fn set_sound_map_half(c: &mut Cdic, base: usize, coding: u8, sample: u8) {
        c.ram[base + (SECTOR_CODING2 - SECTOR_HEADER)] = coding;
        let data_start = base + (SECTOR_DATA - SECTOR_HEADER);
        c.ram[data_start..data_start + SECTOR_AUDIO_SIZE].fill(sample);
    }

    fn finish_sound_map_sector(c: &mut Cdic, coding: u8) {
        for _ in 0..Cdic::sector_count_for_coding(coding) {
            c.audio_tick();
        }
    }

    #[test]
    fn one_sector_sound_map_reports_transfer_done_with_pcm_still_queued() {
        let mut c = Cdic::new();
        let coding = CODING_18KHZ | CODING_MONO;
        set_sound_map_half(&mut c, 0x2800, coding, 0x11);
        set_sound_map_half(&mut c, 0x3200, 0xFF, 0);

        c.write16(0x3FFA, 0x2800);
        c.audio_tick();
        let queued_tail = c.audio_out.len();
        assert!(queued_tail > 0);
        assert!(c.decoding_audio_map);
        assert_eq!(c.audio_buffer & 0x8000, 0);

        finish_sound_map_sector(&mut c, coding);

        // Philips TN079 distinguishes completion of the RAM-to-audio-
        // processor transfer from the later audible end of its internal
        // queue.  `audio_out` is that downstream queue in this HLE.
        assert!(!c.decoding_audio_map);
        assert_eq!(c.decode_addr, 0xFFFF);
        assert_eq!(c.audio_out.len(), queued_tail);
        assert_eq!(c.audio_buffer & 0x8000, 0x8000);
        assert!(c.int_line());
        assert_eq!(c.read16(0x3FFA), 0x2001);
        assert_eq!(c.read16(0x3FFA), 0x2000);

        // CDIC_BlackBoxAnalyzer `test_audiomap_play_stop` observes no
        // second completion interrupt after the $ff-coded stop.
        c.read16(0x3FF4);
        c.audio_tick();
        assert!(!c.int_line());
    }

    #[test]
    fn two_sector_sound_map_interrupts_each_transfer_then_reports_done() {
        let mut c = Cdic::new();
        let coding = CODING_18KHZ | CODING_MONO;
        set_sound_map_half(&mut c, 0x2800, coding, 0x11);
        set_sound_map_half(&mut c, 0x3200, coding, 0x22);

        c.write16(0x3FFA, 0x2800);
        c.audio_tick();
        let one_sector_samples = c.audio_out.len();
        assert!(one_sector_samples > 0);

        finish_sound_map_sector(&mut c, coding);
        assert_eq!(c.audio_out.len(), one_sector_samples * 2);
        assert_eq!(c.audio_buffer & 0x8000, 0x8000);
        assert!(c.decoding_audio_map);

        // Software owns the half whose transfer just completed and can put
        // the terminator there while the other half remains queued.
        c.read16(0x3FF4);
        set_sound_map_half(&mut c, 0x2800, 0xFF, 0);
        finish_sound_map_sector(&mut c, coding);

        assert!(!c.decoding_audio_map);
        assert_eq!(c.audio_out.len(), one_sector_samples * 2);
        assert_eq!(c.audio_buffer & 0x8000, 0x8000);
        assert_eq!(c.read16(0x3FFA), 0x2001);
    }

    #[test]
    fn aborting_sound_map_preserves_queued_tail_and_skips_next_half() {
        let mut c = Cdic::new();
        let coding = CODING_18KHZ | CODING_MONO;
        set_sound_map_half(&mut c, 0x2800, coding, 0x11);
        set_sound_map_half(&mut c, 0x3200, coding, 0x22);

        c.write16(0x3FFA, 0x2800);
        c.audio_tick();
        let queued_tail = c.audio_out.len();
        c.write16(0x3FFA, 0x0000);
        finish_sound_map_sector(&mut c, coding);

        assert!(!c.decoding_audio_map);
        assert_eq!(c.decode_addr, 0xFFFF);
        assert_eq!(c.audio_out.len(), queued_tail);
        assert_eq!(c.audio_buffer & 0x8000, 0x8000);
        assert!(!c.int_line());
        assert_eq!(c.read16(0x3FFA) & 0x0001, 0);
    }

    #[test]
    fn completed_sound_map_can_be_replaced_while_pcm_tail_is_queued() {
        let mut c = Cdic::new();
        let coding = CODING_18KHZ | CODING_MONO;
        set_sound_map_half(&mut c, 0x2800, coding, 0x11);
        set_sound_map_half(&mut c, 0x3200, 0xFF, 0);

        c.write16(0x3FFA, 0x2800);
        c.audio_tick();
        let first_map_samples = c.audio_out.len();
        finish_sound_map_sector(&mut c, coding);
        assert!(!c.decoding_audio_map);

        // A driver can queue the next map as soon as the prior transfer is
        // done; the previous map's downstream PCM must not be discarded.
        c.read16(0x3FFA);
        c.read16(0x3FF4);
        set_sound_map_half(&mut c, 0x2800, coding, 0x33);
        set_sound_map_half(&mut c, 0x3200, 0xFF, 0);
        c.write16(0x3FFA, 0x2800);
        assert!(c.decoding_audio_map);
        c.audio_tick();

        assert_eq!(c.audio_out.len(), first_map_samples * 2);
        assert!(c.decoding_audio_map);
    }

    fn mode2_sector(file: u8, channel: u8, submode: u8) -> [u8; 2352] {
        let mut sector = [0u8; 2352];
        sector[SECTOR_FILE2] = file;
        sector[SECTOR_CHAN2] = channel;
        sector[SECTOR_SUBMODE2] = submode;
        sector
    }

    #[test]
    fn mode2_ordinary_sector_requires_selected_channel() {
        let mut c = Cdic::new();
        c.file = 7 << 8;
        c.channel = 1 << 3;

        assert!(c.is_mode2_sector_selected(&mode2_sector(7, 3, SUBMODE_DATA)));
        assert!(!c.is_mode2_sector_selected(&mode2_sector(7, 4, SUBMODE_DATA)));
        assert!(!c.is_mode2_sector_selected(&mode2_sector(8, 3, SUBMODE_DATA)));
    }

    #[test]
    fn mode2_filters_are_live_and_update_is_a_noop() {
        let mut c = Cdic::new();
        c.file = 7 << 8;
        c.channel = 1 << 3;
        c.audio_channel = 1 << 3;
        assert!(c.is_mode2_sector_selected(&mode2_sector(7, 3, SUBMODE_DATA)));
        assert!(c.is_mode2_audio_selected(&mode2_sector(7, 3, SUBMODE_FORM | SUBMODE_AUDIO,)));

        c.audio_channel = 0;
        c.command = 0x2E;
        c.handle_command();
        assert!(!c.is_mode2_audio_selected(&mode2_sector(7, 3, SUBMODE_FORM | SUBMODE_AUDIO,)));
    }

    #[test]
    fn mode2_event_sector_bypasses_channel_but_not_file_mask() {
        let mut c = Cdic::new();
        c.file = 7 << 8;
        c.channel = 1 << 3;

        // This is the low-level CDIC delivery contract. cdapdriv applies the
        // application-visible EOR channel rule after receiving the sector.
        assert!(c.is_mode2_sector_selected(&mode2_sector(7, 4, SUBMODE_TRIG)));
        assert!(c.is_mode2_sector_selected(&mode2_sector(7, 4, SUBMODE_EOR | SUBMODE_DATA)));
        assert!(c.is_mode2_sector_selected(&mode2_sector(7, 3, SUBMODE_EOR | SUBMODE_DATA)));
        assert!(!c.is_mode2_sector_selected(&mode2_sector(8, 4, SUBMODE_TRIG)));
    }

    #[test]
    fn dbuf_enable_pauses_delivery_while_the_head_advances_then_resumes() {
        let disc = mode2_disc_for_transport_test();
        let mut c = Cdic::new();
        c.command = 0x2A;
        c.data_buffer = 0xC000;
        c.init_disc_read(DiscMode::Mode2);
        c.disc_spinup_counter = 0;
        c.sector_tick(Some(&disc));
        assert_eq!(c.curr_lba, 1);

        c.write16(0x3FFE, 0);
        assert_eq!(c.disc_command, 0x2A);
        assert_eq!(c.disc_mode, DiscMode::Mode2);
        assert!(!c.disc_read_enabled);
        assert_eq!(c.curr_lba, 1);
        let x_buffer_before_pause = c.x_buffer;
        c.sector_tick(Some(&disc));
        assert_eq!(
            c.curr_lba, 2,
            "the spinning disc keeps moving under the head"
        );
        assert_eq!(
            c.x_buffer, x_buffer_before_pause,
            "paused delivery must not fill another guest buffer"
        );

        c.write16(0x3FFE, 0x4000);
        assert_eq!(c.disc_command, 0x2A);
        assert_eq!(c.disc_mode, DiscMode::Mode2);
        assert!(c.disc_read_enabled);
        assert_eq!(c.curr_lba, 2);
        c.sector_tick(Some(&disc));
        assert_eq!(
            c.curr_lba, 3,
            "resume must deliver at the live head position"
        );
    }

    #[test]
    fn stop_commands_finish_the_sector_under_the_head_then_stop() {
        let disc = mode2_disc_for_transport_test();
        for command in [0x0023, 0x0024] {
            let mut c = Cdic::new();
            c.command = 0x2A;
            c.data_buffer = 0xC000;
            c.channel = 1;
            c.init_disc_read(DiscMode::Mode2);
            c.disc_spinup_counter = 0;

            c.write16(0x3C00, command);
            c.write16(0x3FFE, 0xC000);
            assert_eq!(c.disc_command, 0x2A);
            assert_eq!(c.disc_mode, DiscMode::Mode2);
            assert!(c.disc_read_enabled);
            assert!(c.stop_after_next_sector);

            c.sector_tick(Some(&disc));
            assert_ne!(
                c.x_buffer & 0x8000,
                0,
                "the in-flight sector must complete before command {command:#04x} stops"
            );
            assert_eq!(c.disc_command, 0);
            assert_eq!(c.disc_mode, DiscMode::Idle);
            assert!(!c.disc_read_enabled);
            assert!(!c.stop_after_next_sector);
        }
    }

    #[test]
    fn mode2_eof_event_ends_the_read() {
        let mut c = Cdic::new();
        c.file = 7 << 8;
        c.audio_channel = 1 << 3;
        c.disc_command = 0x2A;

        assert!(c.is_mode2_sector_selected(&mode2_sector(7, 4, SUBMODE_EOF)));
        assert_eq!(c.disc_command, 0);
    }
}
