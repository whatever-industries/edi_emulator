// SPDX-License-Identifier: GPL-2.0-or-later
//! SCC68070 on-chip peripherals: interrupt controller (LIR/PICR), timers,
//! UART, and register-level stubs for I²C/DMA/MMU.
//!
//! Ported with reference to MAME `src/devices/machine/scc68070.cpp`
//! (BSD-3-Clause, Ryan Holtz et al.) — see NOTICE.md. Register block lives
//! at `$80000000`; all registers are byte-wide at odd addresses.
//!
//! Interrupt model (matches the datasheet/MAME):
//! * External pins IN2/IN4/IN5/NMI request fixed levels 2/4/5/7; the
//!   acknowledge cycle fetches the vector from the requesting device
//!   (IN2/IN5/NMI default to autovectors 26/29/31; IN4 is the CDIC).
//! * INT1/INT2 pins are latched into LIR with software-programmed levels.
//! * On-chip sources (timer, UART RX/TX, I²C) use PICR-programmed levels.
//! * LIR/on-chip acknowledge returns vector `0x38 + level`.

/// UART status register bits.
pub const USR_RXRDY: u8 = 0x01;
pub const USR_TXRDY: u8 = 0x04;
pub const USR_TXEMT: u8 = 0x08;

/// Timer status register bits.
pub const TSR_OV0: u8 = 0x80;

/// CPU cycles per timer tick (timer clock = CLKOUT / 96).
const TIMER_DIVIDER: u64 = 96;
/// Crude TX pacing: one byte drained per this many CPU cycles.
const UART_TX_CYCLES: u64 = 1200;

#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct Peripherals {
    // Interrupt controller
    lir: u8,
    picr1: u8,
    picr2: u8,
    timer_int: bool,
    uart_rx_int: bool,
    uart_tx_int: bool,
    /// External request lines (asserted by board devices).
    pub in2_line: bool,
    pub in4_line: bool,
    pub in5_line: bool,
    pub nmi_line: bool,
    int1_line: bool,
    int2_line: bool,

    // Timers
    tsr: u8,
    tcr: u8,
    reload: u16,
    timer0: u16,
    timer1: u16,
    timer2: u16,
    timer_accum: u64,

    // UART
    umr: u8,
    usr: u8,
    ucsr: u8,
    ucr: u8,
    rx_holding: u8,
    #[cfg_attr(feature = "savestate", serde(default))]
    rx_queue: Vec<u8>,
    #[cfg_attr(feature = "savestate", serde(default))]
    tx_fifo: Vec<u8>,
    tx_accum: u64,
    /// Transmitted bytes, drained by the frontend/CLI (BIOS boot console).
    #[cfg_attr(feature = "savestate", serde(default))]
    pub tx_out: Vec<u8>,

    // Register-level stubs (serde derives lack impls for arrays > 32, so
    // store as Vec; sized in `new`).
    i2c_regs: [u8; 5],
    #[cfg_attr(feature = "savestate", serde(default))]
    dma_regs: Vec<u8>,
    #[cfg_attr(feature = "savestate", serde(default))]
    mmu_regs: Vec<u8>,
}

impl Peripherals {
    pub fn new() -> Self {
        let mut p = Self {
            dma_regs: vec![0u8; 0x70],
            mmu_regs: vec![0u8; 0x80],
            ..Self::default()
        };
        p.reset();
        p
    }

    pub fn reset(&mut self) {
        self.lir = 0;
        self.picr1 = 0;
        self.picr2 = 0;
        self.timer_int = false;
        self.uart_rx_int = false;
        self.uart_tx_int = false;
        self.tsr = 0;
        self.tcr = 0;
        self.usr = USR_TXRDY | USR_TXEMT;
        self.ucr = 0;
        self.tx_fifo.clear();
        self.rx_queue.clear();
        self.dma_regs.fill(0);
    }

    /// Assert/negate the INT1 pin (MCD212 on Mono-I): latches into LIR.
    pub fn set_int1(&mut self, state: bool) {
        if state && !self.int1_line {
            self.lir |= 0x80;
        }
        self.int1_line = state;
    }

    pub fn set_int2(&mut self, state: bool) {
        if state && !self.int2_line {
            self.lir |= 0x08;
        }
        self.int2_line = state;
    }

    /// Feed a received byte into the UART (e.g. debug terminal input).
    pub fn uart_rx(&mut self, byte: u8) {
        self.rx_queue.push(byte);
    }

    // DMA channel 0 register access (big-endian bytes at $80004000+;
    // used by the CDIC's DMA-control port).

    pub fn dma0_memory_address(&self) -> u32 {
        u32::from_be_bytes(self.dma_regs[0x0C..0x10].try_into().unwrap())
    }

    pub fn set_dma0_memory_address(&mut self, addr: u32) {
        self.dma_regs[0x0C..0x10].copy_from_slice(&addr.to_be_bytes());
    }

    pub fn dma0_transfer_count(&self) -> u16 {
        u16::from_be_bytes(self.dma_regs[0x0A..0x0C].try_into().unwrap())
    }

    /// Operation control register; bit 7 (D) = device-to-memory direction.
    pub fn dma0_operation_control(&self) -> u8 {
        self.dma_regs[0x05]
    }

    // The SCC68070 calls the external request used by VMPEG DMAREQ2, while
    // its programming model exposes it as the second register block at
    // $80004040. Keep the API named `dma1` to match the zero-based register
    // block numbering used elsewhere in this core.

    pub fn dma1_memory_address(&self) -> u32 {
        u32::from_be_bytes(self.dma_regs[0x4C..0x50].try_into().unwrap())
    }

    pub fn dma1_transfer_count(&self) -> u16 {
        u16::from_be_bytes(self.dma_regs[0x4A..0x4C].try_into().unwrap())
    }

    pub fn dma1_operation_control(&self) -> u8 {
        self.dma_regs[0x45]
    }

    pub fn dma1_active(&self) -> bool {
        self.dma_regs[0x47] & 0x80 != 0 && self.dma1_transfer_count() != 0
    }

    /// Complete one 16-bit channel-1 transfer and advance its counters.
    pub fn advance_dma1_word(&mut self) {
        let address = self.dma1_memory_address().wrapping_add(2);
        self.dma_regs[0x4C..0x50].copy_from_slice(&address.to_be_bytes());
        let remaining = self.dma1_transfer_count().saturating_sub(1);
        self.dma_regs[0x4A..0x4C].copy_from_slice(&remaining.to_be_bytes());
        if remaining == 0 {
            self.complete_dma1();
        }
    }

    pub fn complete_dma1(&mut self) {
        self.dma_regs[0x47] &= 0x7F;
        self.dma_regs[0x40] |= 0x80;
    }

    fn dma_interrupt_level(&self, base: usize) -> u8 {
        let status = self.dma_regs[base];
        let control = self.dma_regs[base + 7];
        if status & 0x80 != 0 && control & 0x08 != 0 {
            control & 7
        } else {
            0
        }
    }

    /// Highest pending interrupt level (0 = none).
    pub fn pending_ipl(&self) -> u8 {
        let external = if self.nmi_line {
            7
        } else if self.in5_line {
            5
        } else if self.in4_line {
            4
        } else if self.in2_line {
            2
        } else {
            0
        };
        let int1 = if self.lir & 0x80 != 0 {
            (self.lir >> 4) & 7
        } else {
            0
        };
        let int2 = if self.lir & 0x08 != 0 {
            self.lir & 7
        } else {
            0
        };
        let timer = if self.timer_int { self.picr1 & 7 } else { 0 };
        let uart_rx = if self.uart_rx_int {
            (self.picr2 >> 4) & 7
        } else {
            0
        };
        let uart_tx = if self.uart_tx_int { self.picr2 & 7 } else { 0 };
        let dma0 = self.dma_interrupt_level(0x00);
        let dma1 = self.dma_interrupt_level(0x40);
        external
            .max(int1)
            .max(int2)
            .max(timer)
            .max(uart_rx)
            .max(uart_tx)
            .max(dma0)
            .max(dma1)
    }

    /// Interrupt acknowledge for `level`: clears the matching latched
    /// source and returns the vector number.
    pub fn iack(&mut self, level: u8) -> u8 {
        // External pins take priority and use their own vectors
        // (autovectors by default; the CDIC on IN4 supplies its own and
        // will hook this path when implemented).
        match level {
            7 if self.nmi_line => return 24 + 7,
            5 if self.in5_line => return 24 + 5,
            4 if self.in4_line => return 24 + 4,
            2 if self.in2_line => return 24 + 2,
            _ => {}
        }
        // DMA channels use 68000 autovectors and remain asserted until the
        // channel's COC status bit is cleared by software.
        if level != 0
            && (level == self.dma_interrupt_level(0x00) || level == self.dma_interrupt_level(0x40))
        {
            return 24 + level;
        }
        if self.lir & 0x80 != 0 && level == (self.lir >> 4) & 7 {
            self.lir &= 0x7F;
        } else if self.lir & 0x08 != 0 && level == self.lir & 7 {
            self.lir &= 0xF7;
        } else if self.timer_int && level == self.picr1 & 7 {
            self.timer_int = false;
        } else if self.uart_rx_int && level == (self.picr2 >> 4) & 7 {
            self.uart_rx_int = false;
        } else if self.uart_tx_int && level == self.picr2 & 7 {
            self.uart_tx_int = false;
        }
        0x38 + level
    }

    /// Advance peripheral state by `cycles` CPU cycles.
    pub fn tick(&mut self, cycles: u64) {
        // Timer 0: free-running up-counter at CLKOUT/96; overflow reloads
        // and raises the timer interrupt (level from PICR1).
        self.timer_accum += cycles;
        while self.timer_accum >= TIMER_DIVIDER {
            self.timer_accum -= TIMER_DIVIDER;
            self.timer0 = self.timer0.wrapping_add(1);
            if self.timer0 == 0 {
                self.timer0 = self.reload;
                self.tsr |= TSR_OV0;
                self.timer_int = true;
            }
        }

        // UART TX: drain one byte per pacing interval when the transmitter
        // is enabled (UCR command bits 3:2 == 01).
        self.tx_accum += cycles;
        while self.tx_accum >= UART_TX_CYCLES {
            self.tx_accum -= UART_TX_CYCLES;
            if (self.ucr >> 2) & 3 == 1 {
                self.usr |= USR_TXRDY;
                self.uart_tx_int = true;
                if let Some(byte) = self.tx_fifo.first().copied() {
                    self.tx_fifo.remove(0);
                    self.tx_out.push(byte);
                }
                if self.tx_fifo.is_empty() {
                    self.usr |= USR_TXEMT;
                }
            }
            // UART RX: present the next queued byte when receiver enabled.
            if self.ucr & 3 == 1 && !self.rx_queue.is_empty() {
                self.rx_holding = self.rx_queue[0];
                self.usr |= USR_RXRDY;
                self.uart_rx_int = true;
            }
        }
    }

    /// Byte read at `offset` from $80000000.
    pub fn read8(&mut self, offset: u32) -> u8 {
        match offset {
            0x1001 => self.lir & 0x77,
            0x2001 => self.i2c_regs[0],
            0x2003 => self.i2c_regs[1],
            // I²C status: PIN=1 (idle, no transfer in progress).
            0x2005 => 0x80,
            0x2007 => self.i2c_regs[3],
            0x2009 => self.i2c_regs[4],
            0x2011 => self.umr,
            0x2013 => self.usr,
            0x2015 => self.ucsr,
            0x2017 => self.ucr,
            0x2019 => self.tx_fifo.last().copied().unwrap_or(0),
            0x201B => {
                self.usr &= !USR_RXRDY;
                if !self.rx_queue.is_empty() {
                    self.rx_queue.remove(0);
                }
                self.rx_holding
            }
            0x2020 => self.tsr,
            0x2021 => self.tcr,
            0x2022 => (self.reload >> 8) as u8,
            0x2023 => self.reload as u8,
            0x2024 => (self.timer0 >> 8) as u8,
            0x2025 => self.timer0 as u8,
            0x2026 => (self.timer1 >> 8) as u8,
            0x2027 => self.timer1 as u8,
            0x2028 => (self.timer2 >> 8) as u8,
            0x2029 => self.timer2 as u8,
            0x2045 => self.picr1,
            0x2047 => self.picr2,
            0x4000..=0x406F => self.dma_regs[(offset - 0x4000) as usize],
            0x8000..=0x807F => self.mmu_regs[(offset - 0x8000) as usize],
            _ => {
                log::trace!("68070 periph read8 @ +{offset:#06x} (unimplemented)");
                0
            }
        }
    }

    pub fn write8(&mut self, offset: u32, val: u8) {
        match offset {
            0x1001 => {
                // Writing a set pending bit clears it; otherwise programs
                // the INT1/INT2 priority levels.
                match val & 0x88 {
                    0x08 => self.lir &= 0xF7,
                    0x80 => self.lir &= 0x7F,
                    _ => self.lir = (self.lir & 0x88) | (val & 0x77),
                }
            }
            0x2001 => self.i2c_regs[0] = val,
            0x2003 => self.i2c_regs[1] = val,
            0x2005 => self.i2c_regs[2] = val,
            0x2007 => self.i2c_regs[3] = val,
            0x2009 => self.i2c_regs[4] = val,
            0x2011 => self.umr = val,
            0x2015 => self.ucsr = val,
            0x2017 => self.ucr = val,
            0x2019 => {
                self.tx_fifo.push(val);
                self.usr &= !USR_TXEMT;
            }
            0x2020 => self.tsr &= !val, // write-1-to-clear
            0x2021 => self.tcr = val,
            0x2022 => self.reload = (self.reload & 0x00FF) | (u16::from(val) << 8),
            0x2023 => self.reload = (self.reload & 0xFF00) | u16::from(val),
            0x2024 => self.timer0 = (self.timer0 & 0x00FF) | (u16::from(val) << 8),
            0x2025 => self.timer0 = (self.timer0 & 0xFF00) | u16::from(val),
            0x2026 => self.timer1 = (self.timer1 & 0x00FF) | (u16::from(val) << 8),
            0x2027 => self.timer1 = (self.timer1 & 0xFF00) | u16::from(val),
            0x2028 => self.timer2 = (self.timer2 & 0x00FF) | (u16::from(val) << 8),
            0x2029 => self.timer2 = (self.timer2 & 0xFF00) | u16::from(val),
            0x2045 => self.picr1 = val & 0x77,
            0x2047 => self.picr2 = val & 0x77,
            // DMA channel status is write-one-to-clear.  Treating this as
            // ordinary RAM leaves the BIOS seeing channel 0 permanently
            // busy after its first CDIC transfer.
            0x4000 | 0x4040 => {
                let index = (offset - 0x4000) as usize;
                self.dma_regs[index] &= !(val & 0xB0);
            }
            // Channel-control SO (bit 7) completes the skeleton DMA
            // operation synchronously and raises COC in the status byte,
            // matching the SCC68070 model used by the CD-i BIOS.
            0x4007 => {
                let index = (offset - 0x4000) as usize;
                self.dma_regs[index] = val & 0x7F;
                if val & 0x80 != 0 {
                    self.dma_regs[index - 7] |= 0x80;
                }
            }
            // Channel 1 is handshake-paced by the optional VMPEG device.
            // Preserve SO until MachineBus has transferred every requested
            // word, then `complete_dma1` raises COC in the status byte.
            0x4047 => {
                self.dma_regs[0x47] = val;
                if val & 0x80 != 0 && self.dma1_transfer_count() == 0 {
                    self.complete_dma1();
                }
            }
            0x4000..=0x406F => self.dma_regs[(offset - 0x4000) as usize] = val,
            0x8000..=0x807F => self.mmu_regs[(offset - 0x8000) as usize] = val,
            _ => {
                log::trace!("68070 periph write8 @ +{offset:#06x} = {val:#04x} (unimplemented)");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer0_overflow_raises_interrupt() {
        let mut p = Peripherals::new();
        p.write8(0x2045, 0x05); // timer at level 5
        p.write8(0x2022, 0xFF);
        p.write8(0x2023, 0x00); // reload = 0xFF00
        p.write8(0x2024, 0xFF);
        p.write8(0x2025, 0xFE); // timer0 = 0xFFFE: two ticks to overflow
        assert_eq!(p.pending_ipl(), 0);
        p.tick(2 * TIMER_DIVIDER);
        assert_eq!(p.pending_ipl(), 5);
        assert_eq!(p.read8(0x2020) & TSR_OV0, TSR_OV0);
        // Acknowledge clears the source and yields vector 0x38+5.
        assert_eq!(p.iack(5), 0x3D);
        assert_eq!(p.pending_ipl(), 0);
        // Counter reloaded.
        assert_eq!(p.read8(0x2024), 0xFF);
    }

    #[test]
    fn dma1_waits_for_external_handshake_and_counts_words() {
        let mut p = Peripherals::new();
        for (offset, value) in [
            (0x404A, 0x00),
            (0x404B, 0x02),
            (0x404C, 0x00),
            (0x404D, 0x00),
            (0x404E, 0x30),
            (0x404F, 0x00),
        ] {
            p.write8(offset, value);
        }
        p.write8(0x4047, 0x80);
        assert!(p.dma1_active());
        assert_eq!(p.read8(0x4040) & 0x80, 0);

        p.advance_dma1_word();
        assert_eq!(p.dma1_memory_address(), 0x3002);
        assert_eq!(p.dma1_transfer_count(), 1);
        assert!(p.dma1_active());

        p.advance_dma1_word();
        assert_eq!(p.dma1_memory_address(), 0x3004);
        assert_eq!(p.dma1_transfer_count(), 0);
        assert!(!p.dma1_active());
        assert_eq!(p.read8(0x4040) & 0x80, 0x80);
        p.write8(0x4040, 0x80);
        assert_eq!(p.read8(0x4040) & 0x80, 0);
    }

    #[test]
    fn dma_completion_interrupt_is_autovectored_until_coc_is_cleared() {
        let mut p = Peripherals::new();
        p.write8(0x404A, 0);
        p.write8(0x404B, 1);
        p.write8(0x4047, 0x80 | 0x08 | 5);
        p.advance_dma1_word();

        assert_eq!(p.pending_ipl(), 5);
        assert_eq!(p.iack(5), 29);
        assert_eq!(p.pending_ipl(), 5);

        p.write8(0x4040, 0x80);
        assert_eq!(p.pending_ipl(), 0);
    }

    #[test]
    fn int2_latch_and_ack() {
        let mut p = Peripherals::new();
        p.write8(0x1001, 0x02); // INT2 level 2
        p.set_int2(true);
        p.set_int2(false); // latched even after negation
        assert_eq!(p.pending_ipl(), 2);
        assert_eq!(p.iack(2), 0x3A);
        assert_eq!(p.pending_ipl(), 0);
    }

    #[test]
    fn in2_line_is_level_sensitive_autovector() {
        let mut p = Peripherals::new();
        p.in2_line = true;
        assert_eq!(p.pending_ipl(), 2);
        assert_eq!(p.iack(2), 26);
        // Line still asserted: stays pending until the device drops it.
        assert_eq!(p.pending_ipl(), 2);
        p.in2_line = false;
        assert_eq!(p.pending_ipl(), 0);
    }

    #[test]
    fn dma_software_start_sets_and_status_write_clears_completion() {
        let mut p = Peripherals::new();
        p.write8(0x4007, 0x80);
        assert_eq!(p.read8(0x4000), 0x80);
        assert_eq!(p.read8(0x4007), 0x00);

        p.write8(0x4000, 0xFF);
        assert_eq!(p.read8(0x4000), 0x00);
    }

    #[test]
    fn uart_tx_drains_to_output() {
        let mut p = Peripherals::new();
        p.write8(0x2017, 0x04); // TX enable
        for b in b"OK" {
            p.write8(0x2019, *b);
        }
        p.tick(UART_TX_CYCLES * 3);
        assert_eq!(p.tx_out, b"OK");
        assert_eq!(p.read8(0x2013) & USR_TXEMT, USR_TXEMT);
    }
}
