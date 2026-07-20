//! .cue sheet parser (single-bin and multi-bin aware).
//!
//! Handles `FILE`, `TRACK`, `INDEX 00`, `INDEX 01`, and quoted file paths.
//! Computes per-track durations in sectors from either the next track's LBA
//! (single-bin) or the file size (multi-bin / last track).

use std::fs;
use std::path::{Path, PathBuf};

pub const SECTOR_SIZE_RAW: u64 = 2352;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackType {
    Mode1_2352,
    Mode2_2352,
    Audio,
    Other(String),
}

impl TrackType {
    pub fn parse(s: &str) -> Self {
        let u = s.to_uppercase();
        if u.contains("MODE1") {
            TrackType::Mode1_2352
        } else if u.contains("MODE2") {
            TrackType::Mode2_2352
        } else if u.contains("AUDIO") {
            TrackType::Audio
        } else {
            TrackType::Other(u)
        }
    }

    pub fn is_data(&self) -> bool {
        matches!(self, TrackType::Mode1_2352 | TrackType::Mode2_2352)
    }

    pub fn is_audio(&self) -> bool {
        matches!(self, TrackType::Audio)
    }

    /// Byte offset into a 2352-byte sector to the 2048-byte user data region.
    pub fn data_sector_skip(&self) -> usize {
        match self {
            TrackType::Mode1_2352 => 16, // 12 sync + 4 header
            _ => 24,                     // MODE2/XA: 12 + 4 + 8 subheader
        }
    }
}

#[derive(Debug, Clone)]
pub struct Track {
    pub number: u32,
    pub ttype: TrackType,
    pub bin_file: PathBuf,
    pub index_00: Option<u32>,
    pub index_01: Option<u32>,
    /// Sector count of this track, filled after parse.
    pub duration: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum CueError {
    #[error("i/o error reading cue: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid MSF timestamp: {0}")]
    BadMsf(String),
    #[error("no tracks found in cue")]
    NoTracks,
}

/// MM:SS:FF → LBA (75 frames/s).
pub fn msf_to_lba(s: &str) -> Result<u32, CueError> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    if parts.len() != 3 {
        return Err(CueError::BadMsf(s.into()));
    }
    let mm: u32 = parts[0].parse().map_err(|_| CueError::BadMsf(s.into()))?;
    let ss: u32 = parts[1].parse().map_err(|_| CueError::BadMsf(s.into()))?;
    let ff: u32 = parts[2].parse().map_err(|_| CueError::BadMsf(s.into()))?;
    Ok((mm * 60 + ss) * 75 + ff)
}

/// Split a cue line into tokens; double-quoted strings are a single token.
fn tokenize(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    for ch in line.chars() {
        match ch {
            '"' => in_q = !in_q,
            ' ' | '\t' if !in_q => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn parse_cue(cue_path: &Path) -> Result<Vec<Track>, CueError> {
    let cue_text = fs::read_to_string(cue_path)?;
    let cue_dir = cue_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let mut tracks: Vec<Track> = Vec::new();
    let mut current_bin: Option<PathBuf> = None;
    let mut current: Option<Track> = None;

    for line in cue_text.lines() {
        let tokens = tokenize(line.trim());
        if tokens.is_empty() {
            continue;
        }
        let cmd = tokens[0].to_uppercase();
        match cmd.as_str() {
            "FILE" => {
                if tokens.len() >= 2 {
                    current_bin = Some(cue_dir.join(&tokens[1]));
                }
            }
            "TRACK" => {
                if let Some(t) = current.take() {
                    tracks.push(t);
                }
                let number = tokens
                    .get(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or((tracks.len() as u32) + 1);
                let ttype = tokens
                    .get(2)
                    .map(|s| TrackType::parse(s))
                    .unwrap_or(TrackType::Other("UNKNOWN".into()));
                let bin = current_bin.clone().unwrap_or_default();
                current = Some(Track {
                    number,
                    ttype,
                    bin_file: bin,
                    index_00: None,
                    index_01: None,
                    duration: None,
                });
            }
            "INDEX" => {
                if let Some(ref mut t) = current {
                    let idx: u32 = tokens
                        .get(1)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(u32::MAX);
                    if let Some(msf) = tokens.get(2) {
                        let lba = msf_to_lba(msf)?;
                        match idx {
                            0 => t.index_00 = Some(lba),
                            1 => t.index_01 = Some(lba),
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(t) = current.take() {
        tracks.push(t);
    }
    if tracks.is_empty() {
        return Err(CueError::NoTracks);
    }
    compute_durations(&mut tracks);
    Ok(tracks)
}

fn compute_durations(tracks: &mut [Track]) {
    let n = tracks.len();
    for i in 0..n {
        let is_multi = i == 0 || tracks[i].bin_file != tracks[i - 1].bin_file;
        if is_multi {
            if let Ok(meta) = fs::metadata(&tracks[i].bin_file) {
                let size = meta.len();
                tracks[i].duration = Some((size / SECTOR_SIZE_RAW) as u32);
                continue;
            }
        }
        if i + 1 < n {
            if let (Some(next), Some(this)) = (tracks[i + 1].index_01, tracks[i].index_01) {
                tracks[i].duration = Some(next - this);
                continue;
            }
        }
        // Last track of single-bin
        if let Ok(meta) = fs::metadata(&tracks[i].bin_file) {
            let total = (meta.len() / SECTOR_SIZE_RAW) as u32;
            let start = tracks[i].index_01.unwrap_or(0);
            tracks[i].duration = Some(total.saturating_sub(start));
        }
    }
}
