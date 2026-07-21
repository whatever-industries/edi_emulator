// SPDX-License-Identifier: GPL-3.0-or-later
//! Loaded disc image: tracks laid out in absolute disc time, raw sector
//! access, and transparent descrambling of data sectors found in
//! audio-track regions (CD-i Ready).
//!
//! Layout rules (validated against redump-style rips):
//! * FILE contents are laid out contiguously in absolute time, in order.
//! * If the first track's earliest INDEX is 01 (its 2-second pregap is not
//!   stored), an implicit 150-frame gap precedes the first file, so a
//!   plain data disc's first file starts at absolute frame 150 (LBA 0).
//! * If the first track stores INDEX 00 at 00:00:00 (CD-i Ready rips), the
//!   file itself starts at absolute frame 0.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;

use crate::cuesheet::{parse_cue, TrackMode};
use crate::scramble::descramble_in_place;
use crate::sector::{has_sync, SectorHeader};
use crate::{Msf, RAW_SECTOR_SIZE};

#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub number: u8,
    pub mode: TrackMode,
    /// Absolute frame where the track's stored region begins (INDEX 00 if
    /// present, else INDEX 01).
    pub region_start: u32,
    /// Absolute frame of INDEX 01 (the track's official start).
    pub start: u32,
    /// Absolute frame one past the end of the track's stored region.
    pub end: u32,
}

struct FileRegion {
    file: Mutex<File>,
    /// Absolute frame of the file's first sector.
    start_abs: u32,
    sectors: u32,
}

pub struct DiscImage {
    regions: Vec<FileRegion>,
    tracks: Vec<TrackInfo>,
    leadout: u32,
}

impl DiscImage {
    pub fn load(cue_path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(cue_path)
            .map_err(|e| format!("read {}: {e}", cue_path.display()))?;
        let base_dir = cue_path.parent().unwrap_or(Path::new("."));
        let cue_files = parse_cue(&text, base_dir)?;

        // Implicit 150-frame pregap unless the first track stores INDEX 00.
        let first_track = cue_files
            .iter()
            .flat_map(|f| f.tracks.iter())
            .next()
            .ok_or("no tracks")?;
        let mut abs: u32 = if first_track.indexes.iter().any(|&(n, _)| n == 0) {
            0
        } else {
            150
        };

        let mut regions = Vec::new();
        let mut tracks = Vec::new();
        for cue_file in &cue_files {
            let file = File::open(&cue_file.path)
                .map_err(|e| format!("open {}: {e}", cue_file.path.display()))?;
            let len = file.metadata().map_err(|e| e.to_string())?.len();
            if len % RAW_SECTOR_SIZE as u64 != 0 {
                return Err(format!(
                    "{}: size {} is not a multiple of {}",
                    cue_file.path.display(),
                    len,
                    RAW_SECTOR_SIZE
                ));
            }
            let sectors = (len / RAW_SECTOR_SIZE as u64) as u32;
            let file_start = abs;
            regions.push(FileRegion {
                file: Mutex::new(file),
                start_abs: file_start,
                sectors,
            });

            for (i, track) in cue_file.tracks.iter().enumerate() {
                let first_index_off = track.indexes.first().map_or(0, |&(_, off)| off);
                let index1_off = track
                    .indexes
                    .iter()
                    .find(|&&(n, _)| n == 1)
                    .map(|&(_, off)| off)
                    .ok_or_else(|| format!("track {} has no INDEX 01", track.number))?;
                // A track's stored region ends where the next track in the
                // same file begins, or at end of file.
                let region_end = cue_file
                    .tracks
                    .get(i + 1)
                    .and_then(|t| t.indexes.first())
                    .map_or(sectors, |&(_, off)| off);
                tracks.push(TrackInfo {
                    number: track.number,
                    mode: track.mode,
                    region_start: file_start + first_index_off,
                    start: file_start + index1_off,
                    end: file_start + region_end,
                });
            }
            abs += sectors;
        }

        Ok(Self {
            regions,
            tracks,
            leadout: abs,
        })
    }

    pub fn tracks(&self) -> &[TrackInfo] {
        &self.tracks
    }

    /// Absolute frame of the lead-out.
    pub fn leadout(&self) -> u32 {
        self.leadout
    }

    pub fn leadout_msf(&self) -> Msf {
        Msf::from_frames(self.leadout)
    }

    /// The track whose stored region contains `abs`, if any.
    pub fn track_at(&self, abs: u32) -> Option<&TrackInfo> {
        self.tracks
            .iter()
            .find(|t| (t.region_start..t.end).contains(&abs))
    }

    /// Read the raw 2352-byte sector at an absolute frame. Frames inside
    /// the disc span but outside any file (the implicit pregap) read as
    /// zeros; frames past the lead-out return `None`.
    pub fn read_sector_raw(&self, abs: u32) -> Option<[u8; RAW_SECTOR_SIZE]> {
        if abs >= self.leadout {
            return None;
        }
        let mut sector = [0u8; RAW_SECTOR_SIZE];
        if let Some(region) = self
            .regions
            .iter()
            .find(|r| abs >= r.start_abs && abs < r.start_abs + r.sectors)
        {
            let offset = u64::from(abs - region.start_abs) * RAW_SECTOR_SIZE as u64;
            let mut file = region.file.lock().unwrap();
            if file.seek(SeekFrom::Start(offset)).is_err() || file.read_exact(&mut sector).is_err()
            {
                log::warn!("disc: short read at abs frame {abs}");
                return None;
            }
        }
        Some(sector)
    }

    /// Read a sector for data use: sectors that carry the data sync pattern
    /// but a scrambled payload (CD-i Ready pregap rips) are descrambled.
    pub fn read_sector_data(&self, abs: u32) -> Option<[u8; RAW_SECTOR_SIZE]> {
        let mut sector = self.read_sector_raw(abs)?;
        if has_sync(&sector) && SectorHeader::parse(&sector).is_none() {
            let mut candidate = sector;
            descramble_in_place(&mut candidate);
            if SectorHeader::parse(&candidate).is_some() {
                sector = candidate;
            }
        }
        Some(sector)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scramble;
    use crate::sector::SYNC;
    use std::io::Write;

    fn write_bin(dir: &Path, name: &str, sectors: &[[u8; RAW_SECTOR_SIZE]]) {
        let mut f = File::create(dir.join(name)).unwrap();
        for s in sectors {
            f.write_all(s).unwrap();
        }
    }

    fn data_sector(msf: Msf, tag: u8) -> [u8; RAW_SECTOR_SIZE] {
        let mut s = [tag; RAW_SECTOR_SIZE];
        s[..12].copy_from_slice(&SYNC);
        let bcd = msf.to_bcd();
        s[12..15].copy_from_slice(&bcd);
        s[15] = 2;
        s
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cdi-disc-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn plain_disc_has_implicit_pregap() {
        let dir = temp_dir("plain");
        let sectors: Vec<_> = (0..20)
            .map(|i| data_sector(Msf::from_frames(150 + i), i as u8))
            .collect();
        write_bin(&dir, "game.bin", &sectors);
        std::fs::write(
            dir.join("game.cue"),
            "FILE \"game.bin\" BINARY\n  TRACK 01 CDI/2352\n    INDEX 01 00:00:00\n",
        )
        .unwrap();

        let disc = DiscImage::load(&dir.join("game.cue")).unwrap();
        assert_eq!(disc.tracks()[0].start, 150);
        assert_eq!(disc.leadout(), 170);
        // The implicit pregap reads as zeros.
        assert_eq!(disc.read_sector_raw(0).unwrap(), [0u8; RAW_SECTOR_SIZE]);
        // First file sector at absolute 150.
        let s = disc.read_sector_raw(150).unwrap();
        assert_eq!(SectorHeader::parse(&s).unwrap().abs_frame(), 150);
        assert!(disc.read_sector_raw(170).is_none());
    }

    #[test]
    fn cdi_ready_pregap_descrambles() {
        let dir = temp_dir("ready");
        // Track 1 AUDIO with 10-sector pregap holding scrambled data
        // sectors, then 5 sectors of plain audio.
        let mut sectors = Vec::new();
        for i in 0..10u32 {
            let mut s = data_sector(Msf::from_frames(i), 0x40 + i as u8);
            scramble::descramble_in_place(&mut s); // scramble (involution)
            sectors.push(s);
        }
        for _ in 0..5 {
            sectors.push([0x11u8; RAW_SECTOR_SIZE]);
        }
        write_bin(&dir, "ready.bin", &sectors);
        std::fs::write(
            dir.join("ready.cue"),
            "FILE \"ready.bin\" BINARY\n  TRACK 01 AUDIO\n    INDEX 00 00:00:00\n    INDEX 01 00:00:10\n",
        )
        .unwrap();

        let disc = DiscImage::load(&dir.join("ready.cue")).unwrap();
        assert_eq!(disc.tracks()[0].region_start, 0);
        assert_eq!(disc.tracks()[0].start, 10);
        // Raw read returns the scrambled bytes (audio-style).
        let raw = disc.read_sector_raw(3).unwrap();
        assert!(SectorHeader::parse(&raw).is_none());
        // Data read auto-descrambles and yields a valid header + payload.
        let data = disc.read_sector_data(3).unwrap();
        let h = SectorHeader::parse(&data).unwrap();
        assert_eq!(h.abs_frame(), 3);
        assert_eq!(data[100], 0x43);
        // Plain audio stays untouched.
        assert_eq!(disc.read_sector_data(12).unwrap()[100], 0x11);
    }

    #[test]
    fn multi_file_layout_is_contiguous() {
        let dir = temp_dir("multi");
        let t1: Vec<_> = (0..8)
            .map(|i| data_sector(Msf::from_frames(150 + i), i as u8))
            .collect();
        let t2 = vec![[0x22u8; RAW_SECTOR_SIZE]; 6];
        write_bin(&dir, "t1.bin", &t1);
        write_bin(&dir, "t2.bin", &t2);
        std::fs::write(
            dir.join("disc.cue"),
            "FILE \"t1.bin\" BINARY\n  TRACK 01 CDI/2352\n    INDEX 01 00:00:00\nFILE \"t2.bin\" BINARY\n  TRACK 02 AUDIO\n    INDEX 00 00:00:00\n    INDEX 01 00:00:02\n",
        )
        .unwrap();

        let disc = DiscImage::load(&dir.join("disc.cue")).unwrap();
        let tracks = disc.tracks();
        assert_eq!(tracks[0].start, 150);
        assert_eq!(tracks[0].end, 158);
        assert_eq!(tracks[1].region_start, 158);
        assert_eq!(tracks[1].start, 160);
        assert_eq!(disc.leadout(), 164);
        assert_eq!(disc.read_sector_raw(158).unwrap()[0], 0x22);
        assert_eq!(disc.track_at(159).unwrap().number, 2);
        assert_eq!(disc.track_at(155).unwrap().number, 1);
    }
}
