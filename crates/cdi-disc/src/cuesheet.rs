// SPDX-License-Identifier: GPL-3.0-or-later
//! CUE sheet parsing (the subset used by CD-i rips: FILE/TRACK/INDEX with
//! BINARY files; CDI/2352, MODE1/2352, MODE2/2352, and AUDIO tracks).

use std::path::{Path, PathBuf};

use crate::Msf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackMode {
    Audio,
    Cdi2352,
    Mode1_2352,
    Mode2_2352,
}

impl TrackMode {
    pub fn is_data(self) -> bool {
        !matches!(self, TrackMode::Audio)
    }
}

#[derive(Debug, Clone)]
pub struct CueTrack {
    pub number: u8,
    pub mode: TrackMode,
    /// (index number, file-relative frame offset), in file order.
    pub indexes: Vec<(u8, u32)>,
}

#[derive(Debug, Clone)]
pub struct CueFile {
    pub path: PathBuf,
    pub tracks: Vec<CueTrack>,
}

/// Parse a CUE sheet; `base_dir` resolves relative FILE paths.
pub fn parse_cue(text: &str, base_dir: &Path) -> Result<Vec<CueFile>, String> {
    let mut files: Vec<CueFile> = Vec::new();

    for (lineno, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let err = |msg: &str| format!("cue line {}: {msg}: {line}", lineno + 1);
        let mut words = line.split_whitespace();
        match words.next().map(str::to_ascii_uppercase).as_deref() {
            Some("FILE") => {
                // Filename may be quoted and contain spaces; the last word
                // is the file type.
                let rest = line[4..].trim();
                let name = if let Some(stripped) = rest.strip_prefix('"') {
                    stripped
                        .split('"')
                        .next()
                        .ok_or_else(|| err("unterminated quote"))?
                } else {
                    rest.split_whitespace()
                        .next()
                        .ok_or_else(|| err("missing filename"))?
                };
                let kind = rest
                    .rsplit(|c: char| c.is_whitespace() || c == '"')
                    .find(|w| !w.is_empty())
                    .unwrap_or("");
                if !kind.eq_ignore_ascii_case("BINARY") {
                    return Err(err("only BINARY files are supported"));
                }
                files.push(CueFile {
                    path: base_dir.join(name),
                    tracks: Vec::new(),
                });
            }
            Some("TRACK") => {
                let file = files.last_mut().ok_or_else(|| err("TRACK before FILE"))?;
                let number: u8 = words
                    .next()
                    .and_then(|w| w.parse().ok())
                    .ok_or_else(|| err("bad track number"))?;
                let mode = match words.next().map(str::to_ascii_uppercase).as_deref() {
                    Some("AUDIO") => TrackMode::Audio,
                    Some("CDI/2352") => TrackMode::Cdi2352,
                    Some("MODE1/2352") => TrackMode::Mode1_2352,
                    Some("MODE2/2352") => TrackMode::Mode2_2352,
                    other => {
                        return Err(err(&format!(
                            "unsupported track mode {other:?} (2352-byte raw images only)"
                        )))
                    }
                };
                file.tracks.push(CueTrack {
                    number,
                    mode,
                    indexes: Vec::new(),
                });
            }
            Some("INDEX") => {
                let track = files
                    .last_mut()
                    .and_then(|f| f.tracks.last_mut())
                    .ok_or_else(|| err("INDEX before TRACK"))?;
                let number: u8 = words
                    .next()
                    .and_then(|w| w.parse().ok())
                    .ok_or_else(|| err("bad index number"))?;
                let msf = words
                    .next()
                    .and_then(Msf::parse)
                    .ok_or_else(|| err("bad index MSF"))?;
                track.indexes.push((number, msf.to_frames()));
            }
            // Common but irrelevant directives.
            Some(
                "REM" | "PREGAP" | "POSTGAP" | "FLAGS" | "CATALOG" | "PERFORMER" | "TITLE" | "ISRC"
                | "SONGWRITER" | "CDTEXTFILE",
            ) => {}
            Some(_) | None => log::debug!("cue: ignoring line: {line}"),
        }
    }

    if files.iter().all(|f| f.tracks.is_empty()) {
        return Err("cue sheet contains no tracks".into());
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cdi_ready_cue() {
        let text = r#"
FILE "Alien Gate (USA) (Rev 1) (Track 1).bin" BINARY
  TRACK 01 AUDIO
    INDEX 00 00:00:00
    INDEX 01 01:43:12
FILE "Alien Gate (USA) (Rev 1) (Track 2).bin" BINARY
  TRACK 02 AUDIO
    INDEX 00 00:00:00
    INDEX 01 00:02:27
"#;
        let files = parse_cue(text, Path::new("/discs")).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].tracks[0].number, 1);
        assert_eq!(files[0].tracks[0].mode, TrackMode::Audio);
        assert_eq!(
            files[0].tracks[0].indexes,
            vec![(0, 0), (1, (60 + 43) * 75 + 12)]
        );
        assert_eq!(
            files[1].path.to_string_lossy(),
            "/discs/Alien Gate (USA) (Rev 1) (Track 2).bin"
        );
    }

    #[test]
    fn parses_single_track_cdi() {
        let text =
            "FILE \"CD Shoot (Europe).bin\" BINARY\n  TRACK 01 CDI/2352\n    INDEX 01 00:00:00\n";
        let files = parse_cue(text, Path::new(".")).unwrap();
        assert_eq!(files[0].tracks[0].mode, TrackMode::Cdi2352);
        assert_eq!(files[0].tracks[0].indexes, vec![(1, 0)]);
    }

    #[test]
    fn rejects_unsupported_mode() {
        let text = "FILE \"x.bin\" BINARY\n  TRACK 01 MODE1/2048\n    INDEX 01 00:00:00\n";
        assert!(parse_cue(text, Path::new(".")).is_err());
    }
}
