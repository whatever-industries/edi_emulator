// SPDX-License-Identifier: GPL-2.0-or-later
//! OS-9/68k module parsing and CD-i ROM identification.
//!
//! CD-i player system ROMs are collections of OS-9 memory modules. Scanning
//! for module headers gives us the module directory (useful for debugging)
//! and, via signature rules, identifies the player model a ROM belongs to.

pub mod module;
pub mod rules;

pub use module::{scan_modules, Module, ModuleType};
pub use rules::{identify_board, identify_rom, BoardType, RomType};
