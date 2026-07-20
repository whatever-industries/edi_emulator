// SPDX-License-Identifier: GPL-2.0-or-later
//! Photo CD image pack decoder (core algorithms, no I/O).
//!
//! Absorbed from the Photo CD Player project (`photocd-core`), by the same
//! owner, and relicensed to match this workspace.
//!
//! Based on: *System Description Photo CD*, Philips/Kodak, January 1992,
//! Chapter IV (Image Data Representations) and Section IV.2.5 (Photo YCC).

pub mod base;
pub mod cue;
pub mod decode;
pub mod disc;
pub mod hires;
pub mod huffman;
pub mod iso9660;
pub mod playlist;
pub mod reader;
pub mod ycc;

/// Photo CD resolution tiers (spec Section IV.2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Base16,      // 192 x 128
    Base4,       // 384 x 256
    Base,        // 768 x 512
    FourBase,    // 1536 x 1024
    SixteenBase, // 3072 x 2048
}

impl Resolution {
    pub const fn dims(self) -> (u32, u32) {
        match self {
            Resolution::Base16 => (192, 128),
            Resolution::Base4 => (384, 256),
            Resolution::Base => (768, 512),
            Resolution::FourBase => (1536, 1024),
            Resolution::SixteenBase => (3072, 2048),
        }
    }
}

/// Rotation applied to the decoded image (counter-clockwise degrees).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    None,
    Ccw90,
    R180,
    Ccw270,
}

impl Rotation {
    /// Decode the 2-bit rotation code from an Image Descriptor (spec Section III.2.3).
    pub const fn from_ipa_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0 => Rotation::None,
            1 => Rotation::Ccw90,
            2 => Rotation::R180,
            3 => Rotation::Ccw270,
            _ => unreachable!(),
        }
    }
}
