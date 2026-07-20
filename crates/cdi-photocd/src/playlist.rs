//! PLAYLIST.PCD parser (spec Section III.2.4, Figs III.7–9).
//!
//! Walks chained Play Sequences. Each sequence contains per-image display
//! timings (in 1/30 s units) and an optional CD-DA Entry with start/stop
//! MSF timestamps.

#[derive(Debug, Clone)]
pub struct ImageEntry {
    pub number: u16,
    pub display_time_ticks: u16,
    /// `display_time_ticks / 30`, or `None` if 0 (manual advance).
    pub display_time_s: Option<f32>,
    pub transition: u8,
}

#[derive(Debug, Clone)]
pub struct CddaEntry {
    pub start_msf: (u8, u8, u8),
    pub stop_msf: (u8, u8, u8),
    pub start_s: f32,
    pub stop_s: f32,
    pub attrs: u8,
}

#[derive(Debug, Clone)]
pub struct PlaySequence {
    pub offset: usize,
    pub n_images: u16,
    pub next_off: u32,
    pub prev_off: u32,
    pub images: Vec<ImageEntry>,
    pub cdda: Option<CddaEntry>,
}

fn bcd_to_int(b: u8) -> u32 {
    (b >> 4) as u32 * 10 + (b & 0x0F) as u32
}

fn msf_to_seconds(m: u8, s: u8, f: u8) -> f32 {
    bcd_to_int(m) as f32 * 60.0 + bcd_to_int(s) as f32 + bcd_to_int(f) as f32 / 75.0
}

/// Parse one Play Sequence at `offset`. Returns `None` if not a sequence header.
pub fn parse_play_sequence(data: &[u8], offset: usize) -> Option<PlaySequence> {
    if offset + 3 > data.len() {
        return None;
    }
    let header = data[offset];
    if (header >> 4) != 0x2 {
        return None;
    }
    let n_images = u16::from_be_bytes([data[offset + 1], data[offset + 2]]);

    // Layout: 1 (header) + 2 (n_images) + 4 (next) + 4 (prev) + 22 reserved = 33
    if offset + 33 > data.len() {
        return None;
    }
    let next_off = u32::from_be_bytes([
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
    ]);
    let prev_off = u32::from_be_bytes([
        data[offset + 7],
        data[offset + 8],
        data[offset + 9],
        data[offset + 10],
    ]);

    let img_start = offset + 33;
    let mut images = Vec::with_capacity(n_images as usize);
    for i in 0..n_images as usize {
        let p = img_start + i * 16;
        if p + 16 > data.len() {
            break;
        }
        let num = u16::from_be_bytes([data[p], data[p + 1]]);
        let disp = u16::from_be_bytes([data[p + 2], data[p + 3]]);
        let display_time_s = if disp > 0 {
            Some(disp as f32 / 30.0)
        } else {
            None
        };
        images.push(ImageEntry {
            number: num,
            display_time_ticks: disp,
            display_time_s,
            transition: data[p + 4],
        });
    }

    let cdda_pos = img_start + n_images as usize * 16;
    let cdda = if cdda_pos + 28 <= data.len() {
        let cd = &data[cdda_pos..cdda_pos + 28];
        let any_nonzero = cd[..6].iter().any(|&b| b != 0);
        let any_not_ff = cd[..6].iter().any(|&b| b != 0xFF);
        if any_nonzero && any_not_ff {
            Some(CddaEntry {
                start_msf: (cd[0], cd[1], cd[2]),
                stop_msf: (cd[3], cd[4], cd[5]),
                start_s: msf_to_seconds(cd[0], cd[1], cd[2]),
                stop_s: msf_to_seconds(cd[3], cd[4], cd[5]),
                attrs: cd[6],
            })
        } else {
            None
        }
    } else {
        None
    };

    Some(PlaySequence {
        offset,
        n_images,
        next_off,
        prev_off,
        images,
        cdda,
    })
}

/// Walk the Play Sequence chain. Returns all sequences reachable from the
/// first sequence by following `next_off`.
pub fn find_all_play_sequences(data: &[u8]) -> Vec<PlaySequence> {
    let mut sequences = Vec::new();
    let mut visited: Vec<usize> = Vec::new();

    // Scan for first plausible sequence header.
    let mut pos = 0usize;
    while pos < data.len() {
        let b = data[pos];
        if (b >> 4) == 0x2 && pos + 3 <= data.len() {
            let n = u16::from_be_bytes([data[pos + 1], data[pos + 2]]);
            if (1..=200).contains(&n) {
                break;
            }
        }
        pos += 1;
    }
    if pos >= data.len() {
        return sequences;
    }

    loop {
        if visited.contains(&pos) || pos >= data.len() {
            break;
        }
        visited.push(pos);
        let Some(seq) = parse_play_sequence(data, pos) else {
            break;
        };
        let next = seq.next_off as usize;
        sequences.push(seq);
        if next == 0 || next >= data.len() || next == pos {
            break;
        }
        pos = next;
    }
    sequences
}

/// Map of 1-based image number → display time in seconds, deduplicating
/// to the longest valid timing per image. Filters out-of-range values.
pub fn image_timings(sequences: &[PlaySequence], n_images: u16) -> Vec<(u16, f32)> {
    let mut out: Vec<(u16, f32)> = Vec::new();
    for seq in sequences {
        for img in &seq.images {
            let num = img.number;
            let Some(t) = img.display_time_s else {
                continue;
            };
            if num == 0 || num > n_images {
                continue;
            }
            if !(0.5..=3600.0).contains(&t) {
                continue;
            }
            if let Some(existing) = out.iter_mut().find(|(n, _)| *n == num) {
                if t > existing.1 {
                    existing.1 = t;
                }
            } else {
                out.push((num, t));
            }
        }
    }
    out.sort_by_key(|(n, _)| *n);
    out
}
