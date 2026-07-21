// SPDX-License-Identifier: GPL-3.0-or-later
//! SCC68070 CPU state and execution loop.
//!
//! The 68070 executes the 68000 user-mode instruction set with 68010-style
//! features (MOVE from CCR, format/vector word on exception frames) and its
//! own microcycle timing. Instruction semantics are implemented from the
//! Motorola 68000 PRM and the Philips SCC68070 datasheet, cross-checked
//! against MAME (BSD-3-Clause, Ryan Holtz; see NOTICE.md).
//!
//! Decode is currently a hierarchical match in [`crate::exec`]; a 64K
//! dispatch table is a planned optimization that must not change semantics.

use crate::bus::{Bus68k, FnCode};

/// An SCC68070 external word/byte transfer occupies four CPU clock periods
/// before any device-specific wait states.  Philips SCC68070 (April 1993),
/// section 6.2, lists the bus reads/writes for every instruction alongside
/// the total clock periods; charging the transfers here lets the execution
/// code add only the instruction's internal clocks.
const BUS_ACCESS_CLOCKS: u64 = 4;

/// Status register bit positions.
pub mod sr_bits {
    pub const C: u16 = 1 << 0;
    pub const V: u16 = 1 << 1;
    pub const Z: u16 = 1 << 2;
    pub const N: u16 = 1 << 3;
    pub const X: u16 = 1 << 4;
    pub const IPL_SHIFT: u16 = 8;
    pub const IPL_MASK: u16 = 0b111 << IPL_SHIFT;
    pub const S: u16 = 1 << 13;
    pub const T: u16 = 1 << 15;
    /// Bits that physically exist in the SCC68070 SR.
    pub const SR_MASK: u16 = T | S | IPL_MASK | X | N | Z | V | C;
    pub const CCR_MASK: u16 = X | N | Z | V | C;
}

/// Exception vector numbers used by the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vector {
    BusError = 2,
    AddressError = 3,
    IllegalInstruction = 4,
    ZeroDivide = 5,
    Chk = 6,
    TrapV = 7,
    Privilege = 8,
    Trace = 9,
    LineA = 10,
    LineF = 11,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct Cpu {
    pub d: [u32; 8],
    /// Address registers; `a[7]` is the currently active stack pointer.
    pub a: [u32; 8],
    /// Shadow of the inactive stack pointer (USP when supervisor, SSP when
    /// user).
    pub sp_other: u32,
    pub pc: u32,
    pub sr: u16,
    /// Set by STOP until an interrupt arrives.
    pub stopped: bool,
    /// Pending interrupt priority level asserted by the interrupt
    /// controller (0 = none). The vector is fetched from the bus via
    /// [`Bus68k::iack`] when the interrupt is taken.
    pub pending_ipl: u8,
    /// Cycles consumed since construction (CPU clock, 15 MHz domain).
    pub cycles: u64,
    /// Total exceptions/interrupts taken (diagnostics; also lets test
    /// harnesses detect frame-format divergence from the 68000).
    pub exceptions_taken: u64,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            d: [0; 8],
            a: [0; 8],
            sp_other: 0,
            pc: 0,
            sr: 0x2700,
            stopped: false,
            pending_ipl: 0,
            cycles: 0,
            exceptions_taken: 0,
        }
    }

    /// Hardware reset: load SSP/PC from vectors 0/1, supervisor mode,
    /// interrupts masked.
    pub fn reset<B: Bus68k>(&mut self, bus: &mut B) {
        self.sr = 0x2700;
        self.stopped = false;
        self.a[7] = self.read32(bus, 0, FnCode::SupervisorProgram);
        self.pc = self.read32(bus, 4, FnCode::SupervisorProgram);
    }

    // --- Flag helpers -----------------------------------------------------

    pub fn flag(&self, bit: u16) -> bool {
        self.sr & bit != 0
    }

    pub fn set_flag(&mut self, bit: u16, v: bool) {
        if v {
            self.sr |= bit;
        } else {
            self.sr &= !bit;
        }
    }

    pub fn supervisor(&self) -> bool {
        self.flag(sr_bits::S)
    }

    pub fn data_fc(&self) -> FnCode {
        if self.supervisor() {
            FnCode::SupervisorData
        } else {
            FnCode::UserData
        }
    }

    pub fn prog_fc(&self) -> FnCode {
        if self.supervisor() {
            FnCode::SupervisorProgram
        } else {
            FnCode::UserProgram
        }
    }

    /// Write the full SR, swapping stack pointers if the S bit changes.
    pub fn set_sr(&mut self, value: u16) {
        let new = value & sr_bits::SR_MASK;
        if (self.sr ^ new) & sr_bits::S != 0 {
            std::mem::swap(&mut self.a[7], &mut self.sp_other);
        }
        self.sr = new;
    }

    pub fn set_ccr(&mut self, value: u16) {
        self.sr = (self.sr & !sr_bits::CCR_MASK) | (value & sr_bits::CCR_MASK);
    }

    // --- Bus helpers ------------------------------------------------------

    pub fn read8<B: Bus68k>(&mut self, bus: &mut B, addr: u32, fc: FnCode) -> u8 {
        let (v, wait) = bus.read8(addr, fc);
        self.cycles += BUS_ACCESS_CLOCKS + u64::from(wait);
        v
    }

    pub fn read16<B: Bus68k>(&mut self, bus: &mut B, addr: u32, fc: FnCode) -> u16 {
        let (v, wait) = bus.read16(addr, fc);
        self.cycles += BUS_ACCESS_CLOCKS + u64::from(wait);
        v
    }

    pub fn read32<B: Bus68k>(&mut self, bus: &mut B, addr: u32, fc: FnCode) -> u32 {
        let hi = self.read16(bus, addr, fc);
        let lo = self.read16(bus, addr.wrapping_add(2), fc);
        (u32::from(hi) << 16) | u32::from(lo)
    }

    pub fn write8<B: Bus68k>(&mut self, bus: &mut B, addr: u32, v: u8, fc: FnCode) {
        let wait = bus.write8(addr, v, fc);
        self.cycles += BUS_ACCESS_CLOCKS + u64::from(wait);
    }

    pub fn write16<B: Bus68k>(&mut self, bus: &mut B, addr: u32, v: u16, fc: FnCode) {
        let wait = bus.write16(addr, v, fc);
        self.cycles += BUS_ACCESS_CLOCKS + u64::from(wait);
    }

    pub fn write32<B: Bus68k>(&mut self, bus: &mut B, addr: u32, v: u32, fc: FnCode) {
        self.write16(bus, addr, (v >> 16) as u16, fc);
        self.write16(bus, addr.wrapping_add(2), v as u16, fc);
    }

    pub fn fetch16<B: Bus68k>(&mut self, bus: &mut B) -> u16 {
        let v = self.read16(bus, self.pc, self.prog_fc());
        self.pc = self.pc.wrapping_add(2);
        v
    }

    pub fn fetch32<B: Bus68k>(&mut self, bus: &mut B) -> u32 {
        let hi = self.fetch16(bus);
        let lo = self.fetch16(bus);
        (u32::from(hi) << 16) | u32::from(lo)
    }

    // --- Stack ------------------------------------------------------------

    pub fn push16<B: Bus68k>(&mut self, bus: &mut B, v: u16) {
        self.a[7] = self.a[7].wrapping_sub(2);
        self.write16(bus, self.a[7], v, self.data_fc());
    }

    pub fn push32<B: Bus68k>(&mut self, bus: &mut B, v: u32) {
        self.a[7] = self.a[7].wrapping_sub(4);
        self.write32(bus, self.a[7], v, self.data_fc());
    }

    pub fn pop16<B: Bus68k>(&mut self, bus: &mut B) -> u16 {
        let v = self.read16(bus, self.a[7], self.data_fc());
        self.a[7] = self.a[7].wrapping_add(2);
        v
    }

    pub fn pop32<B: Bus68k>(&mut self, bus: &mut B) -> u32 {
        let v = self.read32(bus, self.a[7], self.data_fc());
        self.a[7] = self.a[7].wrapping_add(4);
        v
    }

    // --- Exceptions -------------------------------------------------------

    /// Set the PC to a computed flow-control target, raising an address
    /// error for odd targets (instruction fetches must be word-aligned).
    pub fn set_pc_checked<B: Bus68k>(&mut self, bus: &mut B, target: u32) {
        if target & 1 != 0 {
            // TODO: the 68070 address-error frame is a long format; a short
            // frame is pushed until the datasheet frame layout is
            // implemented.
            self.exception(bus, Vector::AddressError as u8);
        } else {
            self.pc = target;
        }
    }

    /// Take an exception: push a 68070 short frame (format/vector word, PC,
    /// SR — 68010-style format 0) and vector.
    pub fn exception<B: Bus68k>(&mut self, bus: &mut B, vector: u8) {
        self.exceptions_taken += 1;
        let old_sr = self.sr;
        // Enter supervisor, clear trace.
        self.set_sr((self.sr | sr_bits::S) & !sr_bits::T);
        self.push16(bus, u16::from(vector) << 2); // format 0 | vector offset
        self.push32(bus, self.pc);
        self.push16(bus, old_sr);
        self.pc = self.read32(bus, u32::from(vector) * 4, FnCode::SupervisorProgram);
        // Table 23 totals include the opcode fetch and the four stack writes
        // plus two vector reads performed above.  What remains is internal
        // exception processing; CHK and divide-by-zero have longer paths.
        self.cycles += match vector {
            x if x == Vector::ZeroDivide as u8 => 33,
            x if x == Vector::Chk as u8 => 39,
            x if x == Vector::TrapV as u8 => 24,
            32..=47 => 21,
            _ => 24,
        };
    }

    /// Take an interrupt at `level` with `vector`, raising the mask.
    pub fn interrupt<B: Bus68k>(&mut self, bus: &mut B, level: u8, vector: u8) {
        self.exceptions_taken += 1;
        self.stopped = false;
        let old_sr = self.sr;
        self.set_sr((self.sr | sr_bits::S) & !sr_bits::T);
        self.sr = (self.sr & !sr_bits::IPL_MASK) | (u16::from(level) << sr_bits::IPL_SHIFT);
        self.push16(bus, u16::from(vector) << 2);
        self.push32(bus, self.pc);
        self.push16(bus, old_sr);
        self.pc = self.read32(bus, u32::from(vector) * 4, FnCode::SupervisorProgram);
        // Table 23: 65 clocks including four writes, two vector reads and an
        // assumed four-clock interrupt-acknowledge cycle.  IACK is modeled as
        // an instantaneous callback, so include those remaining 41 clocks.
        self.cycles += 41;
    }

    fn check_interrupts<B: Bus68k>(&mut self, bus: &mut B) -> bool {
        let level = self.pending_ipl;
        if level == 0 {
            return false;
        }
        let mask = ((self.sr & sr_bits::IPL_MASK) >> sr_bits::IPL_SHIFT) as u8;
        if level == 7 || level > mask {
            let vector = bus.iack(level);
            self.interrupt(bus, level, vector);
            return true;
        }
        false
    }

    /// Execute one instruction (or service one interrupt). Returns cycles
    /// consumed.
    pub fn step<B: Bus68k>(&mut self, bus: &mut B) -> u64 {
        let start = self.cycles;
        if self.check_interrupts(bus) {
            return self.cycles - start;
        }
        if self.stopped {
            self.cycles += 2;
            return self.cycles - start;
        }
        // Trace: latched at instruction start; taken after the instruction
        // completes unless it faulted (which already cleared T).
        let trace_pending = self.flag(sr_bits::T);
        let exceptions_before = self.exceptions_taken;
        let op = self.fetch16(bus);
        crate::exec::execute(self, bus, op);
        if trace_pending && self.exceptions_taken == exceptions_before && !self.stopped {
            self.exception(bus, Vector::Trace as u8);
        }
        self.cycles - start
    }
}
