// SPDX-License-Identifier: GPL-3.0-or-later
//! Bus interface the CPU core drives.

/// 68k function codes (supervisor/user, program/data), used by address
/// decoding on some boards and required for accurate bus error reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FnCode {
    UserData,
    UserProgram,
    SupervisorData,
    SupervisorProgram,
    Cpu,
}

/// Additional wait-state ticks incurred by a bus access (master-clock units).
pub type WaitTicks = u32;

/// Width of the external bus cycle that was terminated by `BERRN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusAccessSize {
    Byte,
    Word,
}

/// Information retained by the SCC68070 for a recoverable bus-error frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusError {
    pub address: u32,
    pub function_code: FnCode,
    pub size: BusAccessSize,
    pub read: bool,
    pub write_data: u16,
}

/// Memory system as seen by the SCC68070.
///
/// All CPU accesses are 8- or 16-bit; 32-bit operations are performed as two
/// 16-bit cycles by the core itself.
pub trait Bus68k {
    fn read8(&mut self, addr: u32, fc: FnCode) -> (u8, WaitTicks);
    fn read16(&mut self, addr: u32, fc: FnCode) -> (u16, WaitTicks);
    fn write8(&mut self, addr: u32, val: u8, fc: FnCode) -> WaitTicks;
    fn write16(&mut self, addr: u32, val: u16, fc: FnCode) -> WaitTicks;

    /// Return and clear the first bus error raised since the previous call.
    fn take_bus_error(&mut self) -> Option<BusError> {
        None
    }

    /// Interrupt acknowledge for `level`: return the vector number and clear
    /// the acknowledged request at its source. Defaults to autovectoring.
    fn iack(&mut self, level: u8) -> u8 {
        24 + level
    }
}
