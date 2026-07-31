// SPDX-License-Identifier: GPL-3.0-or-later
//! The emulated machine: memory system built from a board definition.
//!
//! Address decoding uses a 4 KB page table over the low 16 MB (the decoded
//! CD-i address space) plus a special case for the SCC68070 on-chip block at
//! `$80000000`. Pages that mix regions (e.g. the MCD212 register window
//! punched into the top of the ROM range) fall back to a linear region walk.

use cdi_scc68070::bus::{Bus68k, FnCode, WaitTicks};
use cdi_scc68070::Peripherals;
use std::collections::VecDeque;

use cdi_disc::DiscImage;

use crate::board::{DeviceKind, ModelDef, VideoStandard};
use crate::cdic::Cdic;
use crate::diagnostics::{
    CpuDiagnosticSnapshot, DiagnosticProbe, DisplayProvenanceSnapshot,
    DmaChannelDiagnosticSnapshot, DmaDiagnosticSnapshot, InterruptDiagnosticSnapshot,
    MachineDiagnosticEvent, MachineDiagnosticSnapshot, Mcd212DiagnosticSnapshot,
    PclOwnershipTracker, RamDiagnosticRegion,
};
use crate::dvc::{DvcConfig, DvcKind, DvcStats, Vmpeg};
use crate::mcd212::Mcd212;
use crate::slave::SlaveHle;

/// A word transfer in SCC68070 single-address DMA occupies approximately six
/// 15 MHz CPU clocks. The data sheet specifies 2.98 million transfers/s with
/// a 35 MHz crystal (section 8.1), or about twelve crystal clocks per transfer;
/// CLKOUT is half the crystal rate.
const DMA_SINGLE_ADDRESS_WORD_CYCLES: u64 = 6;

const PAGE_SHIFT: u32 = 12;
const PAGE_COUNT: usize = 1 << (24 - PAGE_SHIFT); // 4096 pages / 16 MB
const ONCHIP_BASE: u32 = 0x8000_0000;

#[derive(Debug, Clone, Copy)]
struct DmaDiagnosticObservation {
    channel: u8,
    memory_address: u32,
    bytes: u32,
    device_address_or_target: u32,
    to_memory: bool,
    completed: bool,
    payload_hash: u64,
    transport_payload_hash: u64,
}

#[derive(Debug, Clone, Copy)]
struct DvcDmaDiagnosticInFlight {
    memory_address: u32,
    next_address: u32,
    bytes: u32,
    target: u32,
    payload_hash: u64,
}

#[derive(Debug, Clone, Copy)]
struct WatchedDmaRegion {
    address: u32,
    bytes: u32,
}

#[derive(Debug, Clone, Copy)]
struct GuestMemoryWriteObservation {
    memory_address: u32,
    bytes: u32,
    changed_bytes: u32,
    before_hash: u64,
    after_hash: u64,
    source_dma_address: u32,
    source_dma_bytes: u32,
}

#[derive(Debug, Clone, Copy)]
struct DvcRegisterWriteObservation {
    address: u32,
    value: u8,
}

#[derive(Debug, Clone, Copy)]
struct DmaRegisterWriteObservation {
    channel: u8,
    register_offset: u32,
    value: u8,
}

#[derive(Debug, Clone, Copy)]
struct RawGuestWriteObservation {
    address: u32,
    changed: bool,
}

#[derive(Debug, Default)]
struct DmaDiagnosticCapture {
    pending: Vec<DmaDiagnosticObservation>,
    dvc_in_flight: Option<DvcDmaDiagnosticInFlight>,
    watched_cdic_regions: Vec<WatchedDmaRegion>,
    pending_guest_writes: Vec<GuestMemoryWriteObservation>,
    pending_dvc_register_writes: Vec<DvcRegisterWriteObservation>,
    pending_dma_register_writes: Vec<DmaRegisterWriteObservation>,
    pending_raw_guest_writes: Vec<RawGuestWriteObservation>,
    cdic_dma_active: bool,
}

impl DmaDiagnosticCapture {
    const MAX_WATCHED_CDIC_REGIONS: usize = 128;

    fn watch_cdic_region(&mut self, address: u32, bytes: u32) {
        if bytes == 0 {
            return;
        }
        self.watched_cdic_regions
            .retain(|watch| !ranges_overlap(watch.address, watch.bytes, address, bytes));
        if self.watched_cdic_regions.len() == Self::MAX_WATCHED_CDIC_REGIONS {
            self.watched_cdic_regions.remove(0);
        }
        self.watched_cdic_regions
            .push(WatchedDmaRegion { address, bytes });
    }

    fn observe_guest_write(&mut self, address: u32, before: u8, after: u8) {
        if self.cdic_dma_active {
            return;
        }
        self.pending_raw_guest_writes
            .push(RawGuestWriteObservation {
                address,
                changed: before != after,
            });
        if before == after {
            return;
        }
        let Some(source) = self
            .watched_cdic_regions
            .iter()
            .rev()
            .find(|watch| range_contains_address(watch.address, watch.bytes, address))
            .copied()
        else {
            return;
        };
        if let Some(previous) = self.pending_guest_writes.last_mut() {
            if previous.source_dma_address == source.address
                && previous.source_dma_bytes == source.bytes
                && previous.memory_address.wrapping_add(previous.bytes) == address
            {
                previous.bytes += 1;
                previous.changed_bytes += 1;
                previous.before_hash = diagnostic_hash_byte(previous.before_hash, before);
                previous.after_hash = diagnostic_hash_byte(previous.after_hash, after);
                return;
            }
        }
        self.pending_guest_writes.push(GuestMemoryWriteObservation {
            memory_address: address,
            bytes: 1,
            changed_bytes: 1,
            before_hash: diagnostic_hash_byte(DIAGNOSTIC_HASH_OFFSET, before),
            after_hash: diagnostic_hash_byte(DIAGNOSTIC_HASH_OFFSET, after),
            source_dma_address: source.address,
            source_dma_bytes: source.bytes,
        });
    }

    fn retire_consumed_region(&mut self, address: u32, bytes: u32) {
        self.watched_cdic_regions
            .retain(|watch| !ranges_overlap(watch.address, watch.bytes, address, bytes));
    }
}

fn range_contains_address(start: u32, bytes: u32, address: u32) -> bool {
    start
        .checked_add(bytes)
        .is_some_and(|end| address >= start && address < end)
}

fn ranges_overlap(left: u32, left_bytes: u32, right: u32, right_bytes: u32) -> bool {
    let (Some(left_end), Some(right_end)) =
        (left.checked_add(left_bytes), right.checked_add(right_bytes))
    else {
        return false;
    };
    left < right_end && right < left_end
}

fn diagnostic_read_u32(regions: &[RamDiagnosticRegion<'_>], address: u32) -> Option<u32> {
    let region = regions.iter().find(|region| {
        address >= region.base
            && address
                .checked_add(4)
                .is_some_and(|end| end <= region.base + region.bytes.len() as u32)
    })?;
    let offset = (address - region.base) as usize;
    Some(u32::from_be_bytes(
        region.bytes[offset..offset + 4].try_into().unwrap(),
    ))
}

/// Stub identifiers for devices that are not yet implemented; accesses are
/// logged so BIOS expectations become visible during bring-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DevSlot {
    Cdic,
    Slave,
    Nvram,
    Vdsc,
    OnChip,
    Null,
}

#[derive(Debug, Clone, Copy)]
enum Region {
    Ram { block: usize, base: u32, size: u32 },
    Rom { base: u32, size: u32 },
    Dev { slot: DevSlot, base: u32, size: u32 },
}

impl Region {
    fn contains(&self, addr: u32) -> bool {
        let (base, size) = match *self {
            Region::Ram { base, size, .. }
            | Region::Rom { base, size }
            | Region::Dev { base, size, .. } => (base, size),
        };
        addr.wrapping_sub(base) < size
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Open,
    Ram {
        block: usize,
        page_base: u32,
    },
    Rom {
        page_base: u32,
    },
    /// Page contains more than one region (or a sub-page device window);
    /// resolve by walking the region list.
    Slow,
}

/// Latched owner of the CD-i 220's CDIC/VMPEG interrupt daisy chain.
///
/// The FMV cartridge uses `INTREQN`; SCC68070 IN5 is unused. See Philips
/// CD-i 220 service manual section 6.6.1 and the `INTREQN`/`INTENN` glossary.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum SharedIn4Owner {
    #[default]
    Idle,
    Cdic,
    Vmpeg,
}

/// MK48T08 timekeeper clock registers (device offsets $1FF8-$1FFF), BCD.
/// The clock is seeded once at power-on and advanced from emulated time by
/// the machine, keeping the core deterministic.
#[derive(Debug, Clone)]
pub struct TimekeeperClock {
    pub control: u8,
    pub seconds: u8,
    pub minutes: u8,
    pub hours: u8,
    pub day: u8,
    pub date: u8,
    pub month: u8,
    pub year: u8,
}

impl Default for TimekeeperClock {
    fn default() -> Self {
        // Fixed deterministic seed: Tuesday 1995-06-13 12:00:00.
        Self {
            control: 0,
            seconds: 0x00,
            minutes: 0x00,
            hours: 0x12,
            day: 0x03,
            date: 0x13,
            month: 0x06,
            year: 0x95,
        }
    }
}

/// Memory system of a CD-i machine (everything the CPU addresses).
pub struct MachineBus {
    pages: Vec<Page>,
    regions: Vec<Region>,
    pub ram: Vec<Vec<u8>>,
    pub rom: Vec<u8>,
    /// MK48T08 SRAM contents, 8 KB (persisted by the frontend/CLI between
    /// runs). The device sits on even bytes of its window; device offset =
    /// window offset / 2.
    pub nvram: Vec<u8>,
    pub clock: TimekeeperClock,
    /// SCC68070 on-chip peripherals ($80000000 block).
    pub periph: Peripherals,
    /// SLAVE MCU (input/status controller) at $310000.
    pub slave: SlaveHle,
    /// MCD212 video/display controller at $4FFFE0.
    pub mcd212: Mcd212,
    /// CDIC (CD interface controller) at $300000.
    pub cdic: Cdic,
    /// The loaded disc image, if any.
    pub disc: Option<DiscImage>,
    /// Optional Digital Video Cartridge. M3 supports the VMPEG variant.
    pub dvc: Option<Vmpeg>,
    /// Source whose programmed vector owns the shared IN4 acknowledge.
    shared_in4_owner: SharedIn4Owner,
    /// SCC68070-clock budget for the asynchronous DMAREQ2 transfer engine.
    dvc_dma_cycles: u64,
    /// Present only while bounded diagnostics are enabled.
    dma_diagnostics: Option<DmaDiagnosticCapture>,
}

impl MachineBus {
    pub fn new(model: &ModelDef, rom: Vec<u8>) -> Result<Self, String> {
        let mut regions = Vec::new();
        let mut ram = Vec::new();

        let entries = model.board.entries;
        for (i, entry) in entries.iter().enumerate() {
            // Device windows without an explicit size extend to the next
            // entry's base (matches the .brd file convention).
            let next_base = entries
                .get(i + 1)
                .map(|e| e.base)
                .filter(|&b| b > entry.base);
            let until_next = |dflt: u32| next_base.map_or(dflt, |b| b - entry.base);
            match entry.device {
                DeviceKind::Ram { size, .. } => {
                    regions.push(Region::Ram {
                        block: ram.len(),
                        base: entry.base,
                        size,
                    });
                    ram.push(vec![0u8; size as usize]);
                }
                DeviceKind::SysRom { size } => {
                    regions.push(Region::Rom {
                        base: entry.base,
                        size,
                    });
                }
                DeviceKind::Cdic { .. } => regions.push(Region::Dev {
                    slot: DevSlot::Cdic,
                    base: entry.base,
                    size: until_next(0x1_0000),
                }),
                DeviceKind::Slave { .. } => regions.push(Region::Dev {
                    slot: DevSlot::Slave,
                    base: entry.base,
                    size: until_next(0x8000),
                }),
                DeviceKind::Nvram => regions.push(Region::Dev {
                    slot: DevSlot::Nvram,
                    base: entry.base,
                    size: model.nvram_size,
                }),
                DeviceKind::Vdsc => regions.push(Region::Dev {
                    slot: DevSlot::Vdsc,
                    base: entry.base,
                    size: 0x20,
                }),
                DeviceKind::Cpu68070 => regions.push(Region::Dev {
                    slot: DevSlot::OnChip,
                    base: entry.base,
                    size: 0x1_0000,
                }),
                DeviceKind::Null { size } => regions.push(Region::Dev {
                    slot: DevSlot::Null,
                    base: entry.base,
                    size,
                }),
                DeviceKind::Unsupported(name) => {
                    return Err(format!(
                        "board {} requires unsupported device {name}",
                        model.board.name
                    ));
                }
            }
        }

        let mut bus = Self {
            pages: vec![Page::Open; PAGE_COUNT],
            regions,
            ram,
            rom,
            nvram: vec![0u8; 0x2000],
            clock: TimekeeperClock::default(),
            periph: Peripherals::new(),
            slave: SlaveHle::new(model.slave_version, model.video == VideoStandard::Pal),
            mcd212: Mcd212::new(model.video == VideoStandard::Pal),
            cdic: Cdic::new(),
            disc: None,
            dvc: None,
            shared_in4_owner: SharedIn4Owner::Idle,
            dvc_dma_cycles: 0,
            dma_diagnostics: None,
        };
        bus.build_page_table();
        Ok(bus)
    }

    fn build_page_table(&mut self) {
        for page_idx in 0..PAGE_COUNT {
            let page_base = (page_idx as u32) << PAGE_SHIFT;
            let page_end = page_base + (1 << PAGE_SHIFT) - 1;
            // Later board entries win where regions overlap (VDSC over ROM),
            // so scan the region list in reverse for the covering region.
            let mut covering = None;
            let mut mixed = false;
            for region in self.regions.iter().rev() {
                let full = region.contains(page_base) && region.contains(page_end);
                let partial = region.contains(page_base) || region.contains(page_end);
                if covering.is_none() && full {
                    covering = Some(*region);
                } else if partial {
                    mixed = true;
                }
            }
            self.pages[page_idx] = match (covering, mixed) {
                (_, true) => Page::Slow,
                (Some(Region::Ram { block, base, .. }), false) => Page::Ram {
                    block,
                    page_base: page_base - base,
                },
                (Some(Region::Rom { base, .. }), false) => Page::Rom {
                    page_base: page_base - base,
                },
                (Some(Region::Dev { .. }), false) => Page::Slow,
                (None, false) => Page::Open,
            };
        }
    }

    /// Reset behavior observed on real hardware: the initial SSP/PC vector
    /// pair is fetched from the start of the system ROM. Mirror the first
    /// 8 ROM bytes into RAM at address 0 (MAME does the same).
    pub fn reset(&mut self) {
        if let (Some(ram0), true) = (self.ram.first_mut(), self.rom.len() >= 8) {
            ram0[..8].copy_from_slice(&self.rom[..8]);
        }
        if let Some(dvc) = &mut self.dvc {
            dvc.reset();
        }
        self.shared_in4_owner = SharedIn4Owner::Idle;
        self.periph.in4_line = false;
        self.periph.in5_line = false;
        self.dvc_dma_cycles = 0;
        if let Some(capture) = &mut self.dma_diagnostics {
            *capture = DmaDiagnosticCapture::default();
        }
    }

    pub fn attach_dvc(&mut self, config: DvcConfig) -> Result<(), String> {
        self.dvc = Some(Vmpeg::new(config)?);
        self.shared_in4_owner = SharedIn4Owner::Idle;
        self.dvc_dma_cycles = 0;
        self.refresh_external_interrupts();
        Ok(())
    }

    pub fn detach_dvc(&mut self) {
        self.dvc = None;
        self.shared_in4_owner = SharedIn4Owner::Idle;
        self.dvc_dma_cycles = 0;
        self.refresh_external_interrupts();
    }

    fn refresh_external_interrupts(&mut self) {
        self.arbitrate_shared_in4(
            self.cdic.int_line(),
            self.dvc.as_ref().is_some_and(Vmpeg::irq),
        );
    }

    fn arbitrate_shared_in4(&mut self, cdic_request: bool, vmpeg_request: bool) {
        let owner_still_requests = match self.shared_in4_owner {
            SharedIn4Owner::Idle => false,
            SharedIn4Owner::Cdic => cdic_request,
            SharedIn4Owner::Vmpeg => vmpeg_request,
        };
        if !owner_still_requests {
            self.shared_in4_owner = SharedIn4Owner::Idle;
        }
        if self.shared_in4_owner == SharedIn4Owner::Idle {
            // The extension is later in the physical daisy chain and wins
            // only when both requests first arrive at the same boundary.
            self.shared_in4_owner = if vmpeg_request {
                SharedIn4Owner::Vmpeg
            } else if cdic_request {
                SharedIn4Owner::Cdic
            } else {
                SharedIn4Owner::Idle
            };
        }
        self.periph.in4_line = match self.shared_in4_owner {
            SharedIn4Owner::Idle => false,
            SharedIn4Owner::Cdic => cdic_request,
            SharedIn4Owner::Vmpeg => vmpeg_request,
        };
        // The base player's service manual explicitly marks IN5 unused.
        self.periph.in5_line = false;
    }

    fn dev_read8(&mut self, slot: DevSlot, addr: u32) -> u8 {
        match slot {
            DevSlot::Nvram => {
                // MK48T08 on even bytes only (MAME: umask16 0xff00).
                if addr & 1 != 0 {
                    return 0;
                }
                let idx = ((addr >> 1) as usize) & 0x1FFF;
                match idx {
                    0x1FF8 => self.clock.control,
                    0x1FF9 => self.clock.seconds,
                    0x1FFA => self.clock.minutes,
                    0x1FFB => self.clock.hours,
                    0x1FFC => self.clock.day,
                    0x1FFD => self.clock.date,
                    0x1FFE => self.clock.month,
                    0x1FFF => self.clock.year,
                    _ => self.nvram[idx],
                }
            }
            DevSlot::OnChip => self.periph.read8(addr),
            DevSlot::Cdic => match addr {
                // Mono-I quirk: the UART-loopback enable window inside the
                // CDIC RAM range (MAME maps a 16-bit 0x1234 over
                // $301400-$301403; the BIOS long-compares 0x12341234).
                0x1400 | 0x1402 => 0x12,
                0x1401 | 0x1403 => 0x34,
                0x0000..=0x3BFF => self.cdic.ram_read8(addr),
                0x3C00..=0x3FFF => {
                    let v = self.cdic.read8(addr);
                    self.refresh_external_interrupts();
                    v
                }
                _ => 0,
            },
            DevSlot::Vdsc => {
                let val = self.mcd212.read8(addr);
                self.periph.set_int1(self.mcd212.int_line());
                val
            }
            DevSlot::Slave => {
                // 16-bit word slots; the data byte rides the low (odd) lane.
                if addr & 1 != 0 {
                    let val = self.slave.read((addr as usize >> 1) & 3);
                    self.periph.in2_line = self.slave.irq();
                    val
                } else {
                    0
                }
            }
            DevSlot::Null => 0,
        }
    }

    fn dev_write8(&mut self, slot: DevSlot, addr: u32, val: u8) {
        match slot {
            DevSlot::Nvram => {
                if addr & 1 != 0 {
                    return;
                }
                let idx = ((addr >> 1) as usize) & 0x1FFF;
                match idx {
                    0x1FF8 => self.clock.control = val,
                    0x1FF9 => self.clock.seconds = val,
                    0x1FFA => self.clock.minutes = val,
                    0x1FFB => self.clock.hours = val,
                    0x1FFC => self.clock.day = val,
                    0x1FFD => self.clock.date = val,
                    0x1FFE => self.clock.month = val,
                    0x1FFF => self.clock.year = val,
                    _ => self.nvram[idx] = val,
                }
            }
            DevSlot::OnChip => {
                if matches!(addr, 0x4000..=0x406F) {
                    if let Some(capture) = &mut self.dma_diagnostics {
                        capture
                            .pending_dma_register_writes
                            .push(DmaRegisterWriteObservation {
                                channel: u8::from(addr >= 0x4040),
                                register_offset: addr,
                                value: val,
                            });
                    }
                }
                self.periph.write8(addr, val);
            }
            DevSlot::Cdic => match addr {
                0x0000..=0x3BFF => self.cdic.ram_write8(addr, val),
                0x3C00..=0x3FFF => {
                    if let Some(dma_word) = self.cdic.write8(addr, val) {
                        self.cdic_dma(dma_word);
                    }
                    self.refresh_external_interrupts();
                }
                _ => {}
            },
            DevSlot::Vdsc => self.mcd212.write8(addr, val),
            DevSlot::Slave => {
                if addr & 1 != 0 {
                    self.slave.write((addr as usize >> 1) & 3, val);
                    self.periph.in2_line = self.slave.irq();
                }
            }
            DevSlot::Null => {}
        }
    }

    fn slow_read8(&mut self, addr: u32) -> u8 {
        for region in self.regions.clone().iter().rev() {
            if region.contains(addr) {
                return match *region {
                    Region::Ram { block, base, .. } => self.ram[block][(addr - base) as usize],
                    Region::Rom { base, .. } => self.rom[(addr - base) as usize],
                    Region::Dev { slot, base, .. } => self.dev_read8(slot, addr - base),
                };
            }
        }
        log::trace!("open-bus read8 @ {addr:#010x}");
        0xFF
    }

    fn slow_write8(&mut self, addr: u32, val: u8) {
        for region in self.regions.clone().iter().rev() {
            if region.contains(addr) {
                match *region {
                    Region::Ram { block, base, .. } => {
                        let index = (addr - base) as usize;
                        let before = self.ram[block][index];
                        self.observe_guest_memory_write(addr, before, val);
                        self.ram[block][index] = val;
                    }
                    Region::Rom { .. } => log::trace!("write to ROM @ {addr:#010x}"),
                    Region::Dev { slot, base, .. } => self.dev_write8(slot, addr - base, val),
                }
                return;
            }
        }
        log::trace!("open-bus write8 @ {addr:#010x} = {val:#04x}");
    }

    fn observe_guest_memory_write(&mut self, address: u32, before: u8, after: u8) {
        if let Some(capture) = &mut self.dma_diagnostics {
            capture.observe_guest_write(address, before, after);
        }
    }

    pub fn read8_silent(&mut self, addr: u32) -> u8 {
        self.raw_read8(addr)
    }

    /// CDIC DMA transfer: copy words between CDIC RAM and main memory
    /// using the 68070 DMA channel-0 registers (MAME `cdic_w` 0x3FF8).
    fn cdic_dma(&mut self, dma_word: u16) {
        let start = self.periph.dma0_memory_address();
        let count = u32::from(self.periph.dma0_transfer_count());
        let to_memory = self.periph.dma0_operation_control() & 0x80 != 0;
        let mut device_at = usize::from(dma_word & 0x3FFF) & !1;
        let device_start = device_at as u32;
        let mut payload_hash = DIAGNOSTIC_HASH_OFFSET;
        let mut transport_payload_hash = DIAGNOSTIC_HASH_OFFSET;
        let transfer_bytes = count * 2;
        let transport_skip = if transfer_bytes == 2324 { 12 } else { 0 };
        log::debug!(
            "cdic dma: {} {count} words at mem {start:#010x} dev {device_at:#06x} head {:02x?}",
            if to_memory { "to-mem" } else { "to-dev" },
            &self.cdic.ram
                [device_at & 0x3FFF..(device_at & 0x3FFF) + 16.min(0x3FFF - (device_at & 0x3FFF))]
        );
        if let Some(capture) = &mut self.dma_diagnostics {
            capture.cdic_dma_active = true;
        }
        for i in 0..count {
            let mem_addr = start.wrapping_add(i * 2);
            if to_memory {
                let hi = self.cdic.ram[device_at & 0x3FFF];
                let lo = self.cdic.ram[(device_at + 1) & 0x3FFF];
                payload_hash = diagnostic_hash_byte(diagnostic_hash_byte(payload_hash, hi), lo);
                for (byte_offset, byte) in [(i * 2, hi), (i * 2 + 1, lo)] {
                    if byte_offset >= transport_skip {
                        transport_payload_hash = diagnostic_hash_byte(transport_payload_hash, byte);
                    }
                }
                self.raw_write8(mem_addr, hi);
                self.raw_write8(mem_addr.wrapping_add(1), lo);
            } else {
                let hi = self.raw_read8(mem_addr);
                let lo = self.raw_read8(mem_addr.wrapping_add(1));
                payload_hash = diagnostic_hash_byte(diagnostic_hash_byte(payload_hash, hi), lo);
                for (byte_offset, byte) in [(i * 2, hi), (i * 2 + 1, lo)] {
                    if byte_offset >= transport_skip {
                        transport_payload_hash = diagnostic_hash_byte(transport_payload_hash, byte);
                    }
                }
                self.cdic.ram[device_at & 0x3FFF] = hi;
                self.cdic.ram[(device_at + 1) & 0x3FFF] = lo;
            }
            device_at += 2;
        }
        if let Some(capture) = &mut self.dma_diagnostics {
            capture.cdic_dma_active = false;
        }
        self.periph
            .set_dma0_memory_address(start.wrapping_add(count * 2));
        if let Some(capture) = &mut self.dma_diagnostics {
            // Real-time Form-2 payloads are 2,304 or 2,324 bytes. Excluding
            // ordinary 2,048-byte filesystem traffic keeps this bounded
            // provenance focused on the CDFM/PCL media path.
            if to_memory && transfer_bytes >= 2304 {
                capture.watch_cdic_region(start, transfer_bytes);
            }
            capture.pending.push(DmaDiagnosticObservation {
                channel: 0,
                memory_address: start,
                bytes: count * 2,
                device_address_or_target: device_start,
                to_memory,
                completed: true,
                payload_hash,
                transport_payload_hash,
            });
        }
    }

    /// Advance SCC68070 DMA channel 2 (the second register block) by elapsed
    /// 15 MHz clocks while VMPEG asserts DMAREQ2. This keeps DMA throughput
    /// independent of CPU instruction count without making an entire sector
    /// appear at the transfer register instantaneously.
    fn service_dvc_dma(&mut self, elapsed_cycles: u64) -> u64 {
        let requested = self.dvc.as_ref().is_some_and(Vmpeg::dma_requested);
        if !self.periph.dma1_active() {
            self.dvc_dma_cycles = 0;
            self.finish_dvc_dma_diagnostic(false);
            return 0;
        }
        if !requested {
            // Do not accumulate an arbitrarily large credit while the device
            // has deasserted DMAREQ2 for FIFO backpressure.
            self.dvc_dma_cycles = self.dvc_dma_cycles.min(DMA_SINGLE_ADDRESS_WORD_CYCLES - 1);
            return 0;
        }
        if self.periph.dma1_operation_control() & 0x80 != 0 {
            log::warn!("vmpeg: unsupported device-to-memory DMA channel-1 transfer");
            self.periph.complete_dma1();
            if let Some(dvc) = &mut self.dvc {
                dvc.finish_dma();
            }
            self.finish_dvc_dma_diagnostic(false);
            return 0;
        }

        self.dvc_dma_cycles = self.dvc_dma_cycles.saturating_add(elapsed_cycles);
        let burst = !self.periph.dma1_cycle_steal();
        let mut words = 0u64;
        while self.dvc_dma_cycles >= DMA_SINGLE_ADDRESS_WORD_CYCLES {
            self.dvc_dma_cycles -= DMA_SINGLE_ADDRESS_WORD_CYCLES;
            let address = self.periph.dma1_memory_address();
            let word = u16::from_be_bytes([
                self.raw_read8(address),
                self.raw_read8(address.wrapping_add(1)),
            ]);
            self.observe_dvc_dma_word(address, word);
            if let Some(dvc) = &mut self.dvc {
                dvc.push_dma_word(word);
            }
            words += 1;
            self.periph.advance_dma1_word();
            if !self.periph.dma1_active() {
                if let Some(dvc) = &mut self.dvc {
                    dvc.finish_dma();
                }
                self.finish_dvc_dma_diagnostic(true);
                break;
            }
            if !self.dvc.as_ref().is_some_and(Vmpeg::dma_requested) {
                self.dvc_dma_cycles = self.dvc_dma_cycles.min(DMA_SINGLE_ADDRESS_WORD_CYCLES - 1);
                break;
            }
            if !burst {
                break;
            }
        }
        words
    }

    fn observe_dvc_dma_word(&mut self, address: u32, word: u16) {
        let target = self
            .dvc
            .as_ref()
            .map_or(0, |dvc| u32::from(dvc.register_snapshot().dma_target));
        let Some(capture) = &mut self.dma_diagnostics else {
            return;
        };
        let discontinuity = capture.dvc_in_flight.is_some_and(|in_flight| {
            in_flight.next_address != address || in_flight.target != target
        });
        if discontinuity {
            let in_flight = capture.dvc_in_flight.take().expect("checked above");
            capture.pending.push(dvc_dma_observation(in_flight, false));
        }
        let in_flight = capture
            .dvc_in_flight
            .get_or_insert(DvcDmaDiagnosticInFlight {
                memory_address: address,
                next_address: address,
                bytes: 0,
                target,
                payload_hash: DIAGNOSTIC_HASH_OFFSET,
            });
        for byte in word.to_be_bytes() {
            in_flight.payload_hash = diagnostic_hash_byte(in_flight.payload_hash, byte);
        }
        in_flight.bytes += 2;
        in_flight.next_address = address.wrapping_add(2);
    }

    fn finish_dvc_dma_diagnostic(&mut self, completed: bool) {
        let Some(capture) = &mut self.dma_diagnostics else {
            return;
        };
        if let Some(in_flight) = capture.dvc_in_flight.take() {
            if completed {
                capture.retire_consumed_region(in_flight.memory_address, in_flight.bytes);
            }
            capture
                .pending
                .push(dvc_dma_observation(in_flight, completed));
        }
    }

    fn set_dma_diagnostics_enabled(&mut self, enabled: bool) {
        self.dma_diagnostics = enabled.then(DmaDiagnosticCapture::default);
    }

    fn take_dma_diagnostic_observations(&mut self) -> Vec<DmaDiagnosticObservation> {
        self.dma_diagnostics
            .as_mut()
            .map(|capture| std::mem::take(&mut capture.pending))
            .unwrap_or_default()
    }

    fn take_guest_memory_write_observations(&mut self) -> Vec<GuestMemoryWriteObservation> {
        self.dma_diagnostics
            .as_mut()
            .map(|capture| std::mem::take(&mut capture.pending_guest_writes))
            .unwrap_or_default()
    }

    fn take_dvc_register_write_observations(&mut self) -> Vec<DvcRegisterWriteObservation> {
        self.dma_diagnostics
            .as_mut()
            .map(|capture| std::mem::take(&mut capture.pending_dvc_register_writes))
            .unwrap_or_default()
    }

    fn take_dma_register_write_observations(&mut self) -> Vec<DmaRegisterWriteObservation> {
        self.dma_diagnostics
            .as_mut()
            .map(|capture| std::mem::take(&mut capture.pending_dma_register_writes))
            .unwrap_or_default()
    }

    fn take_raw_guest_write_observations(&mut self) -> Vec<RawGuestWriteObservation> {
        self.dma_diagnostics
            .as_mut()
            .map(|capture| std::mem::take(&mut capture.pending_raw_guest_writes))
            .unwrap_or_default()
    }

    fn diagnostic_ram_regions(&self) -> Vec<RamDiagnosticRegion<'_>> {
        let mut regions = Vec::new();
        if let Some(dvc) = &self.dvc {
            regions.push(RamDiagnosticRegion {
                base: 0x00D0_0000,
                bytes: dvc.extension_ram(),
            });
        }
        regions.extend(self.regions.iter().filter_map(|region| match *region {
            Region::Ram { block, base, .. } => Some(RamDiagnosticRegion {
                base,
                bytes: &self.ram[block],
            }),
            _ => None,
        }));
        regions
    }

    fn raw_read8(&mut self, addr: u32) -> u8 {
        if addr >= ONCHIP_BASE {
            return self.dev_read8(DevSlot::OnChip, addr - ONCHIP_BASE);
        }
        let a24 = addr & 0x00FF_FFFF;
        if let Some(dvc) = &mut self.dvc {
            if let Some(value) = dvc.read8(a24) {
                return value;
            }
        }
        match self.pages[(a24 >> PAGE_SHIFT) as usize] {
            Page::Ram { block, page_base } => self.ram[block][(page_base + (a24 & 0xFFF)) as usize],
            Page::Rom { page_base } => self.rom[(page_base + (a24 & 0xFFF)) as usize],
            Page::Slow => self.slow_read8(a24),
            Page::Open => {
                log::trace!("open-bus read8 @ {addr:#010x}");
                0xFF
            }
        }
    }

    fn raw_write8(&mut self, addr: u32, val: u8) {
        if addr >= ONCHIP_BASE {
            return self.dev_write8(DevSlot::OnChip, addr - ONCHIP_BASE, val);
        }
        let a24 = addr & 0x00FF_FFFF;
        let dvc_ram_before = if matches!(a24, 0x00D0_0000..=0x00DF_FFFF) {
            self.dvc
                .as_ref()
                .map(|dvc| dvc.extension_ram()[(a24 - 0x00D0_0000) as usize])
        } else {
            None
        };
        if self.dvc.as_mut().is_some_and(|dvc| dvc.write8(a24, val)) {
            if let Some(before) = dvc_ram_before {
                self.observe_guest_memory_write(a24, before, val);
            }
            let fma_control = matches!(a24, 0x00E0_3000..=0x00E0_30FF)
                && matches!((a24 - 0x00E0_3000) & 0xFE, 0x00 | 0x08 | 0x0C);
            let fmv_control = matches!(a24, 0x00E0_4000..=0x00E0_40FF)
                && matches!(
                    (a24 - 0x00E0_4000) & 0xFE,
                    0x02 | 0x04 | 0x64 | 0x70..=0x88 | 0xC0 | 0xC2 | 0xC4 | 0xDC
                );
            if matches!(a24, 0x00E0_1000..=0x00E0_1FFF) || fma_control || fmv_control {
                if let Some(capture) = &mut self.dma_diagnostics {
                    capture
                        .pending_dvc_register_writes
                        .push(DvcRegisterWriteObservation {
                            address: a24,
                            value: val,
                        });
                }
            }
            return;
        }
        match self.pages[(a24 >> PAGE_SHIFT) as usize] {
            Page::Ram { block, page_base } => {
                let index = (page_base + (a24 & 0xFFF)) as usize;
                let before = self.ram[block][index];
                self.observe_guest_memory_write(a24, before, val);
                self.ram[block][index] = val;
            }
            Page::Rom { .. } => log::trace!("write to ROM @ {addr:#010x}"),
            Page::Slow => self.slow_write8(a24, val),
            Page::Open => log::trace!("open-bus write8 @ {addr:#010x} = {val:#04x}"),
        }
    }
}

/// A complete machine: CPU + memory system.
pub struct Machine {
    pub cpu: cdi_scc68070::Cpu,
    pub bus: MachineBus,
    diagnostic_events: Option<(usize, VecDeque<MachineDiagnosticEvent>, DiagnosticProbe)>,
    milestone_only_diagnostics: bool,
    pcl_diagnostics: Option<PclOwnershipTracker>,
}

impl Machine {
    pub fn new(model: &ModelDef, rom: Vec<u8>) -> Result<Self, String> {
        Self::with_dvc(model, rom, None)
    }

    pub fn with_dvc(
        model: &ModelDef,
        rom: Vec<u8>,
        dvc: Option<DvcConfig>,
    ) -> Result<Self, String> {
        let mut bus = MachineBus::new(model, rom)?;
        if let Some(config) = dvc {
            bus.attach_dvc(config)?;
        }
        let mut m = Self {
            cpu: cdi_scc68070::Cpu::new(),
            bus,
            diagnostic_events: None,
            milestone_only_diagnostics: false,
            pcl_diagnostics: None,
        };
        m.reset();
        Ok(m)
    }

    /// Attach a DVC and reset the host. The inserted disc is retained.
    pub fn attach_dvc(&mut self, config: DvcConfig) -> Result<(), String> {
        self.bus.attach_dvc(config)?;
        self.reset();
        Ok(())
    }

    /// Detach the optional DVC and reset the host, retaining the disc.
    pub fn detach_dvc(&mut self) {
        self.bus.detach_dvc();
        self.reset();
    }

    pub fn dvc_kind(&self) -> Option<DvcKind> {
        self.bus.dvc.as_ref().map(|_| DvcKind::Vmpeg)
    }

    pub fn dvc_stats(&self) -> Option<DvcStats> {
        self.bus.dvc.as_ref().map(Vmpeg::stats)
    }

    /// Change the emulated player's PAL/NTSC configuration and perform a
    /// full reset. The inserted disc and DVC remain attached.
    pub fn set_video_standard(&mut self, standard: VideoStandard) {
        let pal = standard == VideoStandard::Pal;
        self.bus.slave.set_video_standard(pal);
        self.bus.mcd212.pal = pal;
        self.reset();
    }

    pub fn reset(&mut self) {
        if let Some(tracker) = &mut self.pcl_diagnostics {
            *tracker = PclOwnershipTracker::default();
        }
        self.bus.reset();
        self.bus.periph.reset();
        self.bus.slave.reset();
        self.bus.mcd212.reset();
        self.bus.cdic.reset();
        self.bus.cdic.set_disc_layout(self.bus.disc.as_ref());
        self.cpu.pending_ipl = 0;
        self.cpu.reset(&mut self.bus);
    }

    /// Perform a cold player power cycle.
    ///
    /// Battery-backed NVRAM, the timekeeper, inserted media, and cartridge
    /// firmware remain present. All volatile host, CDIC, and DVC memory is
    /// cleared before the normal reset sequence.
    pub fn power_cycle(&mut self) {
        for ram in &mut self.bus.ram {
            ram.fill(0);
        }
        self.bus.cdic.power_cycle();
        if let Some(dvc) = &mut self.bus.dvc {
            dvc.power_cycle();
        }
        self.cpu = cdi_scc68070::Cpu::new();
        self.reset();
    }

    /// Enable bounded transition events. A zero capacity disables capture.
    /// Diagnostics are observational and do not alter device timing.
    pub fn enable_diagnostics(&mut self, capacity: usize) {
        if capacity == 0 {
            self.diagnostic_events = None;
            self.milestone_only_diagnostics = false;
            self.pcl_diagnostics = None;
            self.bus.set_dma_diagnostics_enabled(false);
            return;
        }
        self.milestone_only_diagnostics = false;
        self.bus.set_dma_diagnostics_enabled(true);
        self.pcl_diagnostics = Some(PclOwnershipTracker::default());
        self.diagnostic_events = Some((
            capacity,
            VecDeque::with_capacity(capacity.min(4096)),
            self.diagnostic_probe(),
        ));
    }

    /// Enable only sparse VMPEG play/pause/end milestones.
    ///
    /// This is intended for very long A/V drift runs where per-frame raster,
    /// DMA, and PCL evidence would add substantial overhead. Each milestone
    /// still includes the current composed-raster hash.
    pub fn enable_dvc_milestone_diagnostics(&mut self, capacity: usize) {
        if capacity == 0 {
            self.enable_diagnostics(0);
            return;
        }
        self.milestone_only_diagnostics = true;
        self.bus.set_dma_diagnostics_enabled(false);
        self.pcl_diagnostics = None;
        self.diagnostic_events = Some((
            capacity,
            VecDeque::with_capacity(capacity.min(4096)),
            self.diagnostic_probe(),
        ));
    }

    /// Return a deterministic read-only snapshot of the current machine.
    pub fn diagnostic_snapshot(&self) -> MachineDiagnosticSnapshot {
        let geometry = self.bus.mcd212.display_geometry();
        let pixel_count = geometry.raster_width * geometry.raster_height;
        MachineDiagnosticSnapshot {
            cpu: CpuDiagnosticSnapshot {
                d: self.cpu.d,
                a: self.cpu.a,
                pc: self.cpu.pc,
                sr: self.cpu.sr,
                stopped: self.cpu.stopped,
                pending_ipl: self.cpu.pending_ipl,
                cycles: self.cpu.cycles,
                exceptions: self.cpu.exceptions_taken,
            },
            interrupts: InterruptDiagnosticSnapshot {
                pending_ipl: self.bus.periph.pending_ipl(),
                slave_in2: self.bus.periph.in2_line,
                cdic_in4: self.bus.cdic.int_line(),
                dvc_in4: self.bus.dvc.as_ref().is_some_and(Vmpeg::irq),
            },
            dma: DmaDiagnosticSnapshot {
                cdic_channel: DmaChannelDiagnosticSnapshot {
                    status: self.bus.periph.dma0_status(),
                    channel_control: self.bus.periph.dma0_channel_control(),
                    memory_address: self.bus.periph.dma0_memory_address(),
                    transfer_count: self.bus.periph.dma0_transfer_count(),
                    operation_control: self.bus.periph.dma0_operation_control(),
                    active: self.bus.periph.dma0_active(),
                },
                dvc_channel: DmaChannelDiagnosticSnapshot {
                    status: self.bus.periph.dma1_status(),
                    channel_control: self.bus.periph.dma1_channel_control(),
                    memory_address: self.bus.periph.dma1_memory_address(),
                    transfer_count: self.bus.periph.dma1_transfer_count(),
                    operation_control: self.bus.periph.dma1_operation_control(),
                    active: self.bus.periph.dma1_active(),
                },
            },
            cdic: self.bus.cdic.diagnostic_snapshot(),
            slave: self.bus.slave.diagnostic_snapshot(),
            mcd212: Mcd212DiagnosticSnapshot {
                geometry,
                csrw: self.bus.mcd212.csrw,
                csrr: self.bus.mcd212.csrr,
                dcr: self.bus.mcd212.dcr,
                vsr: self.bus.mcd212.vsr,
                ddr: self.bus.mcd212.ddr,
                dcp: self.bus.mcd212.dcp,
                dca: self.bus.mcd212.dca,
                image_coding_method: self.bus.mcd212.image_coding_method,
                transparency_control: self.bus.mcd212.transparency_control,
                plane_order: self.bus.mcd212.plane_order,
                dyuv_absolute_start: self.bus.mcd212.dyuv_abs_start,
                cursor_position: self.bus.mcd212.cursor_position,
                cursor_control: self.bus.mcd212.cursor_control,
                frame_count: self.bus.mcd212.frame_count,
            },
            display_provenance: DisplayProvenanceSnapshot {
                cdic_buffer_hash: diagnostic_hash_bytes(&self.bus.cdic.ram),
                plane_a_hash: self
                    .bus
                    .ram
                    .first()
                    .map_or(0, |plane| diagnostic_hash_bytes(plane)),
                plane_b_hash: self
                    .bus
                    .ram
                    .get(1)
                    .map_or(0, |plane| diagnostic_hash_bytes(plane)),
                raster_hash: diagnostic_hash_pixels(
                    &self.bus.mcd212.framebuffer()
                        [..pixel_count.min(self.bus.mcd212.framebuffer().len())],
                ),
            },
            dvc: self.dvc_stats(),
            dvc_registers: self.bus.dvc.as_ref().map(Vmpeg::register_snapshot),
            disc_inserted: self.bus.disc.is_some(),
        }
    }

    /// Drain captured events in occurrence order.
    pub fn take_diagnostic_events(&mut self) -> Vec<MachineDiagnosticEvent> {
        self.diagnostic_events
            .as_mut()
            .map(|(_, events, _)| events.drain(..).collect())
            .unwrap_or_default()
    }

    fn diagnostic_probe(&self) -> DiagnosticProbe {
        let cdic = self.bus.cdic.diagnostic_snapshot();
        let dvc = self.dvc_stats().unwrap_or_default();
        let mut dvc_state = self.bus.dvc.as_ref().map(Vmpeg::register_snapshot);
        if let Some(registers) = &mut dvc_state {
            // These are free-running clocks. Retain their live values in the
            // emitted event, but do not create one event per clock change.
            registers.dclk = 0;
            registers.timer_counter = 0;
            // The shared 45 kHz timer regularly asserts ISR bit $0100. It is
            // useful context at a native-driver transition, but its periodic
            // assert/ack cadence would otherwise evict the MPEG transition
            // history from a bounded long-run capture.
            registers.fma_isr &= !0x0100;
            registers.fmv_isr &= !0x0100;
        }
        DiagnosticProbe {
            frame: self.bus.mcd212.frame_count,
            cdic_mode: cdic.disc_mode,
            cdic_lba: cdic.current_lba,
            cdic_state: [
                cdic.command,
                cdic.audio_buffer,
                cdic.x_buffer,
                cdic.z_buffer,
                cdic.data_buffer,
            ],
            cdic_interrupt: cdic.interrupt_asserted,
            dvc_errors: [
                dvc.demux_errors,
                dvc.video_errors,
                dvc.audio_errors,
                dvc.video_underflow_events,
                dvc.audio_underflow_events,
                dvc.stream_errors,
            ],
            dvc_milestones: [
                dvc.play_events,
                dvc.continue_events,
                dvc.pause_events,
                dvc.video_program_end_events,
                dvc.audio_program_end_events,
                dvc.audio_start_events,
                dvc.audio_stop_events,
                dvc.sequence_end_events,
                dvc.end_of_data_events,
                dvc.audio_stream_switch_events,
            ],
            dvc_state,
        }
    }

    fn push_diagnostic_event(&mut self, event: MachineDiagnosticEvent) {
        let Some((capacity, events, _)) = &mut self.diagnostic_events else {
            return;
        };
        if events.len() == *capacity {
            // VMPEG play/pause/end milestones are deliberately sparse and
            // must survive a long run even when per-frame and DMA evidence
            // fills the bounded ring. Evict the oldest ordinary event first.
            if let Some(index) = events
                .iter()
                .position(|event| !matches!(event, MachineDiagnosticEvent::DvcMilestone { .. }))
            {
                events.remove(index);
            } else if !matches!(event, MachineDiagnosticEvent::DvcMilestone { .. }) {
                return;
            } else {
                events.pop_front();
            }
        }
        events.push_back(event);
    }

    fn sample_diagnostics(&mut self) {
        if !self.milestone_only_diagnostics {
            self.sample_dma_and_pcl_diagnostics();
        }
        let current = self.diagnostic_probe();
        let Some((_, _, previous)) = &self.diagnostic_events else {
            return;
        };
        let previous = *previous;
        if !self.milestone_only_diagnostics && current.frame != previous.frame {
            let geometry = self.bus.mcd212.display_geometry();
            let pixel_count = geometry.raster_width * geometry.raster_height;
            self.push_diagnostic_event(MachineDiagnosticEvent::Frame {
                cycle: self.cpu.cycles,
                frame: current.frame,
                geometry,
                plane_a_hash: self
                    .bus
                    .ram
                    .first()
                    .map_or(0, |plane| diagnostic_hash_bytes(plane)),
                plane_b_hash: self
                    .bus
                    .ram
                    .get(1)
                    .map_or(0, |plane| diagnostic_hash_bytes(plane)),
                raster_hash: diagnostic_hash_pixels(
                    &self.bus.mcd212.framebuffer()
                        [..pixel_count.min(self.bus.mcd212.framebuffer().len())],
                ),
            });
        }
        if !self.milestone_only_diagnostics
            && (current.cdic_mode, current.cdic_lba) != (previous.cdic_mode, previous.cdic_lba)
        {
            self.push_diagnostic_event(MachineDiagnosticEvent::DiscPosition {
                cycle: self.cpu.cycles,
                mode: current.cdic_mode,
                lba: current.cdic_lba,
            });
        }
        if !self.milestone_only_diagnostics
            && (current.cdic_state, current.cdic_interrupt)
                != (previous.cdic_state, previous.cdic_interrupt)
        {
            self.push_diagnostic_event(MachineDiagnosticEvent::CdicState {
                cycle: self.cpu.cycles,
                command: current.cdic_state[0],
                audio_buffer: current.cdic_state[1],
                x_buffer: current.cdic_state[2],
                z_buffer: current.cdic_state[3],
                data_buffer: current.cdic_state[4],
                interrupt_asserted: current.cdic_interrupt,
            });
        }
        if !self.milestone_only_diagnostics && current.dvc_errors != previous.dvc_errors {
            self.push_diagnostic_event(MachineDiagnosticEvent::DvcCounters {
                cycle: self.cpu.cycles,
                demux_errors: current.dvc_errors[0],
                video_errors: current.dvc_errors[1],
                audio_errors: current.dvc_errors[2],
                video_underflows: current.dvc_errors[3],
                audio_underflows: current.dvc_errors[4],
                stream_errors: current.dvc_errors[5],
            });
        }
        if current.dvc_milestones != previous.dvc_milestones {
            if let Some(stats) = self.dvc_stats() {
                let dclk = self
                    .bus
                    .dvc
                    .as_ref()
                    .map_or(0, |dvc| dvc.register_snapshot().dclk);
                let geometry = self.bus.mcd212.display_geometry();
                let pixel_count = geometry.raster_width * geometry.raster_height;
                let raster_hash = diagnostic_hash_pixels(
                    &self.bus.mcd212.framebuffer()
                        [..pixel_count.min(self.bus.mcd212.framebuffer().len())],
                );
                self.push_diagnostic_event(MachineDiagnosticEvent::DvcMilestone {
                    cycle: self.cpu.cycles,
                    dclk,
                    stats: Box::new(stats),
                    raster_hash,
                });
            }
        }
        if !self.milestone_only_diagnostics && current.dvc_state != previous.dvc_state {
            if let Some(registers) = self.bus.dvc.as_ref().map(Vmpeg::register_snapshot) {
                self.push_diagnostic_event(MachineDiagnosticEvent::DvcState {
                    cycle: self.cpu.cycles,
                    registers,
                });
            }
        }
        if let Some((_, _, stored)) = &mut self.diagnostic_events {
            *stored = current;
        }
    }

    fn sample_dma_and_pcl_diagnostics(&mut self) {
        if self.diagnostic_events.is_none() {
            return;
        }
        let observations = self.bus.take_dma_diagnostic_observations();
        let guest_writes = self.bus.take_guest_memory_write_observations();
        let dvc_register_writes = self.bus.take_dvc_register_write_observations();
        let dma_register_writes = self.bus.take_dma_register_write_observations();
        let raw_guest_writes = self.bus.take_raw_guest_write_observations();
        let cycle = self.cpu.cycles;
        let mut events = Vec::new();
        {
            let Some(tracker) = &mut self.pcl_diagnostics else {
                return;
            };
            let regions = self.bus.diagnostic_ram_regions();
            events.extend(tracker.sample(cycle, &regions));
            let known_pcls = tracker.known_pcl_addresses();
            let mut pointer_writes = Vec::new();
            for write in raw_guest_writes {
                for byte_offset in 0..4 {
                    let candidate = write.address.saturating_sub(byte_offset);
                    let Some(value) = diagnostic_read_u32(&regions, candidate) else {
                        continue;
                    };
                    if known_pcls.contains(&value) {
                        if let Some(existing) = pointer_writes
                            .iter_mut()
                            .find(|(address, pcl, _)| *address == candidate && *pcl == value)
                        {
                            existing.2 |= write.changed;
                        } else {
                            pointer_writes.push((candidate, value, write.changed));
                        }
                    }
                }
            }
            for (memory_address, pcl_address, changed) in pointer_writes {
                events.push(MachineDiagnosticEvent::PclPointerWrite {
                    cycle,
                    cpu_pc: self.cpu.pc,
                    memory_address,
                    pcl_address,
                    changed,
                });
            }
            for write in guest_writes {
                events.push(MachineDiagnosticEvent::GuestMemoryWrite {
                    cycle,
                    memory_address: write.memory_address,
                    bytes: write.bytes,
                    changed_bytes: write.changed_bytes,
                    before_hash: write.before_hash,
                    after_hash: write.after_hash,
                    source_dma_address: write.source_dma_address,
                    source_dma_bytes: write.source_dma_bytes,
                    pcl_addresses: tracker.matching_buffers(write.memory_address, write.bytes),
                });
            }
            for write in dvc_register_writes {
                events.push(MachineDiagnosticEvent::DvcRegisterWrite {
                    cycle,
                    address: write.address,
                    value: write.value,
                });
            }
            for write in dma_register_writes {
                events.push(MachineDiagnosticEvent::DmaRegisterWrite {
                    cycle,
                    cpu_pc: self.cpu.pc,
                    channel: write.channel,
                    register_offset: write.register_offset,
                    value: write.value,
                });
            }
            for observation in observations {
                let (pcl_addresses, mut ownership_events) =
                    if observation.channel == 0 && observation.to_memory {
                        tracker.observe_cdic_dma(
                            cycle,
                            &regions,
                            observation.memory_address,
                            observation.bytes,
                        )
                    } else {
                        (
                            tracker.matching_buffers(observation.memory_address, observation.bytes),
                            Vec::new(),
                        )
                    };
                events.append(&mut ownership_events);
                events.push(MachineDiagnosticEvent::DmaTransfer {
                    cycle,
                    channel: observation.channel,
                    memory_address: observation.memory_address,
                    bytes: observation.bytes,
                    device_address_or_target: observation.device_address_or_target,
                    to_memory: observation.to_memory,
                    completed: observation.completed,
                    payload_hash: observation.payload_hash,
                    transport_payload_hash: observation.transport_payload_hash,
                    pcl_addresses,
                });
            }
        }
        for event in &mut events {
            if let MachineDiagnosticEvent::PclState { cpu_pc, .. } = event {
                *cpu_pc = self.cpu.pc;
            }
        }
        for event in events {
            self.push_diagnostic_event(event);
        }
    }

    /// Reset the 68070 and all host-side devices while preserving the
    /// independently powered SLAVE MCU state that requested the reset.
    fn reset_host_preserving_slave(&mut self) {
        if let Some(tracker) = &mut self.pcl_diagnostics {
            *tracker = PclOwnershipTracker::default();
        }
        self.bus.reset();
        self.bus.periph.reset();
        self.bus.mcd212.reset();
        self.bus.cdic.reset();
        self.bus.cdic.set_disc_layout(self.bus.disc.as_ref());
        self.cpu.pending_ipl = 0;
        self.cpu.reset(&mut self.bus);
    }

    /// Execute one instruction; returns CPU cycles consumed.
    pub fn step(&mut self) -> u64 {
        self.cpu.pending_ipl = self.bus.periph.pending_ipl();
        let cycles = self.cpu.step(&mut self.bus);
        let bus = &mut self.bus;
        bus.periph.tick(cycles);
        bus.slave.tick(cycles);
        if bus.slave.take_host_reset_request() {
            log::debug!("machine: SLAVE requested host reset");
            self.push_diagnostic_event(MachineDiagnosticEvent::HostReset {
                cycle: self.cpu.cycles,
            });
            self.reset_host_preserving_slave();
            self.sample_diagnostics();
            return cycles;
        }
        if let Some(atten) = bus.slave.take_attenuation() {
            bus.cdic.set_attenuation(atten);
        }
        bus.cdic.tick(cycles, bus.disc.as_ref());
        bus.service_dvc_dma(cycles);
        if let Some(dvc) = &mut bus.dvc {
            dvc.tick(cycles);
        }
        let planea = bus.ram.first().map(Vec::as_slice).unwrap_or(&[]);
        let planeb = bus.ram.get(1).map(Vec::as_slice).unwrap_or(planea);
        let external = bus.dvc.as_ref().and_then(Vmpeg::external_video);
        let frame_count = bus.mcd212.frame_count;
        bus.mcd212
            .tick_with_external(cycles, planea, planeb, external);
        if bus.mcd212.frame_count != frame_count {
            if let Some(dvc) = &mut bus.dvc {
                dvc.notify_vsync();
            }
        }
        bus.periph.in2_line = bus.slave.irq();
        bus.refresh_external_interrupts();
        bus.periph.set_int1(bus.mcd212.int_line());
        self.sample_diagnostics();
        cycles
    }

    /// Set the disc present at power-on/reset (or remove it with `None`).
    ///
    /// This is intended for construction and restoration. For a live player,
    /// use [`Machine::change_disc`] so the SLAVE reports a drive event.
    pub fn set_disc(&mut self, disc: Option<DiscImage>) {
        // DiscImage can identify CD-ROM XA Bridge media, and SlaveHle can
        // report its native type-4 status. Do not expose that status here
        // until the MCD251 sample-rate-converter origin is implemented:
        // enabling the guest's White Book path without those Xo semantics
        // fixes one title's placement while shifting another.
        self.bus.cdic.set_disc_layout(disc.as_ref());
        self.bus.slave.set_disc_present(disc.is_some());
        self.bus.disc = disc;
    }

    /// Replace media in a running player without resetting the machine.
    ///
    /// The CDIC transport is stopped and the SLAVE forwards the same B0
    /// drive-status packet that its SERVO link supplies on real hardware.
    pub fn change_disc(&mut self, disc: Option<DiscImage>) {
        let replacing = self.bus.disc.is_some() && disc.is_some();
        self.bus.cdic.media_changed(disc.as_ref());
        self.bus.slave.notify_disc_change(disc.is_some(), replacing);
        self.bus.disc = disc;
    }

    /// Drain bytes the BIOS has written to the serial port (boot console).
    pub fn take_uart_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bus.periph.tx_out)
    }

    /// Drain decoded audio (44.1 kHz interleaved stereo).
    pub fn take_audio(&mut self) -> Vec<i16> {
        let mut mixed = std::mem::take(&mut self.bus.cdic.audio_out);
        let dvc_audio = self
            .bus
            .dvc
            .as_mut()
            .map(Vmpeg::take_audio)
            .unwrap_or_default();
        if mixed.is_empty() {
            return dvc_audio;
        }
        if mixed.len() < dvc_audio.len() {
            mixed.resize(dvc_audio.len(), 0);
        }
        for (dst, src) in mixed.iter_mut().zip(dvc_audio) {
            *dst = dst.saturating_add(src);
        }
        mixed
    }
}

fn dvc_dma_observation(
    in_flight: DvcDmaDiagnosticInFlight,
    completed: bool,
) -> DmaDiagnosticObservation {
    DmaDiagnosticObservation {
        channel: 1,
        memory_address: in_flight.memory_address,
        bytes: in_flight.bytes,
        device_address_or_target: in_flight.target,
        to_memory: false,
        completed,
        payload_hash: in_flight.payload_hash,
        transport_payload_hash: in_flight.payload_hash,
    }
}

const DIAGNOSTIC_HASH_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const DIAGNOSTIC_HASH_PRIME: u64 = 0x0000_0100_0000_01B3;

fn diagnostic_hash_byte(hash: u64, byte: u8) -> u64 {
    (hash ^ u64::from(byte)).wrapping_mul(DIAGNOSTIC_HASH_PRIME)
}

fn diagnostic_hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(DIAGNOSTIC_HASH_OFFSET, |hash, byte| {
        diagnostic_hash_byte(hash, *byte)
    })
}

fn diagnostic_hash_pixels(pixels: &[u32]) -> u64 {
    pixels.iter().fold(DIAGNOSTIC_HASH_OFFSET, |hash, pixel| {
        pixel
            .to_be_bytes()
            .into_iter()
            .fold(hash, diagnostic_hash_byte)
    })
}

impl Bus68k for MachineBus {
    fn read8(&mut self, addr: u32, _fc: FnCode) -> (u8, WaitTicks) {
        (self.raw_read8(addr), 0)
    }

    fn read16(&mut self, addr: u32, _fc: FnCode) -> (u16, WaitTicks) {
        let hi = self.raw_read8(addr);
        let lo = self.raw_read8(addr.wrapping_add(1));
        (u16::from_be_bytes([hi, lo]), 0)
    }

    fn write8(&mut self, addr: u32, val: u8, _fc: FnCode) -> WaitTicks {
        self.raw_write8(addr, val);
        0
    }

    fn write16(&mut self, addr: u32, val: u16, _fc: FnCode) -> WaitTicks {
        let [hi, lo] = val.to_be_bytes();
        self.raw_write8(addr, hi);
        self.raw_write8(addr.wrapping_add(1), lo);
        0
    }

    fn iack(&mut self, level: u8) -> u8 {
        if level == 4 && self.periph.in4_line {
            match self.shared_in4_owner {
                SharedIn4Owner::Cdic => return self.cdic.intack(),
                SharedIn4Owner::Vmpeg => {
                    if let Some(dvc) = &mut self.dvc {
                        return dvc.intack();
                    }
                }
                SharedIn4Owner::Idle => {}
            }
        }
        self.periph.iack(level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boards::CDI220B;

    fn machine() -> MachineBus {
        let mut rom = vec![0u8; 512 * 1024];
        // Reset vectors: SSP $00001500, PC $004004B8 (as in real ROMs).
        rom[..8].copy_from_slice(&[0x00, 0x00, 0x15, 0x00, 0x00, 0x40, 0x04, 0xB8]);
        rom[0x4B8] = 0xAB;
        MachineBus::new(&CDI220B, rom).unwrap()
    }

    #[test]
    fn guest_write_provenance_is_bounded_to_the_latest_cdic_region() {
        let mut capture = DmaDiagnosticCapture::default();
        capture.watch_cdic_region(0x1000, 4);
        capture.observe_guest_write(0x1000, 0x10, 0x20);
        capture.observe_guest_write(0x1001, 0x30, 0x30);
        capture.observe_guest_write(0x2000, 0x40, 0x50);

        assert_eq!(capture.pending_guest_writes.len(), 1);
        let observation = capture.pending_guest_writes[0];
        assert_eq!(observation.memory_address, 0x1000);
        assert_eq!(observation.bytes, 1);
        assert_eq!(observation.changed_bytes, 1);
        assert_eq!(observation.source_dma_address, 0x1000);
        assert_eq!(observation.source_dma_bytes, 4);

        capture.watch_cdic_region(0x1002, 4);
        assert_eq!(capture.watched_cdic_regions.len(), 1);
        assert_eq!(capture.watched_cdic_regions[0].address, 0x1002);
    }

    #[test]
    fn guest_write_provenance_excludes_dma_and_retires_after_consumption() {
        let mut capture = DmaDiagnosticCapture::default();
        capture.watch_cdic_region(0x3000, 8);
        capture.cdic_dma_active = true;
        capture.observe_guest_write(0x3000, 0, 1);
        capture.cdic_dma_active = false;
        assert!(capture.pending_guest_writes.is_empty());

        capture.retire_consumed_region(0x3002, 2);
        capture.observe_guest_write(0x3000, 0, 1);
        assert!(capture.pending_guest_writes.is_empty());
    }

    #[test]
    fn ram_read_write() {
        let mut m = machine();
        m.write16(0x0000_1000, 0xBEEF, FnCode::SupervisorData);
        assert_eq!(m.read16(0x0000_1000, FnCode::SupervisorData).0, 0xBEEF);
        // Plane B RAM is a distinct block.
        m.write8(0x0020_0000, 0x42, FnCode::SupervisorData);
        assert_eq!(m.read8(0x0020_0000, FnCode::SupervisorData).0, 0x42);
        assert_ne!(m.read8(0x0000_0000, FnCode::SupervisorData).0, 0x42);
    }

    #[test]
    fn rom_maps_and_is_readonly() {
        let mut m = machine();
        assert_eq!(m.read8(0x0040_04B8, FnCode::SupervisorProgram).0, 0xAB);
        m.write8(0x0040_04B8, 0x00, FnCode::SupervisorData);
        assert_eq!(m.read8(0x0040_04B8, FnCode::SupervisorProgram).0, 0xAB);
    }

    #[test]
    fn reset_mirrors_vectors_into_ram() {
        let mut m = machine();
        m.reset();
        assert_eq!(m.read16(0x0000_0000, FnCode::SupervisorData).0, 0x0000);
        assert_eq!(m.read16(0x0000_0002, FnCode::SupervisorData).0, 0x1500);
        assert_eq!(m.read16(0x0000_0004, FnCode::SupervisorData).0, 0x0040);
        assert_eq!(m.read16(0x0000_0006, FnCode::SupervisorData).0, 0x04B8);
    }

    #[test]
    fn power_cycle_clears_volatile_memory_but_preserves_nvram() {
        let mut m = Machine::new(&CDI220B, machine().rom).unwrap();
        m.bus.ram[0][0x1000] = 0x5A;
        m.bus.ram[1][0x1000] = 0xA5;
        m.bus.cdic.ram_write8(0x100, 0x3C);
        m.bus.nvram[0x100] = 0xC3;
        m.cpu.d[0] = 0xDEAD_BEEF;

        m.power_cycle();

        assert_eq!(m.bus.ram[0][0x1000], 0);
        assert_eq!(m.bus.ram[1][0x1000], 0);
        assert_eq!(m.bus.cdic.ram_read8(0x100), 0);
        assert_eq!(m.bus.nvram[0x100], 0xC3);
        assert_eq!(m.cpu.d[0], 0);
        assert_eq!(&m.bus.ram[0][..8], &m.bus.rom[..8]);
    }

    #[test]
    fn vdsc_window_decodes_subpage() {
        let mut m = machine();
        // The MCD212 register window at 0x4FFFE0 sits above the 512 KB ROM
        // (which ends at 0x480000): the byte just below the window is open
        // bus, the window itself decodes to the VDSC stub (reads 0).
        assert_eq!(m.read8(0x004F_FFDF, FnCode::SupervisorData).0, 0xFF);
        assert_eq!(m.read8(0x004F_FFE0, FnCode::SupervisorData).0, 0);
        // ROM still decodes normally at its top byte.
        let rom_top = m.rom[0x7FFFF];
        assert_eq!(m.read8(0x0047_FFFF, FnCode::SupervisorData).0, rom_top);
    }

    #[test]
    fn nvram_read_write() {
        let mut m = machine();
        m.write8(0x0032_0004, 0x5A, FnCode::SupervisorData);
        assert_eq!(m.read8(0x0032_0004, FnCode::SupervisorData).0, 0x5A);
    }

    #[test]
    fn slave_host_reset_preserves_disc_launch_mode() {
        let mut rom = vec![0u8; 512 * 1024];
        rom[..8].copy_from_slice(&[0x00, 0x00, 0x15, 0x00, 0x00, 0x40, 0x04, 0xB8]);
        // NOP at the reset PC so Machine::step can reach the reset latch.
        rom[0x4B8..0x4BA].copy_from_slice(&[0x4E, 0x71]);
        let mut m = Machine::new(&CDI220B, rom).unwrap();
        m.bus.slave.set_disc_present(true);
        m.bus.slave.write(2, 0x8A);
        m.step();
        assert_eq!(m.cpu.pc, 0x0040_04B8);

        for byte in [0xB0, 0, 0, 0] {
            m.bus.slave.write(3, byte);
        }
        m.bus.slave.tick(15_000_000 / 4);
        assert_eq!(m.bus.slave.read(3), 0xB0);
        assert_eq!(m.bus.slave.read(3), 0x00);
        assert_eq!(m.bus.slave.read(3), 0x42);
        assert_eq!(m.bus.slave.read(3), 0x15);
    }

    #[test]
    fn diagnostics_disabled_and_enabled_execute_identically() {
        let mut rom = vec![0u8; 512 * 1024];
        rom[..8].copy_from_slice(&[0x00, 0x00, 0x15, 0x00, 0x00, 0x40, 0x04, 0xB8]);
        for instruction in rom[0x4B8..0x5B8].chunks_exact_mut(2) {
            instruction.copy_from_slice(&[0x4E, 0x71]);
        }
        let mut plain = Machine::new(&CDI220B, rom.clone()).unwrap();
        let mut observed = Machine::new(&CDI220B, rom).unwrap();
        observed.enable_diagnostics(32);
        for _ in 0..64 {
            plain.step();
            observed.step();
        }
        assert_eq!(plain.cpu.d, observed.cpu.d);
        assert_eq!(plain.cpu.a, observed.cpu.a);
        assert_eq!(plain.cpu.pc, observed.cpu.pc);
        assert_eq!(plain.cpu.sr, observed.cpu.sr);
        assert_eq!(plain.cpu.cycles, observed.cpu.cycles);
        assert_eq!(
            plain.bus.mcd212.frame_count,
            observed.bus.mcd212.frame_count
        );
        assert_eq!(
            plain.bus.mcd212.framebuffer(),
            observed.bus.mcd212.framebuffer()
        );
    }

    #[test]
    fn diagnostic_event_buffer_discards_oldest_entries_at_capacity() {
        let mut rom = vec![0u8; 512 * 1024];
        rom[..8].copy_from_slice(&[0x00, 0x00, 0x15, 0x00, 0x00, 0x40, 0x04, 0xB8]);
        let mut machine = Machine::new(&CDI220B, rom).unwrap();
        machine.enable_diagnostics(2);
        for frame in 1..=3 {
            machine.push_diagnostic_event(MachineDiagnosticEvent::Frame {
                cycle: frame,
                frame,
                geometry: machine.bus.mcd212.display_geometry(),
                plane_a_hash: 0,
                plane_b_hash: 0,
                raster_hash: 0,
            });
        }
        let events = machine.take_diagnostic_events();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0],
            MachineDiagnosticEvent::Frame { frame: 2, .. }
        ));
        assert!(matches!(
            events[1],
            MachineDiagnosticEvent::Frame { frame: 3, .. }
        ));
    }

    #[test]
    fn diagnostic_event_buffer_retains_sparse_dvc_milestones() {
        let mut rom = vec![0u8; 512 * 1024];
        rom[..8].copy_from_slice(&[0x00, 0x00, 0x15, 0x00, 0x00, 0x40, 0x04, 0xB8]);
        let mut machine = Machine::new(&CDI220B, rom).unwrap();
        machine.enable_diagnostics(2);
        machine.push_diagnostic_event(MachineDiagnosticEvent::DvcMilestone {
            cycle: 1,
            dclk: 1,
            stats: Box::new(DvcStats {
                play_events: 1,
                ..DvcStats::default()
            }),
            raster_hash: 1,
        });
        for frame in 2..=3 {
            machine.push_diagnostic_event(MachineDiagnosticEvent::Frame {
                cycle: frame,
                frame,
                geometry: machine.bus.mcd212.display_geometry(),
                plane_a_hash: 0,
                plane_b_hash: 0,
                raster_hash: frame,
            });
        }

        let events = machine.take_diagnostic_events();
        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|event| matches!(
            event,
            MachineDiagnosticEvent::DvcMilestone { stats, .. }
                if stats.play_events == 1
        )));
        assert!(events
            .iter()
            .any(|event| matches!(event, MachineDiagnosticEvent::Frame { frame: 3, .. })));
    }

    #[test]
    fn dvc_diagnostics_record_state_changes_without_clock_noise() {
        let mut machine = Machine::new(&CDI220B, machine().rom).unwrap();
        machine
            .attach_dvc(DvcConfig::new(DvcKind::Vmpeg, vec![0; 128 * 1024]).unwrap())
            .unwrap();
        machine.enable_diagnostics(8);

        machine.bus.dvc.as_mut().unwrap().tick(10_000);
        machine.sample_diagnostics();
        assert!(
            machine.take_diagnostic_events().is_empty(),
            "free-running DCLK and timer changes must not flood bounded evidence"
        );

        // FMV system-command write: select video as the DMA target.
        machine.bus.raw_write8(0x00E0_40C0, 0x80);
        machine.bus.raw_write8(0x00E0_40C1, 0x00);
        machine.sample_diagnostics();
        let events = machine.take_diagnostic_events();
        assert_eq!(events.len(), 3);
        assert!(matches!(
            events[0],
            MachineDiagnosticEvent::DvcRegisterWrite {
                address: 0x00E0_40C0,
                value: 0x80,
                ..
            }
        ));
        assert!(matches!(
            events[1],
            MachineDiagnosticEvent::DvcRegisterWrite {
                address: 0x00E0_40C1,
                value: 0,
                ..
            }
        ));
        assert!(matches!(
            events[2],
            MachineDiagnosticEvent::DvcState { registers, .. }
                if registers.dma_target == 1 && registers.dclk != 0
        ));
    }

    #[test]
    fn dvc_diagnostics_record_play_milestones_with_cumulative_counters() {
        let mut machine = Machine::new(&CDI220B, machine().rom).unwrap();
        machine
            .attach_dvc(DvcConfig::new(DvcKind::Vmpeg, vec![0; 128 * 1024]).unwrap())
            .unwrap();
        machine.enable_diagnostics(16);

        machine.bus.raw_write8(0x00E0_40C0, 0x00);
        machine.bus.raw_write8(0x00E0_40C1, 0x08);
        machine.sample_diagnostics();

        let events = machine.take_diagnostic_events();
        assert!(events.iter().any(|event| matches!(
            event,
            MachineDiagnosticEvent::DvcMilestone {
                stats,
                dclk: 0,
                ..
            } if stats.play_events == 1
        )));
    }

    #[test]
    fn milestone_only_diagnostics_omit_frame_and_register_noise() {
        let mut machine = Machine::new(&CDI220B, machine().rom).unwrap();
        machine
            .attach_dvc(DvcConfig::new(DvcKind::Vmpeg, vec![0; 128 * 1024]).unwrap())
            .unwrap();
        machine.enable_dvc_milestone_diagnostics(8);

        machine.bus.mcd212.frame_count += 1;
        machine.bus.raw_write8(0x00E0_40C0, 0x00);
        machine.bus.raw_write8(0x00E0_40C1, 0x08);
        machine.sample_diagnostics();

        let events = machine.take_diagnostic_events();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            MachineDiagnosticEvent::DvcMilestone { stats, .. }
                if stats.play_events == 1
        ));
    }

    #[test]
    fn vmpeg_interrupt_uses_the_shared_in4_daisy_chain() {
        let mut bus = machine();
        bus.attach_dvc(DvcConfig::new(DvcKind::Vmpeg, vec![0; 128 * 1024]).unwrap())
            .unwrap();
        let dvc = bus.dvc.as_mut().unwrap();
        dvc.write8(0xE0_300C, 0x00);
        dvc.write8(0xE0_300D, 0x5A);
        dvc.write8(0xE0_301C, 0x01);
        dvc.write8(0xE0_301D, 0x00);
        dvc.tick(200_000);
        assert!(dvc.irq());

        bus.refresh_external_interrupts();

        assert_eq!(bus.periph.pending_ipl(), 4);
        assert!(bus.periph.in4_line);
        assert!(!bus.periph.in5_line);
        assert_eq!(bus.iack(4), 0x5A);
    }

    #[test]
    fn shared_in4_owner_is_not_preempted_until_its_request_is_released() {
        let mut bus = machine();

        bus.arbitrate_shared_in4(true, false);
        assert_eq!(bus.shared_in4_owner, SharedIn4Owner::Cdic);
        assert_eq!(bus.periph.pending_ipl(), 4);

        bus.arbitrate_shared_in4(true, true);
        assert_eq!(bus.shared_in4_owner, SharedIn4Owner::Cdic);

        bus.arbitrate_shared_in4(false, true);
        assert_eq!(bus.shared_in4_owner, SharedIn4Owner::Vmpeg);
        assert_eq!(bus.periph.pending_ipl(), 4);

        bus.arbitrate_shared_in4(false, false);
        assert_eq!(bus.shared_in4_owner, SharedIn4Owner::Idle);
        assert_eq!(bus.periph.pending_ipl(), 0);
        assert!(!bus.periph.in5_line);
    }

    #[test]
    fn diagnostics_sample_discovered_pcl_without_an_unrelated_bus_observation() {
        let mut machine = Machine::new(&CDI220B, machine().rom).unwrap();
        machine
            .attach_dvc(DvcConfig::new(DvcKind::Vmpeg, vec![0; 128 * 1024]).unwrap())
            .unwrap();
        machine.enable_diagnostics(16);
        let pcl = 0x300usize;
        let buffer = 0x1000u32;
        {
            let ram = &mut machine.bus.ram[0];
            ram[pcl + 2] = 0x62;
            ram[pcl + 3] = 0x0F;
            ram[pcl + 6..pcl + 10].copy_from_slice(&(pcl as u32).to_be_bytes());
            ram[pcl + 10..pcl + 14].copy_from_slice(&buffer.to_be_bytes());
            ram[pcl + 14..pcl + 18].copy_from_slice(&1u32.to_be_bytes());
        }
        {
            let regions = machine.bus.diagnostic_ram_regions();
            machine
                .pcl_diagnostics
                .as_mut()
                .unwrap()
                .observe_cdic_dma(0, &regions, buffer, 2324);
        }
        machine.take_diagnostic_events();

        machine.bus.ram[0][pcl] = 1;
        machine.bus.ram[0][pcl + 24..pcl + 28].copy_from_slice(&2324u32.to_be_bytes());
        machine.sample_diagnostics();
        let expected_pc = machine.cpu.pc;

        assert!(machine
            .take_diagnostic_events()
            .iter()
            .any(|event| matches!(
                event,
                MachineDiagnosticEvent::PclState {
                    cpu_pc,
                    transition: crate::diagnostics::PclTransition::BufferFull,
                    pcl: snapshot,
                    ..
                } if *cpu_pc == expected_pc && snapshot.address == pcl as u32
            )));

        let pointer_slot = 0x00D0_0200u32;
        for (offset, byte) in (pcl as u32).to_be_bytes().into_iter().enumerate() {
            machine.bus.raw_write8(pointer_slot + offset as u32, byte);
        }
        machine.sample_diagnostics();
        assert!(machine
            .take_diagnostic_events()
            .iter()
            .any(|event| matches!(
                event,
                MachineDiagnosticEvent::PclPointerWrite {
                    memory_address,
                    pcl_address,
                    changed: true,
                    ..
                } if *memory_address == pointer_slot && *pcl_address == pcl as u32
            )));
    }

    #[test]
    fn diagnostics_record_guest_dma_register_writes_with_cpu_context() {
        let mut machine = Machine::new(&CDI220B, machine().rom).unwrap();
        machine.enable_diagnostics(8);
        machine.cpu.pc = 0x42_A100;

        machine.bus.raw_write8(0x8000_400C, 0x00);
        machine.bus.raw_write8(0x8000_400D, 0xD1);
        machine.sample_diagnostics();

        let events = machine.take_diagnostic_events();
        assert!(matches!(
            events.as_slice(),
            [
                MachineDiagnosticEvent::DmaRegisterWrite {
                    cpu_pc: 0x42_A100,
                    channel: 0,
                    register_offset: 0x400C,
                    value: 0,
                    ..
                },
                MachineDiagnosticEvent::DmaRegisterWrite {
                    cpu_pc: 0x42_A100,
                    channel: 0,
                    register_offset: 0x400D,
                    value: 0xD1,
                    ..
                }
            ]
        ));
    }

    #[test]
    fn provenance_hashes_identify_the_first_damaged_storage_stage() {
        let mut rom = vec![0u8; 512 * 1024];
        rom[..8].copy_from_slice(&[0x00, 0x00, 0x15, 0x00, 0x00, 0x40, 0x04, 0xB8]);
        let mut machine = Machine::new(&CDI220B, rom).unwrap();
        let baseline = machine.diagnostic_snapshot().display_provenance;

        machine.bus.cdic.ram[17] = 1;
        let cdic_changed = machine.diagnostic_snapshot().display_provenance;
        assert_ne!(cdic_changed.cdic_buffer_hash, baseline.cdic_buffer_hash);
        assert_eq!(cdic_changed.plane_a_hash, baseline.plane_a_hash);

        machine.bus.ram[0][23] = 1;
        let plane_changed = machine.diagnostic_snapshot().display_provenance;
        assert_ne!(plane_changed.plane_a_hash, baseline.plane_a_hash);
        assert_eq!(plane_changed.plane_b_hash, baseline.plane_b_hash);
        assert_eq!(plane_changed.raster_hash, baseline.raster_hash);
    }

    #[test]
    fn vmpeg_overlay_maps_rom_and_extension_ram() {
        let mut m = machine();
        let mut dvc_rom = vec![0; 128 * 1024];
        dvc_rom[0] = 0x12;
        dvc_rom[0x1FFFF] = 0x34;
        m.attach_dvc(DvcConfig::new(DvcKind::Vmpeg, dvc_rom).unwrap())
            .unwrap();
        assert_eq!(m.read8(0x00E4_0000, FnCode::SupervisorData).0, 0x12);
        assert_eq!(m.read8(0x00E6_0000, FnCode::SupervisorData).0, 0x12);
        assert_eq!(m.read8(0x00E7_FFFF, FnCode::SupervisorData).0, 0x34);
        m.write8(0x00D0_1234, 0xA5, FnCode::SupervisorData);
        assert_eq!(m.read8(0x00D0_1234, FnCode::SupervisorData).0, 0xA5);
    }

    #[test]
    fn vmpeg_dma_channel_one_transfers_memory_words() {
        let mut m = machine();
        m.attach_dvc(DvcConfig::new(DvcKind::Vmpeg, vec![0; 128 * 1024]).unwrap())
            .unwrap();
        m.set_dma_diagnostics_enabled(true);
        let packet = [0x00, 0x00, 0x01, 0xE0, 0x00, 0x04, 0x0F, b'a', b'b', b'c'];
        for (i, byte) in packet.iter().copied().enumerate() {
            m.raw_write8(0x3000 + i as u32, byte);
        }
        for (offset, value) in [
            (0x404A, 0x00),
            (0x404B, 0x05),
            (0x404C, 0x00),
            (0x404D, 0x00),
            (0x404E, 0x30),
            (0x404F, 0x00),
            (0x4045, 0x12),
            (0x4047, 0x80),
        ] {
            m.periph.write8(offset, value);
        }
        m.raw_write8(0xE0_40C0, 0x80);
        m.raw_write8(0xE0_40C1, 0x00);
        assert_eq!(m.service_dvc_dma(5 * DMA_SINGLE_ADDRESS_WORD_CYCLES), 5);
        let stats = m.dvc.as_ref().unwrap().stats();
        assert_eq!(stats.dma_words, 5);
        assert_eq!(stats.video_pes_packets, 1);
        assert_eq!(m.periph.read8(0x4040) & 0x80, 0x80);
        let observations = m.take_dma_diagnostic_observations();
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].memory_address, 0x3000);
        assert_eq!(observations[0].bytes, packet.len() as u32);
        assert_eq!(
            observations[0].transport_payload_hash,
            diagnostic_hash_bytes(&packet)
        );
    }

    #[test]
    fn vmpeg_dma_cycle_steal_transfers_one_word_per_service() {
        let mut m = machine();
        m.attach_dvc(DvcConfig::new(DvcKind::Vmpeg, vec![0; 128 * 1024]).unwrap())
            .unwrap();
        for (i, byte) in [0x12, 0x34, 0x56, 0x78].into_iter().enumerate() {
            m.raw_write8(0x3000 + i as u32, byte);
        }
        for (offset, value) in [
            (0x404A, 0x00),
            (0x404B, 0x02),
            (0x404C, 0x00),
            (0x404D, 0x00),
            (0x404E, 0x30),
            (0x404F, 0x00),
            (0x4044, 0x80),
            (0x4045, 0x12),
            (0x4047, 0x80),
        ] {
            m.periph.write8(offset, value);
        }
        m.raw_write8(0xE0_40C0, 0x80);
        m.raw_write8(0xE0_40C1, 0x00);

        assert_eq!(m.service_dvc_dma(10 * DMA_SINGLE_ADDRESS_WORD_CYCLES), 1);
        assert_eq!(m.periph.dma1_transfer_count(), 1);
        assert_eq!(m.dvc.as_ref().unwrap().stats().dma_words, 1);

        assert_eq!(m.service_dvc_dma(0), 1);
        assert_eq!(m.periph.dma1_transfer_count(), 0);
        assert_eq!(m.dvc.as_ref().unwrap().stats().dma_words, 2);
    }

    #[test]
    fn vmpeg_dma_burst_is_clock_paced() {
        let mut m = machine();
        m.attach_dvc(DvcConfig::new(DvcKind::Vmpeg, vec![0; 128 * 1024]).unwrap())
            .unwrap();
        for i in 0..8 {
            m.raw_write8(0x3000 + i, i as u8);
        }
        for (offset, value) in [
            (0x404A, 0x00),
            (0x404B, 0x04),
            (0x404C, 0x00),
            (0x404D, 0x00),
            (0x404E, 0x30),
            (0x404F, 0x00),
            (0x4044, 0x00),
            (0x4045, 0x12),
            (0x4047, 0x80),
        ] {
            m.periph.write8(offset, value);
        }
        m.raw_write8(0xE0_40C0, 0x80);
        m.raw_write8(0xE0_40C1, 0x00);

        assert_eq!(m.service_dvc_dma(DMA_SINGLE_ADDRESS_WORD_CYCLES - 1), 0);
        assert_eq!(m.periph.dma1_transfer_count(), 4);
        assert_eq!(m.service_dvc_dma(1), 1);
        assert_eq!(m.periph.dma1_transfer_count(), 3);
        assert_eq!(m.service_dvc_dma(2 * DMA_SINGLE_ADDRESS_WORD_CYCLES), 2);
        assert_eq!(m.periph.dma1_transfer_count(), 1);
        assert_eq!(m.service_dvc_dma(DMA_SINGLE_ADDRESS_WORD_CYCLES), 1);
        assert_eq!(m.periph.dma1_transfer_count(), 0);
    }
}
