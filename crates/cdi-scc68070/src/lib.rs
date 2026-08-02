// SPDX-License-Identifier: GPL-3.0-or-later
//! Philips SCC68070 CPU core.
//!
//! The SCC68070 is a 68000-user-ISA microprocessor with its own microcycle
//! timing, exception stack-frame formats, and a block of on-chip peripherals
//! (UART, timers, I²C, DMA, interrupt controller) memory-mapped at
//! `$80000000`. Peripheral behavior is ported with reference to MAME's
//! `src/devices/machine/scc68070.cpp` (BSD-3-Clause, Ryan Holtz) — see
//! NOTICE.md.

pub mod bus;
pub mod cpu;
pub mod ea;
pub mod exec;
pub mod periph;

pub use bus::{Bus68k, BusAccessSize, BusError, FnCode};
pub use cpu::Cpu;
pub use periph::Peripherals;

/// Master-clock tick count (30 MHz crystal domain).
pub type Ticks = u64;
