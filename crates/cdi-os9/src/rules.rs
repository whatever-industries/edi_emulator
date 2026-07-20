// SPDX-License-Identifier: GPL-2.0-or-later
//! CD-i ROM / board identification from OS-9 module signatures.
//!
//! Transliterated from `cditypes.rul` by "CD-i Fan" (www.cdiemu.org),
//! licensed LGPL-2.0-or-later; see NOTICE.md. Only the rules needed for the
//! boards this emulator knows about are carried over; the fallback answer is
//! `Unknown` rather than an error so the caller can still inspect modules.

use crate::module::Module;

/// CD-i mainboard families, named as in the community rule files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardType {
    /// Mono-I: MCD212 VDSC, CDIC + SLAVE (CD-i 200 F1 / 210 F1 / 220 F2).
    Mono1,
    /// Mono-II: MCD212, DSP + SLAVE.
    Mono2,
    /// Mono-III: MCD212, CIAP (MCD211) + IKAT.
    Mono3,
    /// Mono-IV: MCD212, CIAP + IKAT.
    Mono4,
    /// Mono-VI ("OCC").
    Mono6,
    /// Mini-MMC: VSD + 2×VSC (SCC66470), CDIC + SLAVE (205/910, 220 F1, 605).
    MiniMmc,
    /// Maxi-MMC: like Mini-MMC (601/602).
    MaxiMmc,
    /// Roboco: Mono-III variant (450, Goldstar).
    Roboco,
    /// Goldstar portable board (370, most non-Philips).
    Pcdi,
    Unknown,
}

impl BoardType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Mono1 => "mono1",
            Self::Mono2 => "mono2",
            Self::Mono3 => "mono3",
            Self::Mono4 => "mono4",
            Self::Mono6 => "mono6",
            Self::MiniMmc => "minimmc",
            Self::MaxiMmc => "maximmc",
            Self::Roboco => "roboco",
            Self::Pcdi => "pcdi",
            Self::Unknown => "unknown",
        }
    }
}

/// An identified system ROM type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomType {
    /// Short id matching the community naming (`cdi220b`, ...).
    pub id: &'static str,
    pub title: &'static str,
    pub board: BoardType,
}

/// Digital Video Cartridge firmware family.
///
/// This is deliberately separate from [`RomType`]: DVC firmware is an
/// optional expansion ROM executed by the player's 68070, not a replacement
/// player system ROM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DvcRomType {
    /// MCD251 video decoder plus external DSP56001 audio subsystem.
    Vmpeg,
    /// Later integrated MCD270 audio/video decoder.
    Impeg,
    Unknown,
}

impl DvcRomType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Vmpeg => "VMPEG",
            Self::Impeg => "IMPEG",
            Self::Unknown => "unknown",
        }
    }
}

fn has(mods: &[Module], name: &str) -> bool {
    mods.iter().any(|m| m.name.eq_ignore_ascii_case(name))
}

/// Edition of a named module, if present.
///
/// The numeric conditions in `cditypes.rul` (e.g. `video #>=51`,
/// `ciapdriv #<5`) discriminate on the module edition word — validated
/// against real Mono-I ROMs (cdi220b: `video` edition 51, revision 0).
fn edition(mods: &[Module], name: &str) -> Option<u16> {
    mods.iter()
        .find(|m| m.name.eq_ignore_ascii_case(name))
        .map(|m| m.edition)
}

/// Identify the board family from a system ROM's module directory.
pub fn identify_board(mods: &[Module]) -> BoardType {
    let video_edn = edition(mods, "video");
    let ciap_edn = edition(mods, "ciapdriv");

    if has(mods, "cdapdriv") {
        match video_edn {
            Some(edn) if edn >= 51 => return BoardType::Mono1,
            Some(_) if has(mods, "sgstom") => return BoardType::MiniMmc,
            Some(_) if has(mods, "ds1216") => return BoardType::MaxiMmc,
            _ => {}
        }
    }
    if has(mods, "dspdriv") {
        return BoardType::Mono2;
    }
    if let Some(edn) = ciap_edn {
        if has(mods, "hobbes") {
            if edn < 5 {
                return BoardType::Mono3;
            }
            match video_edn {
                Some(v) if v >= 58 => return BoardType::Mono6,
                _ => return BoardType::Mono4,
            }
        }
        return BoardType::Roboco;
    }
    if has(mods, "cddrv") && has(mods, "csd_pcdi") {
        return BoardType::Pcdi;
    }
    BoardType::Unknown
}

/// Identify the specific system ROM type (player model).
pub fn identify_rom(mods: &[Module]) -> RomType {
    let board = identify_board(mods);
    match board {
        BoardType::Mono1 => {
            if has(mods, "csd_220") {
                if has(mods, "magnavox") {
                    RomType {
                        id: "cdi200a",
                        title: "Philips CD-i 200 F1 system ROM",
                        board,
                    }
                } else {
                    RomType {
                        id: "cdi220b",
                        title: "Philips CD-i 220 F2 system ROM",
                        board,
                    }
                }
            } else {
                RomType {
                    id: "cdi210a",
                    title: "Philips CD-i 210 F1 system ROM",
                    board,
                }
            }
        }
        BoardType::MiniMmc => {
            if has(mods, "csd_220") {
                RomType {
                    id: "cdi220a",
                    title: "Philips CD-i 220 F1 system ROM",
                    board,
                }
            } else if has(mods, "csd_205") {
                RomType {
                    id: "cdi205a",
                    title: "Philips CD-i 205/910 F1 system ROM",
                    board,
                }
            } else {
                RomType {
                    id: "cdi000x",
                    title: "Unknown Mini-MMC system ROM",
                    board,
                }
            }
        }
        _ => RomType {
            id: "cdi000x",
            title: "Unknown CD-i system ROM",
            board,
        },
    }
}

/// Identify an optional DVC firmware ROM from its CSD/module signatures.
///
/// VMPEG and IMPEG expose mostly identical high-level `/mv` and `/ma`
/// drivers, so the chipset-specific CSD modules are the reliable boundary.
pub fn identify_dvc_rom(mods: &[Module]) -> DvcRomType {
    if has(mods, "csd_fmvvm") || has(mods, "vmpeg") {
        DvcRomType::Vmpeg
    } else if has(mods, "csd_fmvimpeg") || (has(mods, "impeg_video") && has(mods, "impeg_audio")) {
        DvcRomType::Impeg
    } else {
        DvcRomType::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::ModuleType;

    fn m(name: &str, edition: u16) -> Module {
        Module {
            offset: 0,
            size: 0x100,
            name: name.into(),
            mod_type: ModuleType::Driver,
            language: 1,
            attributes: 0,
            revision: 0,
            edition,
            crc_ok: true,
        }
    }

    #[test]
    fn mono1_wins_even_with_sgstom_present() {
        // Real cdi220b.rom contains a `sgstom` module; video edition 51
        // must still classify it as Mono-I, not Mini-MMC.
        let mods = vec![m("cdapdriv", 28), m("video", 51), m("sgstom", 2)];
        assert_eq!(identify_board(&mods), BoardType::Mono1);
    }

    #[test]
    fn mono1_detected() {
        let mods = vec![m("kernel", 1), m("cdapdriv", 10), m("video", 51)];
        assert_eq!(identify_board(&mods), BoardType::Mono1);
    }

    #[test]
    fn minimmc_detected() {
        let mods = vec![m("cdapdriv", 10), m("video", 40), m("sgstom", 1)];
        assert_eq!(identify_board(&mods), BoardType::MiniMmc);
    }

    #[test]
    fn cdi220b_vs_cdi200a() {
        let base = || vec![m("cdapdriv", 10), m("video", 51), m("csd_220", 1)];
        assert_eq!(identify_rom(&base()).id, "cdi220b");
        let mut with_mag = base();
        with_mag.push(m("magnavox", 1));
        assert_eq!(identify_rom(&with_mag).id, "cdi200a");
    }

    #[test]
    fn dvc_firmware_is_classified_separately() {
        assert_eq!(
            identify_dvc_rom(&[m("csd_fmvvm", 1), m("fmvdrv", 1)]),
            DvcRomType::Vmpeg
        );
        assert_eq!(
            identify_dvc_rom(&[m("csd_fmvimpeg", 1), m("fmvdrv", 1)]),
            DvcRomType::Impeg
        );
        assert_eq!(identify_dvc_rom(&[m("kernel", 1)]), DvcRomType::Unknown);
    }
}
