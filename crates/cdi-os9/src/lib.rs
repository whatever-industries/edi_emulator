// SPDX-License-Identifier: GPL-3.0-or-later
//! OS-9/68k module parsing and CD-i ROM identification.
//!
//! CD-i player system ROMs are collections of OS-9 memory modules. Scanning
//! their static module headers gives a useful ROM inventory and, via signature
//! rules, identifies the player model a ROM belongs to. It does not decode the
//! kernel's live module directory.

pub mod module;
pub mod rules;
pub mod runtime;

pub use module::{
    parse_csd_records, scan_modules, scan_valid_modules, CsdParseError, CsdRecord, Module,
    ModuleType,
};
pub use rules::{identify_board, identify_dvc_rom, identify_rom, BoardType, DvcRomType, RomType};
pub use runtime::{
    classify_ss_opt, inherited_path_numbers, CdiPathKind, SsOptPacketTooShort, SS_OPT_PACKET_LEN,
};
