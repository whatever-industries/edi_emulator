//! Sector reader for .bin files (single-bin and multi-bin).
//!
//! Strips the sync/header prefix from raw 2352-byte sectors to expose
//! 2048-byte user data. Supports both a single track reader (one file with
//! a configurable LBA offset) and a multi-track reader that splices several
//! data tracks into one contiguous ISO 9660 volume address space.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::cue::{Track, TrackType, SECTOR_SIZE_RAW};

pub const ISO_SECTOR_SIZE: usize = 2048;

#[derive(Debug, thiserror::Error)]
pub enum ReaderError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("short read at LBA {lba} (local {local})")]
    ShortRead { lba: i64, local: i64 },
    #[error("LBA {0} not covered by any data track span")]
    LbaNotCovered(i64),
    #[error("no usable data track spans")]
    NoSpans,
}

pub trait SectorReader {
    /// Read one 2048-byte user-data sector at the given absolute LBA.
    fn read_sector(&mut self, lba: u32) -> Result<[u8; ISO_SECTOR_SIZE], ReaderError>;

    /// Read a file of `size` bytes starting at `lba`.
    fn read_file(&mut self, lba: u32, size: usize) -> Result<Vec<u8>, ReaderError> {
        let mut buf = Vec::with_capacity(size);
        let sectors = size.div_ceil(ISO_SECTOR_SIZE);
        for s in 0..sectors {
            let sec = self.read_sector(lba + s as u32)?;
            buf.extend_from_slice(&sec);
        }
        buf.truncate(size);
        Ok(buf)
    }
}

/// Reader for a single data-track .bin file.
pub struct DataTrackReader {
    file: File,
    offset_lba: i64,
    skip: usize,
}

impl DataTrackReader {
    pub fn open(bin_path: &Path, offset_lba: i64, ttype: &TrackType) -> Result<Self, ReaderError> {
        Ok(Self {
            file: File::open(bin_path)?,
            offset_lba,
            skip: ttype.data_sector_skip(),
        })
    }
}

impl SectorReader for DataTrackReader {
    fn read_sector(&mut self, lba: u32) -> Result<[u8; ISO_SECTOR_SIZE], ReaderError> {
        let local = lba as i64 - self.offset_lba;
        if local < 0 {
            return Err(ReaderError::ShortRead {
                lba: lba as i64,
                local,
            });
        }
        let byte_off = (local as u64) * SECTOR_SIZE_RAW;
        self.file.seek(SeekFrom::Start(byte_off))?;
        let mut raw = [0u8; SECTOR_SIZE_RAW as usize];
        self.file
            .read_exact(&mut raw)
            .map_err(|_| ReaderError::ShortRead {
                lba: lba as i64,
                local,
            })?;
        let mut out = [0u8; ISO_SECTOR_SIZE];
        out.copy_from_slice(&raw[self.skip..self.skip + ISO_SECTOR_SIZE]);
        Ok(out)
    }
}

struct Span {
    vol_start: i64,
    vol_end: i64,
    file: File,
    offset_lba: i64,
    skip: usize,
}

/// Multi-track reader: splices several consecutive data tracks into one
/// contiguous ISO 9660 volume address space.
pub struct MultiTrackReader {
    spans: Vec<Span>,
}

impl MultiTrackReader {
    pub fn from_tracks(data_tracks: &[&Track]) -> Result<Self, ReaderError> {
        let mut spans = Vec::new();
        let mut vol_start: i64 = 0;
        for t in data_tracks {
            let duration = t.duration.unwrap_or(0) as i64;
            let bin_start = t.index_00.unwrap_or(0) as i64;
            let data_sectors = (duration - bin_start).max(0);
            if data_sectors == 0 {
                continue;
            }
            let offset_lba = vol_start - bin_start;
            let vol_end = vol_start + data_sectors;
            let file = File::open(&t.bin_file)?;
            spans.push(Span {
                vol_start,
                vol_end,
                file,
                offset_lba,
                skip: t.ttype.data_sector_skip(),
            });
            vol_start = vol_end;
        }
        if spans.is_empty() {
            return Err(ReaderError::NoSpans);
        }
        Ok(Self { spans })
    }
}

impl SectorReader for MultiTrackReader {
    fn read_sector(&mut self, lba: u32) -> Result<[u8; ISO_SECTOR_SIZE], ReaderError> {
        let l = lba as i64;
        for span in &mut self.spans {
            if l >= span.vol_start && l < span.vol_end {
                let local = l - span.offset_lba;
                let byte_off = (local as u64) * SECTOR_SIZE_RAW;
                span.file.seek(SeekFrom::Start(byte_off))?;
                let mut raw = [0u8; SECTOR_SIZE_RAW as usize];
                span.file
                    .read_exact(&mut raw)
                    .map_err(|_| ReaderError::ShortRead { lba: l, local })?;
                let mut out = [0u8; ISO_SECTOR_SIZE];
                out.copy_from_slice(&raw[span.skip..span.skip + ISO_SECTOR_SIZE]);
                return Ok(out);
            }
        }
        Err(ReaderError::LbaNotCovered(l))
    }
}
