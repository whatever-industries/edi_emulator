// SPDX-License-Identifier: GPL-2.0-or-later
//! Raw sector structure: sync, header, Mode 2 subheader.

/// The 12-byte data-sector sync pattern.
pub const SYNC: [u8; 12] = [
    0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00,
];

pub fn has_sync(sector: &[u8]) -> bool {
    sector.len() >= 12 && sector[..12] == SYNC
}

/// Decoded 4-byte sector header (BCD MSF + mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectorHeader {
    pub minute: u8,
    pub second: u8,
    pub frame: u8,
    pub mode: u8,
}

fn from_bcd(v: u8) -> Option<u8> {
    let (hi, lo) = (v >> 4, v & 0xF);
    if hi > 9 || lo > 9 {
        None
    } else {
        Some(hi * 10 + lo)
    }
}

impl SectorHeader {
    /// Parse and validate the header at bytes 12..16.
    pub fn parse(sector: &[u8]) -> Option<Self> {
        if sector.len() < 16 {
            return None;
        }
        let minute = from_bcd(sector[12])?;
        let second = from_bcd(sector[13])?;
        let frame = from_bcd(sector[14])?;
        let mode = sector[15];
        if second >= 60 || frame >= 75 || mode > 2 {
            return None;
        }
        Some(Self {
            minute,
            second,
            frame,
            mode,
        })
    }

    pub fn abs_frame(&self) -> u32 {
        u32::from(self.minute) * 60 * 75 + u32::from(self.second) * 75 + u32::from(self.frame)
    }
}

/// Mode 2 subheader submode bits.
pub mod submode {
    pub const EOR: u8 = 0x01; // End of record
    pub const VIDEO: u8 = 0x02;
    pub const AUDIO: u8 = 0x04;
    pub const DATA: u8 = 0x08;
    pub const TRIGGER: u8 = 0x10;
    pub const FORM2: u8 = 0x20;
    pub const RT: u8 = 0x40; // Real-time
    pub const EOF: u8 = 0x80; // End of file
}

/// Mode 2 subheader (bytes 16..24, duplicated 16..20 == 20..24).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode2Subheader {
    pub file: u8,
    pub channel: u8,
    pub submode: u8,
    pub coding: u8,
}

impl Mode2Subheader {
    pub fn parse(sector: &[u8]) -> Option<Self> {
        if sector.len() < 24 {
            return None;
        }
        Some(Self {
            file: sector[16],
            channel: sector[17],
            submode: sector[18],
            coding: sector[19],
        })
    }

    pub fn is_form2(&self) -> bool {
        self.submode & submode::FORM2 != 0
    }

    pub fn is_audio(&self) -> bool {
        self.submode & submode::AUDIO != 0
    }

    pub fn is_video(&self) -> bool {
        self.submode & submode::VIDEO != 0
    }

    pub fn is_data(&self) -> bool {
        self.submode & submode::DATA != 0
    }

    pub fn is_realtime(&self) -> bool {
        self.submode & submode::RT != 0
    }

    /// User-data byte range within the raw sector.
    pub fn user_data_range(&self) -> std::ops::Range<usize> {
        if self.is_form2() {
            24..24 + 2324
        } else {
            24..24 + 2048
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_sector(mode: u8) -> [u8; 2352] {
        let mut s = [0u8; 2352];
        s[..12].copy_from_slice(&SYNC);
        s[12] = 0x00;
        s[13] = 0x02;
        s[14] = 0x16;
        s[15] = mode;
        s
    }

    #[test]
    fn header_parses_bcd() {
        let s = data_sector(2);
        let h = SectorHeader::parse(&s).unwrap();
        assert_eq!((h.minute, h.second, h.frame, h.mode), (0, 2, 16, 2));
        assert_eq!(h.abs_frame(), 166);
    }

    #[test]
    fn header_rejects_garbage() {
        let mut s = data_sector(2);
        s[13] = 0x77; // BCD 77 seconds: invalid
        assert!(SectorHeader::parse(&s).is_none());
        let mut s = data_sector(2);
        s[12] = 0xAB; // not BCD
        assert!(SectorHeader::parse(&s).is_none());
    }

    #[test]
    fn subheader_form_detection() {
        let mut s = data_sector(2);
        s[18] = submode::FORM2 | submode::AUDIO | submode::RT;
        let sh = Mode2Subheader::parse(&s).unwrap();
        assert!(sh.is_form2() && sh.is_audio() && sh.is_realtime());
        assert_eq!(sh.user_data_range().len(), 2324);
        s[18] = submode::DATA;
        let sh = Mode2Subheader::parse(&s).unwrap();
        assert!(!sh.is_form2() && sh.is_data());
        assert_eq!(sh.user_data_range().len(), 2048);
    }
}
