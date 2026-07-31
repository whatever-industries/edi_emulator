// SPDX-License-Identifier: GPL-3.0-or-later
//! Optional CD-i Digital Video Cartridge support.
//!
//! M3 targets the Philips 22ER9141 VMPEG cartridge: MCD251 video decoding,
//! a DSP56001-facing MPEG Layer-II audio block, 1 MiB extension RAM, and
//! 512 KiB decoder RAM.  The device firmware executes natively on the host
//! SCC68070; this module models the cartridge-visible memory, registers,
//! DMA endpoint, clocks, and MPEG system-stream boundary.

use std::collections::VecDeque;

use crate::mpeg1_video::{DecodedVideoFrame, Mpeg1VideoDecoder};

const VMPEG_SPLIT_ROM_SIZE: usize = 128 * 1024;
const VMPEG_FULL_ROM_SIZE: usize = 256 * 1024;
const EXTENSION_RAM_SIZE: usize = 1024 * 1024;
const DECODE_RAM_SIZE: usize = 512 * 1024;
const MPEG_INPUT_LIMIT: usize = 512 * 1024;
const CLOCK_HZ: u64 = 15_000_000;
const DCLK_HZ: u64 = 45_000;
const FMV_TIMER_HZ: u64 = 5_625;
const MPEG_TIMESTAMP_MODULUS: i64 = 1 << 33;

const FMV_IER: usize = 0x60;
const FMV_ISR: usize = 0x62;
const FMV_TIMER: usize = 0x64;
const FMV_SYSCMD: usize = 0xC0;
const FMV_VIDCMD: usize = 0xC2;
const FMV_STREAM: usize = 0xC4;
const FMV_IVEC: usize = 0xDC;
const FMV_XFER: usize = 0xDE;

const FMA_CMD: usize = 0x00;
const FMA_STREAM: usize = 0x08;
const FMA_IVEC: usize = 0x0C;
const FMA_ISR: usize = 0x1A;
const FMA_IER: usize = 0x1C;

/// A latched MCD251 output view. VMPEG emits 384 active samples at the
/// Green Book 15 MHz rate, or stretches 352 VCD samples to that span when
/// its 13.5 MHz sample-rate converter is selected.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ExternalVideo<'a> {
    pub(crate) frame: &'a DecodedVideoFrame,
    pub(crate) display_x: usize,
    pub(crate) display_y: usize,
    pub(crate) window_x: usize,
    pub(crate) window_y: usize,
    pub(crate) window_width: usize,
    pub(crate) window_height: usize,
    pub(crate) vcd_clock: bool,
    pub(crate) border: u32,
}

impl ExternalVideo<'_> {
    /// Sample one pixel in the MCD212's 768x280 logical active raster.
    pub(crate) fn pixel(self, raster_x: usize, raster_y: usize) -> u32 {
        if raster_y < self.display_y {
            return self.border;
        }
        let source_y = raster_y - self.display_y + self.window_y;
        if source_y >= self.window_y.saturating_add(self.window_height)
            || source_y >= self.frame.height
        {
            return self.border;
        }

        // X-display is expressed in 15 MHz output-sample positions. The
        // MCD212 framebuffer follows C2PIX at twice that rate, so each
        // position occupies two 30 MHz raster pixels. The White Book sample
        // rate converter changes the MPEG output from 15 MHz to 13.5 MHz
        // (Philips Interactive Engineer 96/05), hence 9 source samples per
        // 20 framebuffer pixels.
        let display_x = self.display_x.saturating_mul(2);
        if raster_x < display_x {
            return self.border;
        }
        let relative_x = raster_x - display_x;
        let source_relative_x = if self.vcd_clock {
            relative_x.saturating_mul(9) / 20
        } else {
            relative_x / 2
        };
        if source_relative_x >= self.window_width {
            return self.border;
        }
        let source_x = self.window_x.saturating_add(source_relative_x);
        if source_x >= self.frame.width {
            return self.border;
        }
        self.frame.pixels[source_y * self.frame.width + source_x]
    }
}

/// Supported/recognized Digital Video Cartridge chipset families.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DvcKind {
    Vmpeg,
    /// Recognized for diagnostics, but emulation is deferred to M4.
    Impeg,
}

impl DvcKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Vmpeg => "VMPEG",
            Self::Impeg => "IMPEG",
        }
    }
}

/// Firmware/configuration used to attach an optional DVC.
#[derive(Debug, Clone)]
pub struct DvcConfig {
    pub kind: DvcKind,
    pub rom: Vec<u8>,
}

impl DvcConfig {
    pub fn new(kind: DvcKind, rom: Vec<u8>) -> Result<Self, String> {
        match kind {
            DvcKind::Vmpeg if !matches!(rom.len(), VMPEG_SPLIT_ROM_SIZE | VMPEG_FULL_ROM_SIZE) => {
                return Err(format!(
                    "VMPEG firmware must be 128 or 256 KiB, got {} bytes",
                    rom.len()
                ));
            }
            DvcKind::Impeg if rom.len() != VMPEG_FULL_ROM_SIZE => {
                return Err(format!(
                    "IMPEG firmware must be 256 KiB, got {} bytes",
                    rom.len()
                ));
            }
            _ => {}
        }
        Ok(Self { kind, rom })
    }

    /// Inspect OS-9 module signatures and construct a typed DVC config.
    pub fn from_rom(rom: Vec<u8>) -> Result<Self, String> {
        let modules = cdi_os9::scan_modules(&rom);
        let kind = match cdi_os9::identify_dvc_rom(&modules) {
            cdi_os9::DvcRomType::Vmpeg => DvcKind::Vmpeg,
            cdi_os9::DvcRomType::Impeg => DvcKind::Impeg,
            cdi_os9::DvcRomType::Unknown => {
                return Err("ROM has no recognized VMPEG/IMPEG firmware signature".into());
            }
        };
        Self::new(kind, rom)
    }
}

/// Counters exposed to headless/front-end diagnostics.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct DvcStats {
    pub dma_words: u64,
    pub direct_words: u64,
    pub system_packs: u64,
    pub video_pes_packets: u64,
    pub audio_pes_packets: u64,
    pub video_bytes: u64,
    pub audio_bytes: u64,
    pub decoded_video_frames: u64,
    pub decoded_audio_frames: u64,
    pub presented_video_frames: u64,
    pub video_update_events: u64,
    pub pause_events: u64,
    pub play_events: u64,
    pub continue_events: u64,
    pub audio_start_events: u64,
    pub audio_stop_events: u64,
    pub audio_underflow_events: u64,
    pub video_underflow_events: u64,
    pub audio_samples_discarded: u64,
    pub sequence_events: u64,
    pub group_events: u64,
    pub sequence_end_events: u64,
    pub end_of_data_events: u64,
    pub program_end_events: u64,
    pub video_program_end_events: u64,
    pub audio_program_end_events: u64,
    pub fma_intacks: u64,
    pub fmv_intacks: u64,
    pub demux_errors: u64,
    pub video_errors: u64,
    pub audio_errors: u64,
    /// Bytes skipped while acquiring the first valid Layer-II frame sync.
    /// Beginning playback in the middle of an audio frame is legal and is
    /// therefore not counted as a malformed-stream error.
    pub audio_resync_bytes: u64,
    pub queued_video_frames: u64,
    pub queued_audio_samples: u64,
    pub playing: u64,
    pub video_visible: u64,
    pub stream_errors: u64,
    pub current_video_width: u64,
    pub current_video_height: u64,
    pub current_video_rgb_min: u64,
    pub current_video_rgb_max: u64,
    pub current_video_frame_hash: u64,
    pub video_offset_x: u64,
    pub video_offset_y: u64,
    pub video_active_x: u64,
    pub video_active_y: u64,
    pub video_display_x: u64,
    pub video_display_y: u64,
    pub video_window_x: u64,
    pub video_window_y: u64,
    pub video_window_width: u64,
    pub video_window_height: u64,
    pub vcd_pixel_clock_13_5: u64,
}

/// Read-only VMPEG register/state snapshot for deterministic diagnostics.
///
/// Values are sampled without invoking the register read side effects used by
/// the native driver (notably ISR acknowledge-on-read).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct DvcRegisterSnapshot {
    pub fma_command: u16,
    pub fma_status: u16,
    pub fma_stream: u16,
    pub fma_vector: u16,
    pub fma_isr: u16,
    pub fma_ier: u16,
    pub fmv_system_status: u16,
    pub fmv_ier: u16,
    pub fmv_isr: u16,
    pub fmv_timer: u16,
    pub fmv_decoder_command: u16,
    pub fmv_video_data_command: u16,
    pub fmv_decoding_timestamp: u16,
    pub fmv_pictures_in_fifo: u16,
    pub fmv_system_command: u16,
    pub fmv_video_command: u16,
    pub fmv_stream: u16,
    pub fmv_vector: u16,
    pub dclk: u32,
    pub timer_counter: u16,
    pub dma_target: u8,
    pub decoder_enabled: bool,
    pub video_armed: bool,
    pub playing: bool,
    pub video_visible: bool,
    pub video_underflow_reported: bool,
    pub video_sequence_end_seen: bool,
    pub video_iso_end_pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DmaTarget {
    Video,
    Audio,
}

#[derive(Debug, Default)]
struct DemuxStats {
    packs: u64,
    video_packets: u64,
    audio_packets: u64,
    video_bytes: u64,
    audio_bytes: u64,
    program_ends: u64,
    errors: u64,
}

/// Incremental MPEG-1 system/PES parser. It deliberately stops at elementary
/// streams; codec decode is layered above this boundary.
#[derive(Debug, Default)]
struct MpegSystemDemux {
    pending: Vec<u8>,
    video: VecDeque<u8>,
    audio: VecDeque<u8>,
    selected_video_stream: Option<u8>,
    selected_audio_stream: Option<u8>,
    last_scr: Option<u64>,
    last_video_pts: Option<u64>,
    last_video_dts: Option<u64>,
    last_audio_pts: Option<u64>,
    stats: DemuxStats,
}

impl MpegSystemDemux {
    fn clear(&mut self) {
        *self = Self::default();
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        self.parse_available();
    }

    fn find_prefix(bytes: &[u8]) -> Option<usize> {
        bytes.windows(3).position(|w| w == [0, 0, 1])
    }

    fn parse_available(&mut self) {
        loop {
            let Some(prefix) = Self::find_prefix(&self.pending) else {
                if self.pending.len() > 2 {
                    let keep = self.pending.split_off(self.pending.len() - 2);
                    self.pending = keep;
                    self.stats.errors += 1;
                }
                return;
            };
            if prefix != 0 {
                self.pending.drain(..prefix);
                self.stats.errors += 1;
            }
            if self.pending.len() < 4 {
                return;
            }

            let stream_id = self.pending[3];
            match stream_id {
                0xBA => {
                    let Some(size) = Self::pack_header_size(&self.pending) else {
                        return;
                    };
                    if self.pending[4] & 0xF0 == 0x20 {
                        self.last_scr = Some(Self::decode_mpeg1_scr(&self.pending[4..9]));
                    }
                    self.pending.drain(..size);
                    self.stats.packs += 1;
                }
                0xB9 => {
                    self.pending.drain(..4);
                    self.stats.program_ends += 1;
                }
                _ => {
                    if self.pending.len() < 6 {
                        return;
                    }
                    let payload_len =
                        usize::from(u16::from_be_bytes([self.pending[4], self.pending[5]]));
                    if payload_len == 0 {
                        // MPEG-1 CD-i packs use bounded PES packets. Recover
                        // from an unexpected unbounded packet at the next
                        // start code instead of growing forever.
                        let Some(next) = Self::find_prefix(&self.pending[4..]).map(|v| v + 4)
                        else {
                            return;
                        };
                        self.pending.drain(..next);
                        self.stats.errors += 1;
                        continue;
                    }
                    let packet_len = 6 + payload_len;
                    if self.pending.len() < packet_len {
                        return;
                    }
                    let packet: Vec<u8> = self.pending.drain(..packet_len).collect();
                    self.consume_packet(stream_id, &packet);
                }
            }
        }
    }

    fn pack_header_size(bytes: &[u8]) -> Option<usize> {
        if bytes.len() < 12 {
            return None;
        }
        // MPEG-1 pack headers begin `0010`; MPEG-2 begins `01` and adds a
        // stuffing-length field. CD-i is MPEG-1, but accepting both makes
        // malformed media recovery deterministic.
        if bytes[4] & 0xF0 == 0x20 {
            Some(12)
        } else {
            if bytes.len() < 14 {
                return None;
            }
            let size = 14 + usize::from(bytes[13] & 7);
            (bytes.len() >= size).then_some(size)
        }
    }

    fn consume_packet(&mut self, stream_id: u8, packet: &[u8]) {
        // Non-PES system/private padding packets use the same length shape.
        if !(0xC0..=0xEF).contains(&stream_id) {
            return;
        }
        let Some((payload, pts, dts)) = Self::pes_payload(packet) else {
            self.stats.errors += 1;
            return;
        };
        if (0xE0..=0xEF).contains(&stream_id)
            && self
                .selected_video_stream
                .map_or(true, |selected| stream_id & 0x0F == selected)
        {
            self.video.extend(payload);
            self.stats.video_bytes += payload.len() as u64;
            if let Some(pts) = pts {
                self.last_video_pts = Some(pts);
            }
            if let Some(dts) = dts.or(pts) {
                self.last_video_dts = Some(dts);
            }
            self.stats.video_packets += 1;
        } else if (0xC0..=0xDF).contains(&stream_id)
            && self
                .selected_audio_stream
                .map_or(true, |selected| stream_id & 0x1F == selected)
        {
            self.audio.extend(payload);
            self.stats.audio_bytes += payload.len() as u64;
            if let Some(pts) = pts {
                self.last_audio_pts = Some(pts);
            }
            self.stats.audio_packets += 1;
        }
    }

    fn decode_mpeg1_scr(bytes: &[u8]) -> u64 {
        (u64::from(bytes[0] & 0x0E) << 29)
            | (u64::from(bytes[1]) << 22)
            | (u64::from(bytes[2] & 0xFE) << 14)
            | (u64::from(bytes[3]) << 7)
            | u64::from(bytes[4] >> 1)
    }

    fn pes_payload(packet: &[u8]) -> Option<(&[u8], Option<u64>, Option<u64>)> {
        let mut at = 6usize;
        while packet.get(at) == Some(&0xFF) {
            at += 1;
        }
        if packet.get(at).is_some_and(|b| b & 0xC0 == 0x40) {
            at += 2;
        }

        let marker = *packet.get(at)?;
        let mut pts = None;
        let mut dts = None;
        if marker & 0xC0 == 0x80 {
            // MPEG-2 PES header, accepted for recovery/testing.
            let flags = *packet.get(at + 1)?;
            let header_len = usize::from(*packet.get(at + 2)?);
            if flags & 0x80 != 0 && header_len >= 5 {
                pts = Self::decode_pts(packet.get(at + 3..at + 8)?);
            }
            if flags & 0x40 != 0 && header_len >= 10 {
                dts = Self::decode_pts(packet.get(at + 8..at + 13)?);
            }
            at += 3 + header_len;
        } else {
            match marker >> 4 {
                0x2 => {
                    pts = Self::decode_pts(packet.get(at..at + 5)?);
                    at += 5;
                }
                0x3 => {
                    pts = Self::decode_pts(packet.get(at..at + 5)?);
                    dts = Self::decode_pts(packet.get(at + 5..at + 10)?);
                    at += 10;
                }
                _ if marker == 0x0F => at += 1,
                _ => return None,
            }
        }
        packet.get(at..).map(|payload| (payload, pts, dts))
    }

    fn decode_pts(bytes: &[u8]) -> Option<u64> {
        if bytes.len() < 5 || bytes[0] & 1 == 0 || bytes[2] & 1 == 0 || bytes[4] & 1 == 0 {
            return None;
        }
        Some(
            (u64::from((bytes[0] >> 1) & 7) << 30)
                | (u64::from(bytes[1]) << 22)
                | (u64::from(bytes[2] >> 1) << 15)
                | (u64::from(bytes[3]) << 7)
                | u64::from(bytes[4] >> 1),
        )
    }
}

#[derive(Debug, Default)]
struct Mp2Decoder {
    state: oxideav_mp2::frame::FrameDecodeState,
    pcm: VecDeque<i16>,
    errors: u64,
    resync_bytes: u64,
    synchronized: bool,
}

impl Mp2Decoder {
    fn reset(&mut self) {
        self.state.reset();
        self.pcm.clear();
        self.errors = 0;
        self.resync_bytes = 0;
        self.synchronized = false;
    }

    fn decode_available(&mut self, input: &mut VecDeque<u8>) -> u64 {
        let mut decoded = 0;
        loop {
            if input.len() < 4 {
                break;
            }
            let head = [input[0], input[1], input[2], input[3]];
            let header = match oxideav_mp2::header::FrameHeader::parse(&head) {
                Ok(header) => header,
                Err(_) => {
                    input.pop_front();
                    if self.synchronized {
                        self.errors += 1;
                        self.synchronized = false;
                    } else {
                        self.resync_bytes += 1;
                    }
                    continue;
                }
            };
            let frame_size = header.frame_size_bytes();
            if input.len() < frame_size {
                break;
            }
            let contiguous = input.make_contiguous();
            match oxideav_mp2::frame::decode_frame_with(&contiguous[..frame_size], &mut self.state)
            {
                Ok(frame) => {
                    Self::append_resampled(&mut self.pcm, &frame.pcm, frame.header.sample_rate);
                    input.drain(..frame_size);
                    decoded += 1;
                    self.synchronized = true;
                }
                Err(error) => {
                    log::debug!("vmpeg: malformed MP2 frame: {error}");
                    input.pop_front();
                    self.state.reset();
                    self.errors += 1;
                    self.synchronized = false;
                }
            }
        }
        decoded
    }

    fn append_resampled(output: &mut VecDeque<i16>, channels: &[Vec<f64>], sample_rate: u32) {
        let Some(left) = channels.first() else {
            return;
        };
        let right = channels.get(1).unwrap_or(left);
        let out_len = ((left.len() as u64 * 44_100 + u64::from(sample_rate) / 2)
            / u64::from(sample_rate)) as usize;
        for out_index in 0..out_len {
            let source_pos = out_index as f64 * f64::from(sample_rate) / 44_100.0;
            let index = source_pos.floor() as usize;
            let fraction = source_pos - index as f64;
            let next = (index + 1).min(left.len() - 1);
            for channel in [left, right] {
                let sample = channel[index] + (channel[next] - channel[index]) * fraction;
                output.push_back((sample * 32_768.0).clamp(-32_768.0, 32_767.0) as i16);
            }
        }
    }
}

/// VMPEG cartridge state attached to the host bus.
#[derive(Debug)]
pub struct Vmpeg {
    firmware: Vec<u8>,
    extension_ram: Vec<u8>,
    decode_ram: Vec<u8>,
    decode_ram_visible: bool,
    register_writes: u8,
    vcd_pixel_clock_13_5: bool,
    fma_regs: Vec<u8>,
    fmv_regs: Vec<u8>,
    fma_read_counts: Vec<u64>,
    fmv_read_counts: Vec<u64>,
    dclk: u32,
    dclk_accum: u64,
    timer_accum: u64,
    timer_counter: u16,
    dma_target: Option<DmaTarget>,
    decoder_enabled: bool,
    audio_armed: bool,
    audio_enabled: bool,
    audio_start_dclk: Option<u32>,
    audio_clock_anchor: Option<(u64, u32)>,
    audio_underflow_reported: bool,
    audio_underflow_after_eoi_ack: bool,
    video_armed: bool,
    playing: bool,
    video_visible: bool,
    video_update_pending: bool,
    video_update_scroll: bool,
    video_update_cycles: u64,
    play_start_dclk: Option<u32>,
    video_clock_anchor: Option<(u64, u32)>,
    pause_irq_dclk: Option<u32>,
    video_input: Vec<u8>,
    audio_input: Vec<u8>,
    video_demux: MpegSystemDemux,
    audio_demux: MpegSystemDemux,
    mp2: Mp2Decoder,
    audio_sample_accum: u64,
    video_decoder: Mpeg1VideoDecoder,
    video_frames: VecDeque<DecodedVideoFrame>,
    current_video_frame: Option<DecodedVideoFrame>,
    video_cycle_accum: u64,
    video_cycles_per_frame: u64,
    last_picture_due_dclk: Option<u32>,
    video_underflow_reported: bool,
    video_sequence_end_seen: bool,
    video_iso_end_reported: bool,
    audio_iso_end_reported: bool,
    video_iso_end_pending: bool,
    video_underflow_after_eoi_ack: bool,
    demux_errors_before_reset: u64,
    video_errors_before_reset: u64,
    audio_errors_before_reset: u64,
    audio_resync_bytes_before_reset: u64,
    captured_video_es: Option<Vec<u8>>,
    stats: DvcStats,
    audio_out: Vec<i16>,
}

impl Vmpeg {
    pub fn new(config: DvcConfig) -> Result<Self, String> {
        if config.kind != DvcKind::Vmpeg {
            return Err(format!(
                "{} DVC firmware is recognized but emulation is deferred to M4",
                config.kind.name()
            ));
        }
        let mut firmware = config.rom;
        if firmware.len() == VMPEG_SPLIT_ROM_SIZE {
            firmware.extend_from_within(..);
        }
        if firmware.len() != VMPEG_FULL_ROM_SIZE {
            return Err(format!(
                "VMPEG firmware must map to 256 KiB, got {} bytes",
                firmware.len()
            ));
        }
        let mut dvc = Self {
            firmware,
            extension_ram: vec![0; EXTENSION_RAM_SIZE],
            decode_ram: vec![0; DECODE_RAM_SIZE],
            decode_ram_visible: false,
            register_writes: 0,
            vcd_pixel_clock_13_5: false,
            fma_regs: vec![0; 0x100],
            fmv_regs: vec![0; 0x100],
            fma_read_counts: vec![0; 0x80],
            fmv_read_counts: vec![0; 0x80],
            dclk: 0,
            dclk_accum: 0,
            timer_accum: 0,
            timer_counter: 0,
            dma_target: None,
            decoder_enabled: false,
            audio_armed: false,
            audio_enabled: false,
            audio_start_dclk: None,
            audio_clock_anchor: None,
            audio_underflow_reported: false,
            audio_underflow_after_eoi_ack: false,
            video_armed: false,
            playing: false,
            video_visible: false,
            video_update_pending: false,
            video_update_scroll: false,
            video_update_cycles: 0,
            play_start_dclk: None,
            video_clock_anchor: None,
            pause_irq_dclk: None,
            video_input: Vec::new(),
            audio_input: Vec::new(),
            video_demux: MpegSystemDemux::default(),
            audio_demux: MpegSystemDemux::default(),
            mp2: Mp2Decoder::default(),
            audio_sample_accum: 0,
            video_decoder: Mpeg1VideoDecoder::default(),
            video_frames: VecDeque::new(),
            current_video_frame: None,
            video_cycle_accum: 0,
            video_cycles_per_frame: CLOCK_HZ / 25,
            last_picture_due_dclk: None,
            video_underflow_reported: false,
            video_sequence_end_seen: false,
            video_iso_end_reported: false,
            audio_iso_end_reported: false,
            video_iso_end_pending: false,
            video_underflow_after_eoi_ack: false,
            demux_errors_before_reset: 0,
            video_errors_before_reset: 0,
            audio_errors_before_reset: 0,
            audio_resync_bytes_before_reset: 0,
            captured_video_es: None,
            stats: DvcStats::default(),
            audio_out: Vec::new(),
        };
        dvc.reset();
        Ok(dvc)
    }

    /// Reset cartridge registers while retaining extension/decode RAM.
    pub fn reset(&mut self) {
        self.demux_errors_before_reset = self
            .demux_errors_before_reset
            .saturating_add(self.video_demux.stats.errors + self.audio_demux.stats.errors);
        self.video_errors_before_reset = self
            .video_errors_before_reset
            .saturating_add(self.video_decoder.errors);
        self.audio_errors_before_reset = self
            .audio_errors_before_reset
            .saturating_add(self.mp2.errors);
        self.audio_resync_bytes_before_reset = self
            .audio_resync_bytes_before_reset
            .saturating_add(self.mp2.resync_bytes);
        self.fma_regs.fill(0);
        self.fmv_regs.fill(0);
        self.fma_read_counts.fill(0);
        self.fmv_read_counts.fill(0);
        Self::set_word(&mut self.fma_regs, 0x02, 0x0200);
        Self::set_word(&mut self.fma_regs, 0x04, 0x0007);
        Self::set_word(&mut self.fma_regs, 0x06, 0x0900);
        Self::set_word(&mut self.fma_regs, 0x0E, 0x0042);
        Self::set_word(&mut self.fma_regs, 0x24, 0x0004);
        Self::set_word(&mut self.fmv_regs, FMV_TIMER, 55);
        Self::set_word(&mut self.fmv_regs, 0x5E, 0x2000);
        Self::set_word(&mut self.fmv_regs, 0x9E, 0xFE96);
        self.decode_ram_visible = false;
        self.register_writes = 0;
        self.dclk = 0;
        self.dclk_accum = 0;
        self.timer_accum = 0;
        self.timer_counter = 0;
        self.dma_target = None;
        self.decoder_enabled = false;
        self.audio_armed = false;
        self.audio_enabled = false;
        self.audio_start_dclk = None;
        self.audio_clock_anchor = None;
        self.audio_underflow_reported = false;
        self.audio_underflow_after_eoi_ack = false;
        self.video_armed = false;
        self.playing = false;
        self.video_visible = false;
        self.video_update_pending = false;
        self.video_update_scroll = false;
        self.video_update_cycles = 0;
        self.play_start_dclk = None;
        self.video_clock_anchor = None;
        self.pause_irq_dclk = None;
        self.video_input.clear();
        self.audio_input.clear();
        self.video_demux.clear();
        self.audio_demux.clear();
        self.mp2.reset();
        self.audio_sample_accum = 0;
        self.video_decoder.reset();
        self.video_frames.clear();
        self.current_video_frame = None;
        self.stats.current_video_width = 0;
        self.stats.current_video_height = 0;
        self.stats.current_video_rgb_min = 0;
        self.stats.current_video_rgb_max = 0;
        self.stats.current_video_frame_hash = 0;
        self.video_cycle_accum = 0;
        self.video_cycles_per_frame = CLOCK_HZ / 25;
        self.last_picture_due_dclk = None;
        self.video_underflow_reported = false;
        self.video_sequence_end_seen = false;
        self.video_iso_end_reported = false;
        self.audio_iso_end_reported = false;
        self.video_iso_end_pending = false;
        self.video_underflow_after_eoi_ack = false;
        self.audio_out.clear();
    }

    /// Remove power from the cartridge while retaining its firmware ROM.
    pub fn power_cycle(&mut self) {
        let firmware = self.firmware.clone();
        *self = Self::new(DvcConfig {
            kind: DvcKind::Vmpeg,
            rom: firmware,
        })
        .expect("an attached VMPEG firmware image remains valid");
    }

    pub fn stats(&self) -> DvcStats {
        let mut stats = self.stats;
        stats.queued_video_frames = self.video_frames.len() as u64;
        stats.queued_audio_samples = self.mp2.pcm.len() as u64 / 2;
        stats.playing = u64::from(self.playing);
        stats.video_visible = u64::from(self.video_visible);
        // Motorola MCD251 Technical Summary register map: Yo/Xo/Ya/Xa.
        // These are exposed for provenance only; the available summary does
        // not define enough timing semantics to apply them to presentation.
        stats.video_offset_x = u64::from(Self::word(&self.fmv_regs, 0x6E));
        stats.video_offset_y = u64::from(Self::word(&self.fmv_regs, 0x6C));
        stats.video_active_x = u64::from(Self::word(&self.fmv_regs, 0x72));
        stats.video_active_y = u64::from(Self::word(&self.fmv_regs, 0x70));
        stats.video_display_x = u64::from(Self::word(&self.fmv_regs, 0x76));
        stats.video_display_y = u64::from(Self::word(&self.fmv_regs, 0x74));
        stats.video_window_x = u64::from(Self::word(&self.fmv_regs, 0x7E));
        stats.video_window_y = u64::from(Self::word(&self.fmv_regs, 0x7C));
        stats.video_window_width = u64::from(Self::word(&self.fmv_regs, 0x7A));
        stats.video_window_height = u64::from(Self::word(&self.fmv_regs, 0x78));
        stats.vcd_pixel_clock_13_5 = u64::from(self.vcd_pixel_clock_13_5);
        stats
    }

    pub fn register_snapshot(&self) -> DvcRegisterSnapshot {
        DvcRegisterSnapshot {
            fma_command: Self::word(&self.fma_regs, FMA_CMD),
            fma_status: Self::word(&self.fma_regs, 0x02),
            fma_stream: Self::word(&self.fma_regs, FMA_STREAM),
            fma_vector: Self::word(&self.fma_regs, FMA_IVEC),
            fma_isr: Self::word(&self.fma_regs, FMA_ISR),
            fma_ier: Self::word(&self.fma_regs, FMA_IER),
            fmv_system_status: Self::word(&self.fmv_regs, 0x5E),
            fmv_ier: Self::word(&self.fmv_regs, FMV_IER),
            fmv_isr: Self::word(&self.fmv_regs, FMV_ISR),
            fmv_timer: Self::word(&self.fmv_regs, FMV_TIMER),
            fmv_decoder_command: Self::word(&self.fmv_regs, 0x88),
            fmv_video_data_command: Self::word(&self.fmv_regs, 0x8C),
            fmv_decoding_timestamp: Self::word(&self.fmv_regs, 0xA0),
            fmv_pictures_in_fifo: Self::word(&self.fmv_regs, 0xA4),
            fmv_system_command: Self::word(&self.fmv_regs, FMV_SYSCMD),
            fmv_video_command: Self::word(&self.fmv_regs, FMV_VIDCMD),
            fmv_stream: Self::word(&self.fmv_regs, FMV_STREAM),
            fmv_vector: Self::word(&self.fmv_regs, FMV_IVEC),
            dclk: self.dclk,
            timer_counter: self.timer_counter,
            dma_target: match self.dma_target {
                None => 0,
                Some(DmaTarget::Video) => 1,
                Some(DmaTarget::Audio) => 2,
            },
            decoder_enabled: self.decoder_enabled,
            video_armed: self.video_armed,
            playing: self.playing,
            video_visible: self.video_visible,
            video_underflow_reported: self.video_underflow_reported,
            video_sequence_end_seen: self.video_sequence_end_seen,
            video_iso_end_pending: self.video_iso_end_pending,
        }
    }

    /// Enable or disable capture of the current VMPEG play's elementary
    /// video stream. Capture is kept in memory so the deterministic core does
    /// not perform host I/O; a debugger or CLI may persist it after a run.
    pub fn set_video_es_capture(&mut self, enabled: bool) {
        self.captured_video_es = enabled.then(Vec::new);
    }

    pub fn captured_video_es(&self) -> Option<&[u8]> {
        self.captured_video_es.as_deref()
    }

    /// Read-only native-driver RAM for local diagnostic capture.
    pub fn extension_ram(&self) -> &[u8] {
        &self.extension_ram
    }

    pub fn irq(&self) -> bool {
        Self::word(&self.fma_regs, FMA_ISR) & Self::word(&self.fma_regs, FMA_IER) != 0
            || Self::word(&self.fmv_regs, FMV_ISR) & Self::word(&self.fmv_regs, FMV_IER) != 0
    }

    pub fn intack(&mut self) -> u8 {
        if Self::word(&self.fma_regs, FMA_ISR) & Self::word(&self.fma_regs, FMA_IER) != 0 {
            self.stats.fma_intacks += 1;
            Self::word(&self.fma_regs, FMA_IVEC) as u8
        } else {
            self.stats.fmv_intacks += 1;
            ((Self::word(&self.fmv_regs, FMV_IVEC) >> 3) & 0xFF) as u8
        }
    }

    pub fn dma_requested(&self) -> bool {
        self.dma_target.is_some()
            && match self.dma_target {
                Some(DmaTarget::Video) => self.video_input.len() + 2 <= MPEG_INPUT_LIMIT,
                Some(DmaTarget::Audio) => self.audio_input.len() + 2 <= MPEG_INPUT_LIMIT,
                None => false,
            }
    }

    pub fn push_dma_word(&mut self, word: u16) {
        let bytes = word.to_be_bytes();
        match self.dma_target {
            Some(DmaTarget::Video) => self.video_input.extend_from_slice(&bytes),
            Some(DmaTarget::Audio) => self.audio_input.extend_from_slice(&bytes),
            None => return,
        }
        self.stats.dma_words += 1;
    }

    pub fn finish_dma(&mut self) {
        match self.dma_target.take() {
            Some(DmaTarget::Video) => {
                let program_ends = self.video_demux.stats.program_ends;
                let video_packets = self.video_demux.stats.video_packets;
                let input_len = self.video_input.len();
                let iso_end_offset = self
                    .video_input
                    .windows(4)
                    .position(|bytes| bytes == [0, 0, 1, 0xB9]);
                let sequence_ends = self.video_decoder.sequence_ends;
                let previous_dts = self.video_demux.last_video_dts;
                self.video_demux.selected_video_stream =
                    Some((Self::word(&self.fmv_regs, FMV_STREAM) & 0x0F) as u8);
                self.video_demux.feed(&self.video_input);
                log::debug!(
                    "vmpeg: video DMA completed: {input_len} bytes, stream {}, {} PES packet(s)",
                    Self::word(&self.fmv_regs, FMV_STREAM) & 0x0F,
                    self.video_demux
                        .stats
                        .video_packets
                        .saturating_sub(video_packets),
                );
                self.video_input.clear();
                if self.video_armed {
                    if let Some(scr) = self.video_demux.last_scr {
                        if self.video_clock_anchor.is_none() {
                            self.video_clock_anchor = Some((scr, self.dclk));
                            log::debug!(
                                "vmpeg: video clock anchored at DCLK {} / SCR {scr}",
                                self.dclk
                            );
                        }
                    }
                }
                self.schedule_video_start();
                if self.video_demux.last_video_dts != previous_dts {
                    if let Some(dts) = self.video_demux.last_video_dts {
                        // The MCD251 exposes the low 15 bits of DTS at
                        // 90 kHz / 128 = 703.125 Hz and latches VDI bit 14
                        // whenever a new decoding timestamp is available.
                        Self::set_word(&mut self.fmv_regs, 0xA0, ((dts >> 7) & 0x7FFF) as u16);
                        Self::or_word(&mut self.fmv_regs, 0x8C, 0x4000);
                    }
                }
                let elementary: Vec<u8> = self.video_demux.video.drain(..).collect();
                if let Some(capture) = &mut self.captured_video_es {
                    capture.extend_from_slice(&elementary);
                }
                self.video_decoder.feed(&elementary);
                let frames = self.video_decoder.decode_available(64);
                if self.video_decoder.sequence_ends != sequence_ends {
                    Self::or_word(&mut self.fmv_regs, FMV_ISR, 0x0200);
                    self.video_sequence_end_seen = true;
                    self.last_picture_due_dclk = self.video_timestamp_deadline();
                    self.stats.sequence_end_events += 1;
                    log::debug!(
                        "vmpeg: video sequence end at DCLK {}, last picture due {:?} (SCR {:?}, PTS {:?})",
                        self.dclk,
                        self.last_picture_due_dclk,
                        self.video_demux.last_scr,
                        self.video_demux.last_video_pts
                    );
                }
                if self.video_demux.stats.program_ends != program_ends {
                    log::debug!(
                        "vmpeg: video DMA contained ISO end at {iso_end_offset:?} of {} bytes",
                        input_len
                    );
                    self.signal_video_program_end();
                }
                if !frames.is_empty() {
                    if self.video_armed && self.play_start_dclk.is_none() {
                        // Timestamp-less streams still need a small decoder
                        // priming delay. Timestamped streams use their actual
                        // SCR-to-PTS lead in `schedule_video_start`.
                        self.play_start_dclk = Some(self.dclk.wrapping_add(3_000));
                    }
                    let rate = self.video_decoder.frame_rate();
                    if rate > 0.0 {
                        self.video_cycles_per_frame =
                            (CLOCK_HZ as f64 / rate).round().max(1.0) as u64;
                    }
                    let first = &frames[0];
                    Self::set_word(&mut self.fmv_regs, 0x02, first.width as u16);
                    Self::set_word(&mut self.fmv_regs, 0x04, first.height as u16);
                    Self::set_word(
                        &mut self.fmv_regs,
                        0x06,
                        u16::from(self.video_decoder.sequence_prpa()),
                    );
                    if rate > 0.0 {
                        Self::set_word(&mut self.fmv_regs, 0xA8, (90_000.0 / rate).round() as u16);
                    }
                    self.stats.decoded_video_frames += frames.len() as u64;
                    self.video_frames.extend(frames);
                    self.prime_video_output();
                    self.video_underflow_reported = false;
                }
                let command = Self::word(&self.fmv_regs, FMV_SYSCMD) & !0x8000;
                Self::set_word(&mut self.fmv_regs, FMV_SYSCMD, command);
            }
            Some(DmaTarget::Audio) => {
                let program_ends = self.audio_demux.stats.program_ends;
                let input_len = self.audio_input.len();
                let pending_tail = self
                    .audio_demux
                    .pending
                    .iter()
                    .rev()
                    .take(8)
                    .copied()
                    .collect::<Vec<_>>();
                let input_head = self.audio_input.iter().take(8).copied().collect::<Vec<_>>();
                let iso_end_offset = self
                    .audio_input
                    .windows(4)
                    .position(|bytes| bytes == [0, 0, 1, 0xB9]);
                self.audio_demux.selected_audio_stream =
                    Some((Self::word(&self.fma_regs, FMA_STREAM) & 0x1F) as u8);
                self.audio_demux.feed(&self.audio_input);
                self.audio_input.clear();
                if self.audio_armed {
                    if let Some(scr) = self.audio_demux.last_scr {
                        if self.audio_clock_anchor.is_none() {
                            self.audio_clock_anchor = Some((scr, self.dclk));
                            log::debug!(
                                "vmpeg: audio clock anchored at DCLK {} / SCR {scr}",
                                self.dclk
                            );
                        }
                    }
                }
                if self.audio_demux.stats.program_ends != program_ends {
                    log::debug!(
                        "vmpeg: audio DMA contained ISO end at {iso_end_offset:?} of {input_len} bytes; pending tail(rev) {pending_tail:02x?}, input head {input_head:02x?}"
                    );
                    self.signal_audio_program_end();
                }
                let decoded = self.mp2.decode_available(&mut self.audio_demux.audio);
                self.stats.decoded_audio_frames += decoded;
                if decoded != 0 && self.audio_armed && self.audio_start_dclk.is_none() {
                    if let (Some(scr), Some(pts)) =
                        (self.audio_demux.last_scr, self.audio_demux.last_audio_pts)
                    {
                        // MPEG timestamps use 90 kHz; FMA DCLK uses 45 kHz.
                        // Keep the mapping established by the first accepted
                        // pack. Later packs can arrive well ahead of their SCR.
                        let (anchor_scr, anchor_dclk) =
                            *self.audio_clock_anchor.get_or_insert((scr, self.dclk));
                        self.audio_start_dclk = Some(Self::anchored_timestamp_deadline(
                            anchor_dclk,
                            anchor_scr,
                            pts,
                        ));
                    }
                }
                let command = Self::word(&self.fma_regs, FMA_CMD) & !0x8000;
                Self::set_word(&mut self.fma_regs, FMA_CMD, command);
                if decoded != 0 {
                    Self::or_word(&mut self.fma_regs, 0x02, 0x0004);
                    Self::or_word(&mut self.fma_regs, FMA_ISR, 0x0004);
                    self.audio_underflow_reported = false;
                }
            }
            None => return,
        }
        self.sync_stats();
    }

    pub fn tick(&mut self, cycles: u64) {
        self.dclk_accum += cycles * DCLK_HZ;
        while self.dclk_accum >= CLOCK_HZ {
            self.dclk_accum -= CLOCK_HZ;
            self.dclk = self.dclk.wrapping_add(1);
        }

        if self.audio_armed
            && !self.audio_enabled
            && self
                .audio_start_dclk
                .is_some_and(|start| self.dclk.wrapping_sub(start) < 0x8000_0000)
        {
            self.audio_enabled = true;
            self.stats.audio_start_events += 1;
            Self::or_word(&mut self.fma_regs, 0x02, 0x0010);
            Self::or_word(&mut self.fma_regs, FMA_ISR, 0x0010);
        }

        if self
            .play_start_dclk
            .is_some_and(|start| self.dclk.wrapping_sub(start) < 0x8000_0000)
        {
            self.play_start_dclk = None;
            self.video_armed = false;
            self.playing = true;
            self.prime_video_output();
        }
        if self
            .pause_irq_dclk
            .is_some_and(|start| self.dclk.wrapping_sub(start) < 0x8000_0000)
        {
            self.pause_irq_dclk = None;
            self.stats.pause_events += 1;
            Self::or_word(&mut self.fmv_regs, FMV_ISR, 0x1000);
        }
        if self.video_update_pending && self.video_update_scroll {
            self.video_update_cycles = self.video_update_cycles.saturating_add(cycles);
            if self.video_update_cycles >= CLOCK_HZ / 50 {
                self.complete_video_update();
            }
        }

        self.timer_accum += cycles * FMV_TIMER_HZ;
        while self.timer_accum >= CLOCK_HZ {
            self.timer_accum -= CLOCK_HZ;
            let compare = Self::word(&self.fmv_regs, FMV_TIMER);
            if self.timer_counter >= compare {
                self.timer_counter = 0;
                Self::or_word(&mut self.fma_regs, FMA_ISR, 0x0100);
                Self::or_word(&mut self.fmv_regs, FMV_ISR, 0x0100);
            } else {
                self.timer_counter = self.timer_counter.wrapping_add(1);
            }
        }

        if self.audio_enabled {
            self.audio_sample_accum += cycles * 44_100;
            while self.audio_sample_accum >= CLOCK_HZ && self.mp2.pcm.len() >= 2 {
                self.audio_sample_accum -= CLOCK_HZ;
                self.audio_out.push(self.mp2.pcm.pop_front().unwrap_or(0));
                self.audio_out.push(self.mp2.pcm.pop_front().unwrap_or(0));
            }
            self.raise_pending_audio_underflow();
        }

        if self.playing {
            self.video_cycle_accum = self.video_cycle_accum.saturating_add(cycles);
            while self.video_cycle_accum >= self.video_cycles_per_frame {
                if self.last_picture_waiting_for_pts() {
                    // Retry on the next device tick without accumulating a
                    // large burst of overdue frame periods.
                    self.video_cycle_accum = self.video_cycles_per_frame;
                    break;
                }
                self.video_cycle_accum -= self.video_cycles_per_frame;
                if self.present_next_video_frame() {
                    self.video_underflow_reported = false;
                } else if self.stats.decoded_video_frames != 0 && !self.video_underflow_reported {
                    Self::or_word(&mut self.fmv_regs, FMV_ISR, 0x0020);
                    self.video_underflow_reported = true;
                    self.stats.video_underflow_events += 1;
                }
            }
        }
    }

    /// Latch the MCD251 vertical-sync status bit from the base player's
    /// display timing. This bit is observable even when it is masked out of
    /// the interrupt enable register: the native `MV_Release` path disables
    /// normal decoder interrupts, then polls ISR.VSYNC before releasing the
    /// device.
    pub(crate) fn notify_vsync(&mut self) {
        Self::or_word(&mut self.fmv_regs, FMV_ISR, 0x0800);
    }

    fn prime_video_output(&mut self) {
        if self.playing && self.video_visible && self.current_video_frame.is_none() {
            self.present_next_video_frame();
        }
    }

    fn schedule_video_start(&mut self) {
        if !self.video_armed || self.play_start_dclk.is_some() {
            return;
        }
        let (Some(scr), Some(pts)) = (self.video_demux.last_scr, self.video_demux.last_video_pts)
        else {
            return;
        };
        // MPEG system timestamps use 90 kHz while VMPEG's DCLK is 45 kHz.
        // The PTS-SCR lead is the decoder's presentation latency; presenting
        // immediately after DMA makes LPD occur before the disc-side EOR and
        // breaks titles which pipeline the next real-time record.
        let (anchor_scr, anchor_dclk) = *self.video_clock_anchor.get_or_insert((scr, self.dclk));
        self.play_start_dclk = Some(Self::anchored_timestamp_deadline(
            anchor_dclk,
            anchor_scr,
            pts,
        ));
        log::debug!(
            "vmpeg: video start scheduled at DCLK {} from SCR {scr} PTS {pts}",
            self.play_start_dclk.unwrap_or(self.dclk)
        );
    }

    fn present_next_video_frame(&mut self) -> bool {
        if self.last_picture_waiting_for_pts() {
            return false;
        }
        let Some(frame) = self.video_frames.pop_front() else {
            return false;
        };
        Self::set_word(&mut self.fmv_regs, 0x02, frame.width as u16);
        Self::set_word(&mut self.fmv_regs, 0x04, frame.height as u16);
        Self::set_word(&mut self.fmv_regs, 0x52, frame.width as u16);
        Self::set_word(&mut self.fmv_regs, 0x54, frame.height as u16);
        Self::set_word(
            &mut self.fmv_regs,
            0x56,
            u16::from(self.video_decoder.sequence_prpa()),
        );
        if frame.first_in_sequence {
            Self::or_word(&mut self.fmv_regs, FMV_ISR, 0x0001);
            self.stats.sequence_events += 1;
        }
        if frame.first_in_group {
            Self::or_word(&mut self.fmv_regs, FMV_ISR, 0x0002);
            self.stats.group_events += 1;
        }
        if frame.last_in_sequence {
            Self::or_word(&mut self.fmv_regs, FMV_ISR, 0x0008);
            self.stats.end_of_data_events += 1;
            self.last_picture_due_dclk = None;
            log::debug!("vmpeg: last picture displayed at DCLK {}", self.dclk);
        }
        self.stats.current_video_width = frame.width as u64;
        self.stats.current_video_height = frame.height as u64;
        self.stats.current_video_rgb_min = frame
            .pixels
            .iter()
            .map(|pixel| pixel & 0x00FF_FFFF)
            .min()
            .unwrap_or_default() as u64;
        self.stats.current_video_rgb_max = frame
            .pixels
            .iter()
            .map(|pixel| pixel & 0x00FF_FFFF)
            .max()
            .unwrap_or_default() as u64;
        self.stats.current_video_frame_hash =
            frame
                .pixels
                .iter()
                .fold(0xCBF2_9CE4_8422_2325u64, |hash, pixel| {
                    pixel.to_be_bytes().into_iter().fold(hash, |hash, byte| {
                        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
                    })
                });
        self.current_video_frame = Some(frame);
        self.stats.presented_video_frames += 1;
        Self::or_word(&mut self.fmv_regs, FMV_ISR, 0x0004);
        if self.video_update_pending && !self.video_update_scroll {
            self.complete_video_update();
        }
        self.raise_pending_video_underflow();
        true
    }

    fn timestamp_delta(later: u64, earlier: u64) -> i64 {
        let mask = (MPEG_TIMESTAMP_MODULUS - 1) as u64;
        let mut delta = (later & mask) as i64 - (earlier & mask) as i64;
        if delta > MPEG_TIMESTAMP_MODULUS / 2 {
            delta -= MPEG_TIMESTAMP_MODULUS;
        } else if delta < -(MPEG_TIMESTAMP_MODULUS / 2) {
            delta += MPEG_TIMESTAMP_MODULUS;
        }
        delta
    }

    fn anchored_timestamp_deadline(dclk: u32, scr: u64, pts: u64) -> u32 {
        let delta = Self::timestamp_delta(pts, scr) / 2;
        if delta >= 0 {
            dclk.wrapping_add(delta as u32)
        } else {
            dclk.wrapping_sub((-delta) as u32)
        }
    }

    fn video_timestamp_deadline(&self) -> Option<u32> {
        let (scr, dclk) = self.video_clock_anchor?;
        Some(Self::anchored_timestamp_deadline(
            dclk,
            scr,
            self.video_demux.last_video_pts?,
        ))
    }

    fn dclk_reached(&self, deadline: u32) -> bool {
        self.dclk.wrapping_sub(deadline) < 0x8000_0000
    }

    fn last_picture_waiting_for_pts(&self) -> bool {
        self.video_frames
            .front()
            .is_some_and(|frame| frame.last_in_sequence)
            && self
                .last_picture_due_dclk
                .is_some_and(|deadline| !self.dclk_reached(deadline))
    }

    fn complete_video_update(&mut self) {
        self.video_update_pending = false;
        self.video_update_cycles = 0;
        self.stats.video_update_events += 1;
        // VCUP and DCL are raised together by VMPEG when display-control
        // registers become active.
        Self::or_word(&mut self.fmv_regs, FMV_ISR, 0x2080);
    }

    fn signal_video_program_end(&mut self) {
        if self.video_iso_end_reported {
            return;
        }
        self.video_iso_end_reported = true;
        self.video_iso_end_pending = true;
        if !self.video_sequence_end_seen || Self::word(&self.fmv_regs, FMV_ISR) & 0x0200 == 0 {
            self.signal_pending_video_iso_end();
        }
        self.stats.program_end_events += 1;
        log::debug!("vmpeg: ISO end from video stream at DCLK {}", self.dclk);
    }

    fn signal_audio_program_end(&mut self) {
        if self.audio_iso_end_reported {
            return;
        }
        self.audio_iso_end_reported = true;
        Self::or_word(&mut self.fma_regs, 0x02, 0x0001);
        Self::or_word(&mut self.fma_regs, FMA_ISR, 0x0001);
        self.stats.program_end_events += 1;
        self.stats.audio_program_end_events += 1;
        log::debug!("vmpeg: ISO end from audio stream at DCLK {}", self.dclk);
    }

    fn signal_pending_video_iso_end(&mut self) {
        if !self.video_iso_end_pending {
            return;
        }
        self.video_iso_end_pending = false;
        Self::or_word(&mut self.fmv_regs, FMV_ISR, 0x0400);
        self.stats.video_program_end_events += 1;
        log::debug!("vmpeg: video ISO end received at DCLK {}", self.dclk);
    }

    pub fn take_audio(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.audio_out)
    }

    pub(crate) fn external_video(&self) -> Option<ExternalVideo<'_>> {
        let frame = self
            .video_visible
            .then_some(self.current_video_frame.as_ref())??;
        let window_x = usize::from(Self::word(&self.fmv_regs, 0x7E));
        let window_y = usize::from(Self::word(&self.fmv_regs, 0x7C));
        let configured_width = usize::from(Self::word(&self.fmv_regs, 0x7A));
        let configured_height = usize::from(Self::word(&self.fmv_regs, 0x78));
        let window_width = if configured_width == 0 {
            frame.width.saturating_sub(window_x)
        } else {
            configured_width
        };
        let window_height = if configured_height == 0 {
            frame.height.saturating_sub(window_y)
        } else {
            configured_height
        };
        let border = (u32::from(Self::word(&self.fmv_regs, 0x66) as u8) << 16)
            | (u32::from(Self::word(&self.fmv_regs, 0x68) as u8) << 8)
            | u32::from(Self::word(&self.fmv_regs, 0x6A) as u8);
        Some(ExternalVideo {
            frame,
            display_x: usize::from(Self::word(&self.fmv_regs, 0x76)),
            display_y: usize::from(Self::word(&self.fmv_regs, 0x74)),
            window_x,
            window_y,
            window_width,
            window_height,
            vcd_clock: self.vcd_pixel_clock_13_5,
            border,
        })
    }

    /// Read a cartridge-mapped byte. `None` means the address is not decoded
    /// by the optional cartridge and normal mainboard mapping should continue.
    pub fn read8(&mut self, addr: u32) -> Option<u8> {
        match addr {
            0xD0_0000..=0xDF_FFFF => Some(self.extension_ram[(addr - 0xD0_0000) as usize]),
            0xE0_1000..=0xE0_1FFF => Some(0xFF), // VCD control is write-only.
            0xE0_3000..=0xE0_30FF => {
                let offset = (addr - 0xE0_3000) as usize;
                self.refresh_fma_dynamic(offset);
                if offset & 1 == 0 {
                    Self::trace_register_read(
                        "FMA",
                        offset,
                        Self::word(&self.fma_regs, offset),
                        &mut self.fma_read_counts,
                    );
                }
                let value = self.fma_regs[offset];
                if offset == FMA_ISR + 1 {
                    let acknowledged = Self::word(&self.fma_regs, FMA_ISR);
                    self.acknowledge_fma_isr(acknowledged);
                }
                Some(value)
            }
            0xE0_4000..=0xE0_40FF => {
                let offset = (addr - 0xE0_4000) as usize;
                self.refresh_fmv_dynamic();
                if offset & 1 == 0 {
                    Self::trace_register_read(
                        "FMV",
                        offset,
                        Self::word(&self.fmv_regs, offset),
                        &mut self.fmv_read_counts,
                    );
                }
                let value = self.fmv_regs[offset];
                if offset == FMV_ISR + 1 {
                    let acknowledged = Self::word(&self.fmv_regs, FMV_ISR);
                    self.acknowledge_fmv_isr(acknowledged);
                }
                Some(value)
            }
            0xE4_0000..=0xE7_FFFF => Some(self.firmware[(addr - 0xE4_0000) as usize]),
            0xE8_0000..=0xEF_FFFF => Some(if self.decode_ram_visible {
                self.decode_ram[(addr - 0xE8_0000) as usize]
            } else {
                0xFF
            }),
            _ => None,
        }
    }

    pub fn write8(&mut self, addr: u32, value: u8) -> bool {
        match addr {
            0xD0_0000..=0xDF_FFFF => {
                self.extension_ram[(addr - 0xD0_0000) as usize] = value;
            }
            0xE0_1000..=0xE0_1FFF => {
                self.vcd_pixel_clock_13_5 = value & 1 != 0;
                log::trace!(
                    "vmpeg: VCD write @ {addr:#08x} = {value:#04x} (13.5 MHz={})",
                    self.vcd_pixel_clock_13_5
                );
                self.count_register_write(addr);
            }
            0xE0_3000..=0xE0_30FF => {
                let offset = (addr - 0xE0_3000) as usize;
                self.fma_regs[offset] = value;
                if offset & 1 != 0 {
                    self.handle_fma_write(offset & !1);
                    self.count_register_write(addr);
                }
            }
            0xE0_4000..=0xE0_40FF => {
                let offset = (addr - 0xE0_4000) as usize;
                self.fmv_regs[offset] = value;
                if offset & 1 != 0 {
                    self.handle_fmv_write(offset & !1);
                    self.count_register_write(addr);
                }
            }
            0xE4_0000..=0xE7_FFFF => {
                log::trace!("vmpeg: write to firmware ROM @ {addr:#08x}");
            }
            0xE8_0000..=0xEF_FFFF => {
                if self.decode_ram_visible {
                    self.decode_ram[(addr - 0xE8_0000) as usize] = value;
                }
            }
            _ => return false,
        }
        true
    }

    fn count_register_write(&mut self, _addr: u32) {
        if !self.decode_ram_visible {
            self.register_writes = self.register_writes.saturating_add(1);
            if self.register_writes >= 64 {
                self.decode_ram_visible = true;
                log::debug!("vmpeg: decoder RAM enabled after register initialization");
            }
        }
    }

    fn handle_fma_write(&mut self, offset: usize) {
        let value = Self::word(&self.fma_regs, offset);
        log::trace!("vmpeg: FMA write +{offset:#04x} = {value:#06x}");
        match offset {
            FMA_CMD => {
                if value & 0x0001 != 0 {
                    log::debug!(
                        "vmpeg: audio stop at DCLK {}, status={:#06x}, queued={} samples",
                        self.dclk,
                        Self::word(&self.fma_regs, 0x02),
                        self.mp2.pcm.len() / 2
                    );
                    self.stats.audio_stop_events += 1;
                    self.stats.audio_samples_discarded += self.mp2.pcm.len() as u64 / 2;
                    self.audio_armed = false;
                    self.audio_enabled = false;
                    self.audio_start_dclk = None;
                    self.audio_clock_anchor = None;
                    self.audio_input.clear();
                    // A stopped transport may leave an incomplete PES packet
                    // in the incremental parser. It belongs to the old play
                    // and must not be prefixed to the next stream.
                    self.audio_demux.pending.clear();
                    self.audio_demux.audio.clear();
                    self.audio_demux.last_scr = None;
                    self.audio_demux.last_audio_pts = None;
                    Self::set_word(&mut self.fma_regs, 0x02, 0x0200);
                    Self::set_word(&mut self.fma_regs, FMA_ISR, 0);
                    self.audio_errors_before_reset = self
                        .audio_errors_before_reset
                        .saturating_add(self.mp2.errors);
                    self.audio_resync_bytes_before_reset = self
                        .audio_resync_bytes_before_reset
                        .saturating_add(self.mp2.resync_bytes);
                    self.mp2.reset();
                    self.audio_underflow_reported = false;
                    self.audio_underflow_after_eoi_ack = false;
                    self.audio_iso_end_reported = false;
                }
                if value & 0x0002 != 0 {
                    if !self.audio_armed {
                        log::debug!("vmpeg: audio play armed at DCLK {}", self.dclk);
                    }
                    self.audio_armed = true;
                }
                if value & 0x8000 != 0 {
                    self.dma_target = Some(DmaTarget::Audio);
                    let status = Self::word(&self.fma_regs, 0x02);
                    Self::set_word(&mut self.fma_regs, 0x02, status & !0x0008);
                    self.audio_underflow_reported = false;
                }
            }
            FMA_ISR => self.acknowledge_fma_isr(value),
            _ => {}
        }
    }

    fn handle_fmv_write(&mut self, offset: usize) {
        let value = Self::word(&self.fmv_regs, offset);
        log::trace!("vmpeg: FMV write +{offset:#04x} = {value:#06x}");
        match offset {
            FMV_SYSCMD => {
                if value & 0x2000 != 0 {
                    log::debug!("vmpeg: video decoder disable at DCLK {}", self.dclk);
                    self.decoder_enabled = false;
                    self.video_armed = false;
                    self.playing = false;
                    self.video_clock_anchor = None;
                }
                if value & 0x0100 != 0 {
                    log::debug!("vmpeg: video decoder reset at DCLK {}", self.dclk);
                    self.video_input.clear();
                    // Decoder reset flushes both the elementary decoder and
                    // the system/PES input FIFO. Retaining a partial PES tail
                    // here concatenated the end of one clip with the next
                    // clip, causing one or two damaged pictures per play.
                    self.video_demux.pending.clear();
                    self.video_demux.video.clear();
                    if let Some(capture) = &mut self.captured_video_es {
                        capture.clear();
                    }
                    self.video_errors_before_reset = self
                        .video_errors_before_reset
                        .saturating_add(self.video_decoder.errors);
                    self.video_decoder.reset();
                    self.video_frames.clear();
                    self.current_video_frame = None;
                    self.video_armed = false;
                    self.playing = false;
                    self.play_start_dclk = None;
                    self.video_clock_anchor = None;
                    self.last_picture_due_dclk = None;
                    self.pause_irq_dclk = None;
                    Self::set_word(&mut self.fmv_regs, FMV_ISR, 0);
                    self.video_iso_end_reported = false;
                    self.video_iso_end_pending = false;
                    self.video_underflow_after_eoi_ack = false;
                    self.video_sequence_end_seen = false;
                    self.video_demux.last_scr = None;
                    self.video_demux.last_video_pts = None;
                    self.video_demux.last_video_dts = None;
                }
                if value & 0x1000 != 0 {
                    log::debug!("vmpeg: video decoder enable at DCLK {}", self.dclk);
                    self.decoder_enabled = true;
                }
                if value & 0x0008 != 0 {
                    log::debug!("vmpeg: video play at DCLK {}", self.dclk);
                    self.stats.play_events += 1;
                    self.video_armed = true;
                    self.play_start_dclk = None;
                    self.schedule_video_start();
                }
                if value & 0x0020 != 0 {
                    log::debug!("vmpeg: video continue at DCLK {}", self.dclk);
                    self.stats.continue_events += 1;
                    self.video_armed = false;
                    self.playing = true;
                    self.play_start_dclk = None;
                    self.prime_video_output();
                }
                if value & 0x0010 != 0 {
                    log::debug!("vmpeg: video pause at DCLK {}", self.dclk);
                    self.playing = false;
                    self.play_start_dclk = None;
                    self.pause_irq_dclk = Some(self.dclk.wrapping_add(100));
                }
                if value & 0x0080 != 0 {
                    log::debug!("vmpeg: video stop at DCLK {}", self.dclk);
                    self.video_armed = false;
                    self.playing = false;
                    self.play_start_dclk = None;
                    self.video_clock_anchor = None;
                    self.last_picture_due_dclk = None;
                    self.pause_irq_dclk = None;
                }
                if value & 0x8000 != 0 {
                    self.dma_target = Some(DmaTarget::Video);
                }
            }
            FMV_VIDCMD => {
                if value & 0x0008 != 0 {
                    self.video_update_pending = true;
                    self.video_update_scroll = value & 0x0004 != 0;
                    self.video_update_cycles = 0;
                }
                if value & 0x0100 != 0 {
                    log::debug!("vmpeg: external video hidden at DCLK {}", self.dclk);
                    self.video_visible = false;
                }
                if value & (0x0200 | 0x0400) != 0 {
                    log::debug!("vmpeg: external video visible at DCLK {}", self.dclk);
                    self.video_visible = true;
                    self.prime_video_output();
                }
            }
            FMV_ISR => {
                self.acknowledge_fmv_isr(value);
            }
            FMV_TIMER => self.timer_counter = 0,
            FMV_XFER => {
                self.video_input.extend_from_slice(&value.to_be_bytes());
                self.stats.direct_words += 1;
            }
            _ => {}
        }
    }

    fn refresh_fma_dynamic(&mut self, offset: usize) {
        // DSPD is a write-only DSP56001 host data port. Its readback is
        // the constant HF2-ready value used by `madriv`; retaining the
        // last byte written leaves the native driver polling forever.
        if offset == 0x24 {
            Self::set_word(&mut self.fma_regs, 0x24, 0x0004);
        }
        if offset == 0x10 {
            Self::set_word(&mut self.fma_regs, 0x12, self.dclk as u16);
        }
        Self::set_word(&mut self.fma_regs, 0x10, (self.dclk >> 16) as u16);
        let stream = Self::word(&self.fma_regs, FMA_STREAM) & 0xF;
        Self::set_word(&mut self.fma_regs, 0x0A, stream);
        if self.audio_enabled {
            self.fma_regs[0x18] |= 1;
        }
        if self.audio_demux.audio.len() >= 4 {
            let header: Vec<u8> = self.audio_demux.audio.iter().take(4).copied().collect();
            self.fma_regs[0x14..0x18].copy_from_slice(&header);
        }
    }

    fn acknowledge_fma_isr(&mut self, acknowledged: u16) {
        Self::set_word(&mut self.fma_regs, FMA_ISR, 0);
        if acknowledged & 0x0001 != 0 && self.audio_iso_end_reported {
            // Native madriv reports EOI and the subsequent empty compressed
            // input buffer as distinct CD-RTOS signals. The host MP2 decoder
            // can drain its PCM queue before the ISO-end pack arrives, so an
            // earlier underflow must not consume the post-EOI transition.
            self.audio_underflow_after_eoi_ack = true;
            self.audio_underflow_reported = false;
            self.raise_pending_audio_underflow();
        }
    }

    fn raise_pending_audio_underflow(&mut self) {
        if self.audio_enabled
            && self.mp2.pcm.is_empty()
            && self.stats.decoded_audio_frames != 0
            && !self.audio_underflow_reported
        {
            let post_eoi = self.audio_underflow_after_eoi_ack;
            self.audio_underflow_after_eoi_ack = false;
            let status = Self::word(&self.fma_regs, 0x02);
            Self::set_word(&mut self.fma_regs, 0x02, (status | 0x0008) & !0x0010);
            Self::or_word(&mut self.fma_regs, FMA_ISR, 0x0008);
            self.audio_underflow_reported = true;
            self.stats.audio_underflow_events += 1;
            log::debug!(
                "vmpeg: audio underflow at DCLK {}{}",
                self.dclk,
                if post_eoi { " after EOI" } else { "" }
            );
        }
    }

    fn acknowledge_fmv_isr(&mut self, acknowledged: u16) {
        Self::set_word(&mut self.fmv_regs, FMV_ISR, 0);
        if acknowledged & 0x0200 != 0 && self.video_iso_end_pending {
            // ISO end follows sequence end in the compressed input stream.
            // Raise EII only after ESI has been observed by native fmvdrv so
            // both state transitions remain visible.
            self.signal_pending_video_iso_end();
        } else if acknowledged & 0x0400 != 0 {
            self.video_underflow_after_eoi_ack = true;
            self.raise_pending_video_underflow();
        }
    }

    fn raise_pending_video_underflow(&mut self) {
        if self.video_underflow_after_eoi_ack
            && self.decoder_enabled
            && self.video_frames.is_empty()
            && (self.video_sequence_end_seen
                || self
                    .current_video_frame
                    .as_ref()
                    .is_some_and(|frame| frame.last_in_sequence))
            && !self.video_underflow_reported
        {
            // Keep NDAT separate from the EII interrupt which preceded it.
            // Native fmvdrv exposes both transitions to CD-RTOS; combining
            // them into one ISR read loses the asynchronous play-completion
            // notification.
            self.video_underflow_after_eoi_ack = false;
            Self::or_word(&mut self.fmv_regs, FMV_ISR, 0x0020);
            self.video_underflow_reported = true;
            self.stats.video_underflow_events += 1;
        }
    }

    fn refresh_fmv_dynamic(&mut self) {
        let fifo_ready = self.video_input.len() + 2 <= MPEG_INPUT_LIMIT;
        Self::set_word(
            &mut self.fmv_regs,
            0x5E,
            if fifo_ready { 0x2000 } else { 0 },
        );
        // MCD251 exposes its 45 kHz clock divided by 64 here. This is the
        // same 703.125 Hz timebase as the 90 kHz decoding timestamp divided
        // by 128; using /32 makes the native PCL scheduler run twice fast.
        Self::set_word(&mut self.fmv_regs, 0x98, ((self.dclk >> 6) & 0xFFFF) as u16);
        Self::set_word(
            &mut self.fmv_regs,
            0xA4,
            self.video_frames.len().min(31) as u16,
        );
        let stream = Self::word(&self.fmv_regs, FMV_STREAM) & 0xF;
        Self::set_word(&mut self.fmv_regs, FMV_STREAM, stream);
    }

    fn sync_stats(&mut self) {
        self.stats.system_packs = self.video_demux.stats.packs + self.audio_demux.stats.packs;
        self.stats.video_pes_packets = self.video_demux.stats.video_packets;
        self.stats.audio_pes_packets = self.audio_demux.stats.audio_packets;
        self.stats.video_bytes = self.video_demux.stats.video_bytes;
        self.stats.audio_bytes = self.audio_demux.stats.audio_bytes;
        self.stats.demux_errors = self.demux_errors_before_reset
            + self.video_demux.stats.errors
            + self.audio_demux.stats.errors;
        self.stats.video_errors = self.video_errors_before_reset + self.video_decoder.errors;
        self.stats.audio_errors = self.audio_errors_before_reset + self.mp2.errors;
        self.stats.audio_resync_bytes =
            self.audio_resync_bytes_before_reset + self.mp2.resync_bytes;
        self.stats.stream_errors =
            self.stats.demux_errors + self.stats.video_errors + self.stats.audio_errors;
    }

    fn word(regs: &[u8], offset: usize) -> u16 {
        u16::from_be_bytes([regs[offset], regs[offset + 1]])
    }

    fn set_word(regs: &mut [u8], offset: usize, value: u16) {
        regs[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }

    fn or_word(regs: &mut [u8], offset: usize, bits: u16) {
        Self::set_word(regs, offset, Self::word(regs, offset) | bits);
    }

    fn trace_register_read(block: &str, offset: usize, value: u16, counters: &mut [u64]) {
        let count = &mut counters[offset / 2];
        *count += 1;
        if *count <= 4 || count.is_power_of_two() {
            log::trace!(
                "vmpeg: {block} read +{offset:#04x} = {value:#06x} (count {})",
                *count
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fmv_system_command(dvc: &mut Vmpeg, command: u16) {
        let [high, low] = command.to_be_bytes();
        assert!(dvc.write8(0xE0_40C0, high));
        assert!(dvc.write8(0xE0_40C1, low));
    }

    fn test_video_frame(pixel: u32) -> DecodedVideoFrame {
        DecodedVideoFrame {
            width: 1,
            height: 1,
            pixels: vec![pixel],
            first_in_sequence: false,
            first_in_group: false,
            last_in_sequence: false,
        }
    }

    fn mpeg1_pes(stream_id: u8, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0, 0, 1, stream_id];
        let len = u16::try_from(payload.len() + 1).unwrap();
        packet.extend_from_slice(&len.to_be_bytes());
        packet.push(0x0F);
        packet.extend_from_slice(payload);
        packet
    }

    fn mpeg1_pes_with_pts(stream_id: u8, pts: u64, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0, 0, 1, stream_id];
        let len = u16::try_from(payload.len() + 5).unwrap();
        packet.extend_from_slice(&len.to_be_bytes());
        packet.extend_from_slice(&[
            0x20 | (((pts >> 30) as u8 & 7) << 1) | 1,
            (pts >> 22) as u8,
            (((pts >> 15) as u8 & 0x7F) << 1) | 1,
            (pts >> 7) as u8,
            ((pts as u8 & 0x7F) << 1) | 1,
        ]);
        packet.extend_from_slice(payload);
        packet
    }

    #[test]
    fn split_firmware_is_mirrored() {
        let mut rom = vec![0; VMPEG_SPLIT_ROM_SIZE];
        rom[0] = 0x4A;
        rom[VMPEG_SPLIT_ROM_SIZE - 1] = 0xFC;
        let dvc = Vmpeg::new(DvcConfig::new(DvcKind::Vmpeg, rom).unwrap()).unwrap();
        assert_eq!(dvc.firmware.len(), VMPEG_FULL_ROM_SIZE);
        assert_eq!(dvc.firmware[0], dvc.firmware[VMPEG_SPLIT_ROM_SIZE]);
        assert_eq!(
            dvc.firmware[VMPEG_SPLIT_ROM_SIZE - 1],
            dvc.firmware[VMPEG_FULL_ROM_SIZE - 1]
        );
    }

    #[test]
    fn external_video_x_display_uses_15_mhz_sample_positions() {
        let frame = DecodedVideoFrame {
            width: 2,
            height: 1,
            pixels: vec![0x11_2233, 0x44_5566],
            first_in_sequence: false,
            first_in_group: false,
            last_in_sequence: false,
        };
        let video = ExternalVideo {
            frame: &frame,
            display_x: 1,
            display_y: 0,
            window_x: 0,
            window_y: 0,
            window_width: 2,
            window_height: 1,
            vcd_clock: false,
            border: 0,
        };

        assert_eq!(video.pixel(1, 0), 0);
        assert_eq!(video.pixel(2, 0), 0x11_2233);
        assert_eq!(video.pixel(3, 0), 0x11_2233);
        assert_eq!(video.pixel(4, 0), 0x44_5566);
        assert_eq!(video.pixel(5, 0), 0x44_5566);
        assert_eq!(video.pixel(6, 0), 0);
    }

    #[test]
    fn white_book_clock_expands_345_samples_across_the_active_raster() {
        let frame = DecodedVideoFrame {
            width: 352,
            height: 1,
            pixels: (0..352).map(|value| value as u32 + 1).collect(),
            first_in_sequence: false,
            first_in_group: false,
            last_in_sequence: false,
        };
        let video = ExternalVideo {
            frame: &frame,
            display_x: 0,
            display_y: 0,
            window_x: 7,
            window_y: 0,
            window_width: 345,
            window_height: 1,
            vcd_clock: true,
            border: 0,
        };

        assert_eq!(video.pixel(0, 0), 8);
        assert_eq!(video.pixel(766, 0), 352);
        assert_eq!(video.pixel(767, 0), 0);
    }

    #[test]
    fn diagnostic_snapshot_exposes_mcd251_origin_and_active_registers() {
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();

        for (address, value) in [
            (0xE0_406C, 0x001A),
            (0xE0_406E, 0x0041),
            (0xE0_4070, 0x0118),
            (0xE0_4072, 0x0180),
        ] as [(u32, u16); 4]
        {
            let [high, low] = value.to_be_bytes();
            assert!(dvc.write8(address, high));
            assert!(dvc.write8(address + 1, low));
        }

        let stats = dvc.stats();
        assert_eq!(stats.video_offset_y, 26);
        assert_eq!(stats.video_offset_x, 65);
        assert_eq!(stats.video_active_y, 280);
        assert_eq!(stats.video_active_x, 384);
    }

    #[test]
    fn impeg_is_recognized_but_not_attached() {
        let config = DvcConfig::new(DvcKind::Impeg, vec![0; VMPEG_FULL_ROM_SIZE]).unwrap();
        assert!(Vmpeg::new(config).unwrap_err().contains("deferred to M4"));
    }

    #[test]
    fn incremental_pes_demux_handles_split_prefix_and_ptsless_payload() {
        let packet = mpeg1_pes(0xE0, b"video");
        let mut demux = MpegSystemDemux::default();
        demux.feed(&packet[..2]);
        demux.feed(&packet[2..7]);
        demux.feed(&packet[7..]);
        assert_eq!(demux.video.into_iter().collect::<Vec<_>>(), b"video");
        assert_eq!(demux.stats.video_packets, 1);
        assert_eq!(demux.stats.errors, 0);
    }

    #[test]
    fn four_byte_program_end_is_reported_without_waiting_for_another_packet() {
        let mut demux = MpegSystemDemux::default();
        demux.feed(&[0, 0, 1, 0xB9]);

        assert_eq!(demux.stats.program_ends, 1);
        assert!(demux.pending.is_empty());
    }

    #[test]
    fn audio_and_video_pes_are_routed_independently() {
        let mut bytes = mpeg1_pes(0xC0, b"audio");
        bytes.extend(mpeg1_pes(0xE2, b"picture"));
        let mut demux = MpegSystemDemux::default();
        demux.feed(&bytes);
        assert_eq!(demux.audio.into_iter().collect::<Vec<_>>(), b"audio");
        assert_eq!(demux.video.into_iter().collect::<Vec<_>>(), b"picture");
    }

    #[test]
    fn stream_switching_routes_only_the_new_selection_and_timestamp() {
        let mut demux = MpegSystemDemux {
            selected_video_stream: Some(0),
            ..MpegSystemDemux::default()
        };
        let mut first = mpeg1_pes_with_pts(0xE0, 90_000, b"first");
        first.extend(mpeg1_pes_with_pts(0xE1, 180_000, b"ignored"));
        demux.feed(&first);
        assert_eq!(demux.video.drain(..).collect::<Vec<_>>(), b"first");
        assert_eq!(demux.last_video_pts, Some(90_000));

        demux.selected_video_stream = Some(1);
        let mut second = mpeg1_pes_with_pts(0xE0, 270_000, b"ignored");
        second.extend(mpeg1_pes_with_pts(0xE1, 360_000, b"second"));
        demux.feed(&second);
        assert_eq!(demux.video.drain(..).collect::<Vec<_>>(), b"second");
        assert_eq!(demux.last_video_pts, Some(360_000));
        assert_eq!(demux.stats.video_packets, 2);
        assert_eq!(demux.stats.errors, 0);
    }

    #[test]
    fn ptsless_pes_retains_the_last_timestamp() {
        let mut bytes = mpeg1_pes_with_pts(0xE0, 123_456, b"first");
        bytes.extend(mpeg1_pes(0xE0, b"second"));
        let mut demux = MpegSystemDemux::default();
        demux.feed(&bytes);
        assert_eq!(demux.last_video_pts, Some(123_456));
        assert_eq!(demux.last_video_dts, Some(123_456));
    }

    #[test]
    fn decoder_ram_is_hidden_until_register_initialization() {
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();
        dvc.write8(0xE8_0000, 0x5A);
        assert_eq!(dvc.read8(0xE8_0000), Some(0xFF));
        for _ in 0..64 {
            dvc.write8(0xE0_3001, 0);
        }
        dvc.write8(0xE8_0000, 0x5A);
        assert_eq!(dvc.read8(0xE8_0000), Some(0x5A));
    }

    #[test]
    fn fmv_dma_and_direct_transfer_reach_demux() {
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();
        let packet = mpeg1_pes(0xE0, b"abc");
        dvc.write8(0xE0_40DE, packet[0]);
        dvc.write8(0xE0_40DF, packet[1]);
        dvc.write8(0xE0_40C0, 0x80);
        dvc.write8(0xE0_40C1, 0x00);
        for pair in packet[2..].chunks(2) {
            let word = u16::from_be_bytes([pair[0], pair.get(1).copied().unwrap_or(0)]);
            dvc.push_dma_word(word);
        }
        dvc.finish_dma();
        assert_eq!(dvc.stats.direct_words, 1);
        assert_eq!(dvc.stats.video_pes_packets, 1);
        assert!(dvc.stats.video_bytes >= 3);
    }

    #[test]
    fn transport_resets_discard_partial_pes_from_the_previous_play() {
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();
        dvc.video_demux
            .pending
            .extend_from_slice(&[0, 0, 1, 0xE0, 0, 20]);
        dvc.audio_demux
            .pending
            .extend_from_slice(&[0, 0, 1, 0xC0, 0, 20]);

        dvc.write8(0xE0_40C0, 0x01);
        dvc.write8(0xE0_40C1, 0x00);
        assert!(dvc.video_demux.pending.is_empty());

        dvc.write8(0xE0_3000, 0x00);
        dvc.write8(0xE0_3001, 0x01);
        assert!(dvc.audio_demux.pending.is_empty());
    }

    #[test]
    fn pause_continue_retains_current_and_queued_video_pictures() {
        // TN 088, printed pages 2 and 6: pause/continue preserves decoder
        // context. Paused device time must not consume queued pictures.
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();
        dvc.decoder_enabled = true;
        dvc.playing = true;
        dvc.current_video_frame = Some(test_video_frame(1));
        dvc.video_frames.push_back(test_video_frame(2));
        dvc.video_frames.push_back(test_video_frame(3));
        dvc.video_cycles_per_frame = 100;

        write_fmv_system_command(&mut dvc, 0x0010);
        assert!(!dvc.playing);
        dvc.tick(1_000);
        assert_eq!(dvc.video_frames.len(), 2);
        assert_eq!(
            dvc.current_video_frame
                .as_ref()
                .and_then(|frame| frame.pixels.first()),
            Some(&1)
        );
        assert_eq!(dvc.stats.presented_video_frames, 0);

        write_fmv_system_command(&mut dvc, 0x0020);
        assert!(dvc.playing);
        dvc.tick(100);
        assert_eq!(dvc.video_frames.len(), 1);
        assert_eq!(
            dvc.current_video_frame
                .as_ref()
                .and_then(|frame| frame.pixels.first()),
            Some(&2)
        );
        assert_eq!(dvc.stats.presented_video_frames, 1);
    }

    #[test]
    fn decoder_abort_clears_current_and_queued_video_pictures() {
        // TN 088's stale-picture warning is treated as a reset invariant,
        // not as hardware behavior to reproduce.
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();
        dvc.decoder_enabled = true;
        dvc.playing = true;
        dvc.current_video_frame = Some(test_video_frame(1));
        dvc.video_frames.push_back(test_video_frame(2));
        dvc.last_picture_due_dclk = Some(123);

        write_fmv_system_command(&mut dvc, 0x0100);

        assert!(dvc.decoder_enabled);
        assert!(!dvc.playing);
        assert!(dvc.current_video_frame.is_none());
        assert!(dvc.video_frames.is_empty());
        assert_eq!(dvc.last_picture_due_dclk, None);
        assert_eq!(Vmpeg::word(&dvc.fmv_regs, FMV_ISR), 0);
    }

    #[test]
    fn codec_error_counts_survive_transport_reset() {
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();
        dvc.video_decoder.errors = 2;

        dvc.write8(0xE0_40C0, 0x01);
        dvc.write8(0xE0_40C1, 0x00);
        dvc.sync_stats();

        assert_eq!(dvc.stats.video_errors, 2);
        assert_eq!(dvc.stats.stream_errors, 2);
    }

    #[test]
    fn mp2_frame_sync_acquisition_is_not_a_stream_error() {
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();
        let mut input = VecDeque::from([0x00, 0x11, 0x22, 0x33]);
        assert_eq!(dvc.mp2.decode_available(&mut input), 0);
        dvc.sync_stats();

        assert_eq!(dvc.stats.audio_resync_bytes, 1);
        assert_eq!(dvc.stats.audio_errors, 0);
        assert_eq!(dvc.stats.stream_errors, 0);
    }

    #[test]
    fn mp2_lost_sync_after_a_frame_is_a_stream_error() {
        let mut decoder = Mp2Decoder {
            synchronized: true,
            ..Mp2Decoder::default()
        };
        let mut input = VecDeque::from([0x00, 0x11, 0x22, 0x33]);
        assert_eq!(decoder.decode_available(&mut input), 0);

        assert_eq!(decoder.errors, 1);
        assert_eq!(decoder.resync_bytes, 0);
        assert!(!decoder.synchronized);
    }

    #[test]
    fn video_underflow_follows_eoi_as_a_separate_interrupt_while_paused() {
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();
        dvc.decoder_enabled = true;
        dvc.playing = false;
        dvc.video_sequence_end_seen = true;
        dvc.video_iso_end_pending = true;
        Vmpeg::set_word(&mut dvc.fmv_regs, FMV_ISR, 0x0200);

        assert_eq!(dvc.read8(0xE0_4062), Some(0x02));
        assert_eq!(dvc.read8(0xE0_4063), Some(0x00));
        assert_eq!(Vmpeg::word(&dvc.fmv_regs, FMV_ISR), 0x0400);

        assert_eq!(dvc.read8(0xE0_4062), Some(0x04));
        assert_eq!(dvc.read8(0xE0_4063), Some(0x00));
        assert_eq!(Vmpeg::word(&dvc.fmv_regs, FMV_ISR), 0x0020);
        assert_eq!(dvc.stats.video_underflow_events, 1);
    }

    #[test]
    fn audio_underflow_follows_eoi_as_a_separate_interrupt() {
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();
        dvc.audio_enabled = true;
        dvc.stats.decoded_audio_frames = 1;

        // The PCM FIFO may drain before the system-stream ISO end reaches
        // the audio decoder. Native madriv ignores this early underflow until
        // it has observed EOI.
        dvc.tick(1);
        assert_eq!(Vmpeg::word(&dvc.fma_regs, FMA_ISR), 0x0008);
        assert_eq!(dvc.read8(0xE0_301A), Some(0x00));
        assert_eq!(dvc.read8(0xE0_301B), Some(0x08));
        assert_eq!(Vmpeg::word(&dvc.fma_regs, FMA_ISR), 0);

        dvc.signal_audio_program_end();
        assert_eq!(Vmpeg::word(&dvc.fma_regs, FMA_ISR), 0x0001);
        assert_eq!(dvc.read8(0xE0_301A), Some(0x00));
        assert_eq!(dvc.read8(0xE0_301B), Some(0x01));

        // EOI acknowledgement must expose a new underflow transition even
        // when an earlier empty-FIFO interrupt was already acknowledged.
        assert_eq!(Vmpeg::word(&dvc.fma_regs, FMA_ISR), 0x0008);
    }

    #[test]
    fn audio_underflow_waits_for_queued_pcm_after_eoi_acknowledgement() {
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();
        dvc.audio_enabled = true;
        dvc.stats.decoded_audio_frames = 1;
        dvc.mp2.pcm.extend([1, 1]);

        dvc.signal_audio_program_end();
        assert_eq!(dvc.read8(0xE0_301A), Some(0x00));
        assert_eq!(dvc.read8(0xE0_301B), Some(0x01));
        assert_eq!(Vmpeg::word(&dvc.fma_regs, FMA_ISR), 0);

        dvc.mp2.pcm.clear();
        dvc.tick(1);
        assert_eq!(Vmpeg::word(&dvc.fma_regs, FMA_ISR), 0x0008);
    }

    #[test]
    fn video_and_audio_program_ends_are_independent() {
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();

        dvc.signal_video_program_end();
        assert_eq!(Vmpeg::word(&dvc.fmv_regs, FMV_ISR), 0x0400);
        assert_eq!(Vmpeg::word(&dvc.fma_regs, FMA_ISR), 0);
        assert_eq!(dvc.stats.video_program_end_events, 1);
        assert_eq!(dvc.stats.audio_program_end_events, 0);

        Vmpeg::set_word(&mut dvc.fmv_regs, FMV_ISR, 0);
        dvc.signal_audio_program_end();
        assert_eq!(Vmpeg::word(&dvc.fmv_regs, FMV_ISR), 0);
        assert_eq!(Vmpeg::word(&dvc.fma_regs, FMA_ISR), 0x0001);
        assert_eq!(dvc.stats.video_program_end_events, 1);
        assert_eq!(dvc.stats.audio_program_end_events, 1);
        assert_eq!(dvc.stats.program_end_events, 2);
    }

    #[test]
    fn mpeg_timestamp_delta_crosses_the_33_bit_wrap() {
        let before_wrap = (1u64 << 33) - 1_196;
        assert_eq!(Vmpeg::timestamp_delta(28_800, before_wrap), 29_996);
        assert_eq!(Vmpeg::timestamp_delta(before_wrap, 28_800), -29_996);
    }

    #[test]
    fn six_hour_scr_pts_mapping_has_no_audio_video_clock_drift() {
        // Both FMA and FMV use this integer 90 kHz -> 45 kHz mapping. The
        // long interval proves the shared deadline does not accumulate a
        // floating-point per-frame error.
        let anchor_dclk = 123_456;
        let anchor_scr = (1u64 << 33) - 1_000_000;
        let six_hours_90khz = 6 * 60 * 60 * 90_000u64;
        let pts = (anchor_scr + six_hours_90khz) & ((1u64 << 33) - 1);
        assert_eq!(
            Vmpeg::anchored_timestamp_deadline(anchor_dclk, anchor_scr, pts),
            anchor_dclk.wrapping_add((six_hours_90khz / 2) as u32)
        );
    }

    #[test]
    fn presentation_deadline_keeps_the_initial_scr_clock_mapping() {
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();
        dvc.video_clock_anchor = Some((1_000, 500));
        dvc.video_demux.last_scr = Some(5_000);
        dvc.video_demux.last_video_pts = Some(7_000);
        dvc.dclk = 2_000;

        // The 90 kHz PTS remains tied to the original 45 kHz DCLK anchor;
        // arrival of a later pack does not move the presentation timeline.
        assert_eq!(dvc.video_timestamp_deadline(), Some(3_500));
    }

    #[test]
    fn last_picture_waits_for_pts_before_reporting_buffer_underflow() {
        // Green Book IX.3.3.5-IX.3.3.7: the last-picture indication is a
        // display-time event. Parsing EOS must not release the old PCL or
        // report underflow before the final delayed reference reaches PTS.
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();
        dvc.decoder_enabled = true;
        dvc.playing = true;
        dvc.video_sequence_end_seen = true;
        dvc.video_underflow_after_eoi_ack = true;
        dvc.last_picture_due_dclk = Some(200);
        dvc.dclk = 100;
        dvc.video_frames.push_back(DecodedVideoFrame {
            width: 1,
            height: 1,
            pixels: vec![0],
            first_in_sequence: false,
            first_in_group: false,
            last_in_sequence: true,
        });

        assert!(!dvc.present_next_video_frame());
        assert_eq!(dvc.video_frames.len(), 1);
        assert_eq!(Vmpeg::word(&dvc.fmv_regs, FMV_ISR), 0);

        dvc.dclk = 200;
        assert!(dvc.present_next_video_frame());
        assert!(dvc.video_frames.is_empty());
        assert_eq!(
            Vmpeg::word(&dvc.fmv_regs, FMV_ISR) & (0x0004 | 0x0008 | 0x0020),
            0x002C
        );
        assert_eq!(dvc.stats.video_underflow_events, 1);
    }

    #[test]
    fn vsync_status_latches_even_when_masked_and_clears_on_isr_read() {
        let config = DvcConfig::new(DvcKind::Vmpeg, vec![0; VMPEG_SPLIT_ROM_SIZE]).unwrap();
        let mut dvc = Vmpeg::new(config).unwrap();
        Vmpeg::set_word(&mut dvc.fmv_regs, FMV_IER, 0x2000);

        dvc.notify_vsync();

        assert_eq!(Vmpeg::word(&dvc.fmv_regs, FMV_ISR), 0x0800);
        assert!(!dvc.irq());
        assert_eq!(dvc.read8(0xE0_4062), Some(0x08));
        assert_eq!(dvc.read8(0xE0_4063), Some(0x00));
        assert_eq!(Vmpeg::word(&dvc.fmv_regs, FMV_ISR), 0);
    }
}
