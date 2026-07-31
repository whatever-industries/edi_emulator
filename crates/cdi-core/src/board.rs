// SPDX-License-Identifier: GPL-3.0-or-later
//! Board and model definitions.
//!
//! A board is a list of devices at base addresses — the same shape as the
//! `.brd` data files published at cdiemu.org by "CD-i Fan" (see
//! NOTICE.md), from which our tables are transliterated. A model binds a
//! board to a system ROM and per-unit parameters (`.mdl` equivalent).

/// What lives at a base address on a board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceKind {
    /// Main/plane RAM, size in bytes.
    Ram { size: u32, name: &'static str },
    /// System ROM window, size in bytes.
    SysRom { size: u32 },
    /// CD interface controller (CDIC), with IRQ level.
    Cdic { level: u8 },
    /// SLAVE MCU (HLE), IRQ level + vector.
    Slave { level: u8, vector: u8 },
    /// Battery-backed SRAM + timekeeper clock.
    Nvram,
    /// MCD212 VDSC register window.
    Vdsc,
    /// SCC68070 on-chip peripheral block.
    Cpu68070,
    /// Decoded but unpopulated space (reads as open bus, writes ignored).
    Null { size: u32 },
    /// Placeholder for devices of not-yet-supported boards (VSC/VSD, DSP,
    /// CIAP/IKAT...). Instantiating a board containing one is an error.
    Unsupported(&'static str),
}

/// One base-address entry of a board definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardEntry {
    pub base: u32,
    pub device: DeviceKind,
}

/// A mainboard: named list of devices at addresses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardDef {
    pub name: &'static str,
    pub entries: &'static [BoardEntry],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoStandard {
    Pal,
    Ntsc,
}

/// A player model: board + ROM binding + per-unit parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDef {
    /// Short id matching community naming (`cdi220b`, ...).
    pub id: &'static str,
    /// Human-readable name shown in UIs.
    pub title: &'static str,
    pub board: &'static BoardDef,
    /// SLAVE version string reported to the BIOS (e.g. "3231").
    pub slave_version: &'static str,
    /// NVRAM size in bytes.
    pub nvram_size: u32,
    /// Default video standard (many models support both; boot config).
    pub video: VideoStandard,
}
