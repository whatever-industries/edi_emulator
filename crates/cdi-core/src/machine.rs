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
    /// SCC68070-clock budget for the asynchronous DMAREQ2 transfer engine.
    dvc_dma_cycles: u64,
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
            dvc_dma_cycles: 0,
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
        self.dvc_dma_cycles = 0;
    }

    pub fn attach_dvc(&mut self, config: DvcConfig) -> Result<(), String> {
        self.dvc = Some(Vmpeg::new(config)?);
        self.dvc_dma_cycles = 0;
        Ok(())
    }

    pub fn detach_dvc(&mut self) {
        self.dvc = None;
        self.dvc_dma_cycles = 0;
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
                    self.periph.in4_line = self.cdic.int_line();
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
            DevSlot::OnChip => self.periph.write8(addr, val),
            DevSlot::Cdic => match addr {
                0x0000..=0x3BFF => self.cdic.ram_write8(addr, val),
                0x3C00..=0x3FFF => {
                    if let Some(dma_word) = self.cdic.write8(addr, val) {
                        self.cdic_dma(dma_word);
                    }
                    self.periph.in4_line = self.cdic.int_line();
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
                        self.ram[block][(addr - base) as usize] = val;
                    }
                    Region::Rom { .. } => log::trace!("write to ROM @ {addr:#010x}"),
                    Region::Dev { slot, base, .. } => self.dev_write8(slot, addr - base, val),
                }
                return;
            }
        }
        log::trace!("open-bus write8 @ {addr:#010x} = {val:#04x}");
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
        log::debug!(
            "cdic dma: {} {count} words at mem {start:#010x} dev {device_at:#06x} head {:02x?}",
            if to_memory { "to-mem" } else { "to-dev" },
            &self.cdic.ram
                [device_at & 0x3FFF..(device_at & 0x3FFF) + 16.min(0x3FFF - (device_at & 0x3FFF))]
        );
        for i in 0..count {
            let mem_addr = start.wrapping_add(i * 2);
            if to_memory {
                let hi = self.cdic.ram[device_at & 0x3FFF];
                let lo = self.cdic.ram[(device_at + 1) & 0x3FFF];
                self.raw_write8(mem_addr, hi);
                self.raw_write8(mem_addr.wrapping_add(1), lo);
            } else {
                let hi = self.raw_read8(mem_addr);
                let lo = self.raw_read8(mem_addr.wrapping_add(1));
                self.cdic.ram[device_at & 0x3FFF] = hi;
                self.cdic.ram[(device_at + 1) & 0x3FFF] = lo;
            }
            device_at += 2;
        }
        self.periph
            .set_dma0_memory_address(start.wrapping_add(count * 2));
    }

    /// Advance SCC68070 DMA channel 2 (the second register block) by elapsed
    /// 15 MHz clocks while VMPEG asserts DMAREQ2. This keeps DMA throughput
    /// independent of CPU instruction count without making an entire sector
    /// appear at the transfer register instantaneously.
    fn service_dvc_dma(&mut self, elapsed_cycles: u64) -> u64 {
        let requested = self.dvc.as_ref().is_some_and(Vmpeg::dma_requested);
        if !self.periph.dma1_active() {
            self.dvc_dma_cycles = 0;
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
            if let Some(dvc) = &mut self.dvc {
                dvc.push_dma_word(word);
            }
            words += 1;
            self.periph.advance_dma1_word();
            if !self.periph.dma1_active() {
                if let Some(dvc) = &mut self.dvc {
                    dvc.finish_dma();
                }
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
        if self.dvc.as_mut().is_some_and(|dvc| dvc.write8(a24, val)) {
            return;
        }
        match self.pages[(a24 >> PAGE_SHIFT) as usize] {
            Page::Ram { block, page_base } => {
                self.ram[block][(page_base + (a24 & 0xFFF)) as usize] = val;
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
            return;
        }
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
                cdic_in4: self.bus.periph.in4_line,
                dvc_in5: self.bus.periph.in5_line,
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
        }
    }

    fn push_diagnostic_event(&mut self, event: MachineDiagnosticEvent) {
        let Some((capacity, events, _)) = &mut self.diagnostic_events else {
            return;
        };
        if events.len() == *capacity {
            events.pop_front();
        }
        events.push_back(event);
    }

    fn sample_diagnostics(&mut self) {
        let current = self.diagnostic_probe();
        let Some((_, _, previous)) = &self.diagnostic_events else {
            return;
        };
        let previous = *previous;
        if current.frame != previous.frame {
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
        if (current.cdic_mode, current.cdic_lba) != (previous.cdic_mode, previous.cdic_lba) {
            self.push_diagnostic_event(MachineDiagnosticEvent::DiscPosition {
                cycle: self.cpu.cycles,
                mode: current.cdic_mode,
                lba: current.cdic_lba,
            });
        }
        if (current.cdic_state, current.cdic_interrupt)
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
        if current.dvc_errors != previous.dvc_errors {
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
        if let Some((_, _, stored)) = &mut self.diagnostic_events {
            *stored = current;
        }
    }

    /// Reset the 68070 and all host-side devices while preserving the
    /// independently powered SLAVE MCU state that requested the reset.
    fn reset_host_preserving_slave(&mut self) {
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
        bus.periph.in4_line = bus.cdic.int_line();
        bus.periph.in5_line = bus.dvc.as_ref().is_some_and(Vmpeg::irq);
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

fn diagnostic_hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xCBF2_9CE4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

fn diagnostic_hash_pixels(pixels: &[u32]) -> u64 {
    pixels.iter().fold(0xCBF2_9CE4_8422_2325u64, |hash, pixel| {
        pixel.to_be_bytes().into_iter().fold(hash, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
        })
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
        // The CDIC on IN4 supplies its own programmed vector.
        if level == 4 && self.periph.in4_line {
            return self.cdic.intack();
        }
        if level == 5 && self.periph.in5_line {
            if let Some(dvc) = &mut self.dvc {
                return dvc.intack();
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
