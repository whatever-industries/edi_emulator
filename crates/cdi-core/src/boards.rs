// SPDX-License-Identifier: GPL-3.0-or-later
//! Concrete board and model tables.
//!
//! Transliterated from the `.brd`/`.mdl` data files shipped with CD-i
//! Emulator by "CD-i Fan" (LGPL-2.0-or-later; see NOTICE.md), currently the
//! Mono-I subset only.

use crate::board::{BoardDef, BoardEntry, DeviceKind, ModelDef, VideoStandard};

const KB: u32 = 1024;

/// Mono-I board (`mono1.brd`): MCD212 VDSC, CDIC + SLAVE.
/// Used by CD-i 200 F1, 210 F1, 220 F2.
pub static MONO1: BoardDef = BoardDef {
    name: "mono1",
    entries: &[
        BoardEntry {
            base: 0x0000_0000,
            device: DeviceKind::Ram {
                size: 512 * KB,
                name: "planea",
            },
        },
        BoardEntry {
            base: 0x0020_0000,
            device: DeviceKind::Ram {
                size: 512 * KB,
                name: "planeb",
            },
        },
        BoardEntry {
            base: 0x0030_0000,
            device: DeviceKind::Cdic { level: 4 },
        },
        BoardEntry {
            base: 0x0031_0000,
            device: DeviceKind::Slave {
                level: 2,
                vector: 26,
            },
        },
        BoardEntry {
            base: 0x0031_8000,
            device: DeviceKind::Null { size: 32 * KB },
        },
        BoardEntry {
            base: 0x0032_0000,
            device: DeviceKind::Nvram,
        },
        BoardEntry {
            base: 0x0040_0000,
            device: DeviceKind::SysRom { size: 512 * KB },
        },
        // MCD212 register window, above the 512 KB ROM range (which ends at
        // $480000). Entries later in the list win address decoding.
        BoardEntry {
            base: 0x004F_FFE0,
            device: DeviceKind::Vdsc,
        },
        BoardEntry {
            base: 0x8000_0000,
            device: DeviceKind::Cpu68070,
        },
    ],
};

pub static CDI200A: ModelDef = ModelDef {
    id: "cdi200a",
    title: "CD-i 200 F1",
    board: &MONO1,
    slave_version: "3231",
    nvram_size: 32 * KB,
    video: VideoStandard::Pal,
};

pub static CDI210A: ModelDef = ModelDef {
    id: "cdi210a",
    title: "CD-i 210 F1",
    board: &MONO1,
    slave_version: "3231",
    nvram_size: 32 * KB,
    video: VideoStandard::Pal,
};

pub static CDI220B: ModelDef = ModelDef {
    id: "cdi220b",
    title: "CD-i 220 F2",
    board: &MONO1,
    slave_version: "3231",
    nvram_size: 32 * KB,
    video: VideoStandard::Pal,
};

/// All known models, for lookup by id.
pub static MODELS: &[&ModelDef] = &[&CDI200A, &CDI210A, &CDI220B];

pub fn model_by_id(id: &str) -> Option<&'static ModelDef> {
    MODELS.iter().copied().find(|m| m.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono1_matches_reference_map() {
        // Spot-check against mono1.brd ground truth.
        let cdic = MONO1
            .entries
            .iter()
            .find(|e| matches!(e.device, DeviceKind::Cdic { .. }))
            .unwrap();
        assert_eq!(cdic.base, 0x0030_0000);
        let slave = MONO1
            .entries
            .iter()
            .find(|e| matches!(e.device, DeviceKind::Slave { .. }))
            .unwrap();
        assert_eq!(slave.base, 0x0031_0000);
        assert_eq!(
            slave.device,
            DeviceKind::Slave {
                level: 2,
                vector: 26
            }
        );
    }

    #[test]
    fn model_lookup() {
        assert_eq!(model_by_id("cdi220b").unwrap().title, "CD-i 220 F2");
        assert!(model_by_id("cdi999z").is_none());
    }
}
