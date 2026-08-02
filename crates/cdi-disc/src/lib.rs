// SPDX-License-Identifier: GPL-3.0-or-later
//! CD-i disc image handling: CUE/BIN parsing, raw sector access in absolute
//! disc time, Mode 2 subheaders, and ECMA-130 descrambling for CD-i Ready
//! discs whose data hides in an audio track's pregap.
//!
//! All sector addressing uses **absolute frames**: frames since MSF
//! 00:00:00, where the conventional CD LBA 0 sits at absolute frame 150
//! (MSF 00:02:00).

pub mod cuesheet;
pub mod image;
pub mod inventory;
pub mod scramble;
pub mod sector;

pub use cuesheet::{parse_cue, CueFile, CueTrack, TrackMode};
pub use image::{DiscImage, TrackInfo};
pub use inventory::{
    inspect_cue, CdiFileEntry, CdiVolumeInventory, DiscContentKind, DiscFingerprint, DiscInventory,
    IsoVolumeInventory, MpegSequenceInventory, Os9ModuleInventory, RealtimeFileInventory,
    SectorClassInventory, TrackInventory, VcdEntryInventory, VcdListInventory,
    VcdNavigationInventory,
};
pub use sector::{Mode2Subheader, SectorHeader};

pub const RAW_SECTOR_SIZE: usize = 2352;

/// Minute/second/frame address (75 frames per second).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Msf {
    pub m: u8,
    pub s: u8,
    pub f: u8,
}

impl Msf {
    pub fn from_frames(frames: u32) -> Self {
        Self {
            m: (frames / (60 * 75)) as u8,
            s: ((frames / 75) % 60) as u8,
            f: (frames % 75) as u8,
        }
    }

    pub fn to_frames(self) -> u32 {
        u32::from(self.m) * 60 * 75 + u32::from(self.s) * 75 + u32::from(self.f)
    }

    /// Parse "MM:SS:FF".
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split(':');
        let m = parts.next()?.parse().ok()?;
        let s = parts.next()?.parse().ok()?;
        let f = parts.next()?.parse().ok()?;
        if parts.next().is_some() || s >= 60 || f >= 75 {
            return None;
        }
        Some(Self { m, s, f })
    }

    pub fn to_bcd(self) -> [u8; 3] {
        let bcd = |v: u8| ((v / 10) << 4) | (v % 10);
        [bcd(self.m), bcd(self.s), bcd(self.f)]
    }
}

impl std::fmt::Display for Msf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.m, self.s, self.f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msf_round_trip() {
        let msf = Msf::parse("01:43:12").unwrap();
        assert_eq!(msf.to_frames(), (60 + 43) * 75 + 12);
        assert_eq!(Msf::from_frames(msf.to_frames()), msf);
        assert_eq!(msf.to_string(), "01:43:12");
        assert_eq!(msf.to_bcd(), [0x01, 0x43, 0x12]);
    }

    #[test]
    fn msf_rejects_invalid() {
        assert!(Msf::parse("00:60:00").is_none());
        assert!(Msf::parse("00:00:75").is_none());
        assert!(Msf::parse("xx:00:00").is_none());
    }
}
