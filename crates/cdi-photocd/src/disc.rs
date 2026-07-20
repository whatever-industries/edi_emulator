//! Top-level: open a Photo CD disc from a .cue path.
//!
//! Parses the cue, picks a data track whose .bin contains a valid PVD,
//! opens a SectorReader, walks to PHOTO_CD/IMAGES/, and returns the list
//! of IMG*.PCD entries together with parsed INFO.PCD metadata.

use std::path::{Path, PathBuf};

use crate::cue::{parse_cue, CueError, Track};
use crate::iso9660::{find_entry, list_directory, read_pvd, DirEntry, DiscFormat, IsoError, Pvd};
use crate::playlist::{find_all_play_sequences, PlaySequence};
use crate::reader::{DataTrackReader, MultiTrackReader, ReaderError, SectorReader};

#[derive(Debug, thiserror::Error)]
pub enum DiscError {
    #[error("cue: {0}")]
    Cue(#[from] CueError),
    #[error("reader: {0}")]
    Reader(#[from] ReaderError),
    #[error("iso9660: {0}")]
    Iso(#[from] IsoError),
    #[error("no data track with a valid PVD")]
    NoPvd,
    #[error("PHOTO_CD directory not found")]
    NoPhotoCdDir,
    #[error("PHOTO_CD/IMAGES directory not found")]
    NoImagesDir,
}

#[derive(Debug, Clone)]
pub struct ImageEntry {
    pub name: String,
    pub lba: u32,
    pub size: u32,
    /// Kodak Photo CD (USA) raw uncompressed RGB variants, one per tier.
    /// `None` for standard Image Pack discs.
    pub rgb_variants: Option<RgbVariants>,
}

/// One raw uncompressed RGB variant on a Kodak Photo CD (USA) disc.
///
/// The file is `width * height * 3` bytes of row-major R,G,B (no header).
#[derive(Debug, Clone, Copy)]
pub struct RgbVariant {
    pub lba: u32,
    pub size: u32,
    pub width: u32,
    pub height: u32,
}

/// The three resolution tiers present (or absent) on a Kodak USA disc.
/// Indexed 0=Base (768x512 / _512), 1=4Base (1536x1024 / _1K), 2=16Base (3072x2048 / _2K).
#[derive(Debug, Clone, Copy, Default)]
pub struct RgbVariants {
    pub variants: [Option<RgbVariant>; 3],
}

impl RgbVariants {
    /// Return the variant for an exact tier index (0..=2), if present.
    pub fn get(&self, tier: usize) -> Option<&RgbVariant> {
        self.variants.get(tier).and_then(|v| v.as_ref())
    }

    /// Return the best available variant for the requested tier, falling
    /// back in the order Python does: Base (512) → 4Base (1K) → 16Base (2K).
    pub fn best_for(&self, tier: usize) -> Option<&RgbVariant> {
        if let Some(v) = self.get(tier) {
            return Some(v);
        }
        for t in 0..3 {
            if let Some(v) = self.get(t) {
                return Some(v);
            }
        }
        None
    }

    /// The highest tier index present on the disc (0=Base, 1=4Base, 2=16Base).
    pub fn max_tier(&self) -> usize {
        for t in (0..3).rev() {
            if self.variants[t].is_some() {
                return t;
            }
        }
        0
    }
}

#[derive(Debug, Clone)]
pub struct ImageDescriptor {
    pub lba: u32,
    /// Resolution order: 0=Base, 1=4Base, 2=16Base.
    pub resolution: u8,
    /// 2-bit CCW rotation code: 0=0°, 1=90°, 2=180°, 3=270°.
    pub rotation: u8,
}

#[derive(Debug, Clone, Default)]
pub struct DiscInfo {
    pub disc_id: String,
    pub spec_version: String,
    pub serial: String,
    pub n_images: u16,
    pub n_sessions: u8,
    /// Bits 7-4 of byte 33 of INFO.PCD: highest resolution on disc.
    pub res_highest: u8,
    /// Bits 3-0 of byte 33 of INFO.PCD: lowest resolution on disc.
    pub res_lowest: u8,
    pub writer_vendor: String,
    pub writer_product: String,
    pub creation_unix_ts: u32,
    pub image_descriptors: Vec<ImageDescriptor>,
}

/// Parsed INFO.PCD content (spec Section III.2.3).
///
/// Layout:
///   0- 7   "PHOTO_CD"
///   8- 9   spec version (major, minor)
///  10-21   serial (12 bytes)
///  22-25   creation unix ts (BE u32)
///  30-31   n images (BE u16)
///  33      res highest/lowest nibbles
///  37      n sessions
///  38+     session descriptors (68 bytes each)
///  then    6-byte image descriptors, one per image
pub fn parse_info_pcd(data: &[u8]) -> DiscInfo {
    let mut info = DiscInfo::default();
    if data.len() < 38 {
        return info;
    }
    if &data[0..8] != b"PHOTO_CD" {
        // Still try — disc_id is informational.
    }
    info.disc_id = String::from_utf8_lossy(&data[0..8])
        .trim_end_matches('\0')
        .to_string();
    info.spec_version = format!("{}.{:02}", data[8], data[9]);
    info.serial = String::from_utf8_lossy(&data[10..22]).trim().to_string();
    info.creation_unix_ts = u32::from_be_bytes([data[22], data[23], data[24], data[25]]);
    info.n_images = u16::from_be_bytes([data[30], data[31]]);
    info.res_highest = (data[33] >> 4) & 0x0F;
    info.res_lowest = data[33] & 0x0F;
    info.n_sessions = data[37];

    // Session descriptor #1 at byte 38.
    if data.len() >= 38 + 68 {
        let sd = &data[38..38 + 68];
        info.writer_vendor = String::from_utf8_lossy(&sd[8..16])
            .replace('\0', " ")
            .trim()
            .to_string();
        info.writer_product = String::from_utf8_lossy(&sd[16..32])
            .replace('\0', " ")
            .trim()
            .to_string();
    }

    // Image descriptors after session descriptors.
    let img_desc_off = 38 + info.n_sessions as usize * 68;
    let n = info.n_images as usize;
    info.image_descriptors.reserve(n);
    for i in 0..n {
        let off = img_desc_off + i * 6;
        if off + 6 > data.len() {
            break;
        }
        let lba = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        let attr = data[off + 4];
        info.image_descriptors.push(ImageDescriptor {
            lba,
            resolution: (attr >> 2) & 0x03,
            rotation: attr & 0x03,
        });
    }
    info
}

/// Opened disc: reader + images + info + audio tracks.
pub struct OpenedDisc {
    pub reader: Box<dyn SectorReader>,
    pub images: Vec<ImageEntry>,
    pub info: DiscInfo,
    pub audio_tracks: Vec<Track>,
    pub pvd: Pvd,
    pub play_sequences: Vec<PlaySequence>,
}

/// Find the first data track whose bin has a valid PVD/HSG signature at
/// volume sector 16. Returns the track index in `tracks`.
///
/// For multi-bin volumes the PVD is at byte offset 16 * 2352 of the track's
/// bin file (probe with a local DataTrackReader at offset_lba=0). For
/// single-bin the probe uses the track's index_01 as offset.
fn find_filesystem_track(tracks: &[Track]) -> Option<usize> {
    // Detect single-bin layout: any non-first track sharing its file with track 0.
    let is_single_bin =
        !tracks.is_empty() && tracks[1..].iter().any(|t| t.bin_file == tracks[0].bin_file);

    for (i, t) in tracks.iter().enumerate() {
        if !t.ttype.is_data() {
            continue;
        }
        if t.bin_file.as_os_str().is_empty() || !t.bin_file.is_file() {
            continue;
        }
        let offset: i64 = if is_single_bin {
            t.index_01.unwrap_or(0) as i64
        } else {
            0
        };
        let Ok(mut r) = DataTrackReader::open(&t.bin_file, offset, &t.ttype) else {
            continue;
        };
        if let Ok(sec) = r.read_sector(16) {
            if &sec[1..6] == b"CD001" || &sec[9..14] == b"CDROM" {
                return Some(i);
            }
        }
    }
    None
}

/// Parse cue, open a reader spanning all data tracks from the PVD track on,
/// and walk PHOTO_CD/IMAGES/.
pub fn open_disc(cue_path: &Path) -> Result<OpenedDisc, DiscError> {
    let tracks = parse_cue(cue_path)?;
    let pvd_track_idx = find_filesystem_track(&tracks).ok_or(DiscError::NoPvd)?;
    let pvd_track_num = tracks[pvd_track_idx].number;

    // Detect single-bin layout: any non-first track sharing its file with track 0.
    let is_single_bin =
        !tracks.is_empty() && tracks[1..].iter().any(|t| t.bin_file == tracks[0].bin_file);

    let mut reader: Box<dyn SectorReader> = if is_single_bin {
        // Single .bin for the whole disc: use a plain DataTrackReader with
        // the PVD track's index_01 as the offset.
        let t = &tracks[pvd_track_idx];
        let offset = t.index_01.unwrap_or(0) as i64;
        Box::new(DataTrackReader::open(&t.bin_file, offset, &t.ttype)?)
    } else {
        // Multi-bin: splice every data track from the PVD track onward.
        let data_tracks_in_volume: Vec<&Track> = tracks
            .iter()
            .filter(|t| t.ttype.is_data() && t.number >= pvd_track_num)
            .collect();
        Box::new(MultiTrackReader::from_tracks(&data_tracks_in_volume)?)
    };

    let pvd = read_pvd(&mut *reader)?;

    // Kodak Photo CD (USA): High Sierra filesystem, raw .RGB files at
    // root level instead of standard PHOTO_CD/ Image Packs.
    if pvd.format == DiscFormat::HighSierra {
        return open_kodak_usa(reader, pvd, &tracks);
    }

    let photo_cd = find_entry(&mut *reader, pvd.root_lba, pvd.root_size, "PHOTO_CD")?;

    // INFO.PCD (only when PHOTO_CD/ exists).
    let info = match &photo_cd {
        Some(pc) => match find_entry(&mut *reader, pc.lba, pc.size, "INFO.PCD")? {
            Some(e) => {
                let data = reader.read_file(e.lba, e.size as usize)?;
                parse_info_pcd(&data)
            }
            None => DiscInfo::default(),
        },
        None => DiscInfo::default(),
    };

    // Locate image-pack entries. Three layouts:
    //   1) Standard: PHOTO_CD/IMAGES/IMG####.PCD
    //   2) Non-compliant: .PCD files in the root directory
    //   3) Non-compliant: .PCD files one subdirectory deep (e.g. Aktuelles)
    let mut images: Vec<ImageEntry> = match &photo_cd {
        Some(pc) => {
            let images_dir = find_entry(&mut *reader, pc.lba, pc.size, "IMAGES")?
                .ok_or(DiscError::NoImagesDir)?;
            list_directory(&mut *reader, images_dir.lba, images_dir.size)?
                .into_iter()
                .filter(|e: &DirEntry| e.name.to_uppercase().ends_with(".PCD"))
                .map(|e| ImageEntry {
                    name: e.name,
                    lba: e.lba,
                    size: e.size,
                    rgb_variants: None,
                })
                .collect()
        }
        None => find_image_packs(&mut *reader, pvd.root_lba, pvd.root_size)?,
    };
    images.sort_by(|a, b| a.name.cmp(&b.name));

    if images.is_empty() && photo_cd.is_none() {
        return Err(DiscError::NoPhotoCdDir);
    }

    // PLAYLIST.PCD (optional; only on compliant discs with PHOTO_CD/).
    let play_sequences = match &photo_cd {
        Some(pc) => match find_entry(&mut *reader, pc.lba, pc.size, "PLAYLIST.PCD")? {
            Some(e) => {
                let data = reader.read_file(e.lba, e.size as usize)?;
                find_all_play_sequences(&data)
            }
            None => Vec::new(),
        },
        None => Vec::new(),
    };

    let audio_tracks: Vec<Track> = tracks
        .iter()
        .filter(|t| t.ttype.is_audio())
        .cloned()
        .collect();

    Ok(OpenedDisc {
        reader,
        images,
        info,
        audio_tracks,
        pvd,
        play_sequences,
    })
}

/// Convenience: from an ImageEntry, return the full image-pack bytes needed
/// for Base + hires decoding. Reads up to `max_sectors` sectors.
pub fn read_image_pack<R: SectorReader + ?Sized>(
    reader: &mut R,
    image: &ImageEntry,
    max_sectors: usize,
) -> Result<Vec<u8>, ReaderError> {
    let mut out = Vec::with_capacity(max_sectors * 2048);
    for s in 0..max_sectors {
        match reader.read_sector(image.lba + s as u32) {
            Ok(sec) => out.extend_from_slice(&sec),
            Err(_) => break,
        }
    }
    Ok(out)
}

/// Check whether the sector after `lba` starts with the `PCD_IPI\0` magic,
/// which identifies a Photo CD Image Pack file.
fn is_image_pack<R: SectorReader + ?Sized>(reader: &mut R, lba: u32) -> bool {
    match reader.read_sector(lba + 1) {
        Ok(sec) => &sec[0..8] == b"PCD_IPI\x00",
        Err(_) => false,
    }
}

/// Scan the root directory (and one level of subdirectories) for `.PCD`
/// Image Pack files on non-compliant discs that lack a `PHOTO_CD/` dir.
/// Mirrors Python's `_find_image_packs`.
fn find_image_packs<R: SectorReader + ?Sized>(
    reader: &mut R,
    root_lba: u32,
    root_size: u32,
) -> Result<Vec<ImageEntry>, IsoError> {
    let root = list_directory(reader, root_lba, root_size)?;
    let mut out: Vec<ImageEntry> = Vec::new();

    for e in &root {
        if e.is_dir {
            continue;
        }
        if !e.name.to_uppercase().ends_with(".PCD") {
            continue;
        }
        if is_image_pack(reader, e.lba) {
            out.push(ImageEntry {
                name: e.name.clone(),
                lba: e.lba,
                size: e.size,
                rgb_variants: None,
            });
        }
    }

    if out.is_empty() {
        for d in &root {
            if !d.is_dir {
                continue;
            }
            let Ok(sub) = list_directory(reader, d.lba, d.size) else {
                continue;
            };
            for e in sub {
                if e.is_dir {
                    continue;
                }
                if !e.name.to_uppercase().ends_with(".PCD") {
                    continue;
                }
                if is_image_pack(reader, e.lba) {
                    out.push(ImageEntry {
                        name: e.name.clone(),
                        lba: e.lba,
                        size: e.size,
                        rgb_variants: None,
                    });
                }
            }
            if !out.is_empty() {
                break;
            }
        }
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Read a raw Kodak USA .RGB file into an RGB byte buffer of length
/// `width * height * 3`.  The file is plain R,G,B row-major with no header.
pub fn read_raw_rgb_variant<R: SectorReader + ?Sized>(
    reader: &mut R,
    variant: &RgbVariant,
) -> Result<Vec<u8>, ReaderError> {
    let want = (variant.width as usize) * (variant.height as usize) * 3;
    let bytes = reader.read_file(variant.lba, want)?;
    Ok(bytes)
}

/// Kodak Photo CD (USA) disc opener: High Sierra root contains .RGB files
/// instead of PHOTO_CD/IMAGES/ Image Packs.
///
/// Naming convention: `BASE_512.RGB`, `BASE_1K.RGB`, `BASE_2K.RGB`.
fn open_kodak_usa(
    mut reader: Box<dyn SectorReader>,
    pvd: Pvd,
    tracks: &[Track],
) -> Result<OpenedDisc, DiscError> {
    let root = list_directory(&mut *reader, pvd.root_lba, pvd.root_size)?;

    // Gather .RGB files, grouped by base name; key 0=512/Base, 1=1K/4Base, 2=2K/16Base.
    let mut groups: Vec<(String, [Option<RgbVariant>; 3])> = Vec::new();
    for e in root {
        let upper = e.name.to_uppercase();
        if !upper.ends_with(".RGB") {
            continue;
        }
        let stem = &upper[..upper.len() - 4];
        let (base, tier, (w, h)) = if let Some(b) = stem.strip_suffix("_2K") {
            (b.to_string(), 2usize, (3072u32, 2048u32))
        } else if let Some(b) = stem.strip_suffix("_1K") {
            (b.to_string(), 1usize, (1536u32, 1024u32))
        } else if let Some(b) = stem.strip_suffix("_512") {
            (b.to_string(), 0usize, (768u32, 512u32))
        } else {
            // Unrecognized suffix: treat as a standalone Base image.
            (stem.to_string(), 0usize, (768u32, 512u32))
        };

        let variant = RgbVariant {
            lba: e.lba,
            size: e.size,
            width: w,
            height: h,
        };

        match groups.iter_mut().find(|(b, _)| b == &base) {
            Some((_, vs)) => vs[tier] = Some(variant),
            None => {
                let mut vs: [Option<RgbVariant>; 3] = [None, None, None];
                vs[tier] = Some(variant);
                groups.push((base, vs));
            }
        }
    }

    groups.sort_by(|a, b| a.0.cmp(&b.0));

    let images: Vec<ImageEntry> = groups
        .into_iter()
        .map(|(base, vs)| {
            // Primary entry: prefer Base, then 4Base, then 16Base.
            let primary = vs[0].or(vs[1]).or(vs[2]).expect("at least one variant");
            ImageEntry {
                name: format!("{}.RGB", base),
                lba: primary.lba,
                size: primary.size,
                rgb_variants: Some(RgbVariants { variants: vs }),
            }
        })
        .collect();

    let info = DiscInfo {
        n_images: images.len() as u16,
        n_sessions: 1,
        ..DiscInfo::default()
    };

    let audio_tracks: Vec<Track> = tracks
        .iter()
        .filter(|t| t.ttype.is_audio())
        .cloned()
        .collect();

    Ok(OpenedDisc {
        reader,
        images,
        info,
        audio_tracks,
        pvd,
        play_sequences: Vec::new(),
    })
}

/// Just for convenience: find the cue's directory.
#[allow(dead_code)]
fn cue_dir(cue: &Path) -> PathBuf {
    cue.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic INFO.PCD: header, one session descriptor, and two
    /// image descriptors (spec Section III.2.3 layout).
    fn synthetic_info_pcd() -> Vec<u8> {
        let mut data = vec![0u8; 38 + 68 + 2 * 6];
        data[0..8].copy_from_slice(b"PHOTO_CD");
        data[8] = 1; // spec version major
        data[9] = 0; // spec version minor
        data[10..22].copy_from_slice(b"TESTSERIAL12");
        data[22..26].copy_from_slice(&0x1234_5678u32.to_be_bytes());
        data[30..32].copy_from_slice(&2u16.to_be_bytes());
        data[33] = (3 << 4) | 1; // res highest 3, lowest 1
        data[37] = 1; // one session
        let sd = 38;
        data[sd + 8..sd + 16].copy_from_slice(b"KODAK\0\0\0");
        data[sd + 16..sd + 24].copy_from_slice(b"WRITERPR");
        let id = 38 + 68;
        data[id..id + 4].copy_from_slice(&1055u32.to_be_bytes());
        data[id + 4] = (2 << 2) | 1; // 16Base, 90 CCW
        data[id + 6..id + 10].copy_from_slice(&2121u32.to_be_bytes());
        data[id + 10] = 0; // Base, no rotation
        data
    }

    #[test]
    fn parse_info_pcd_reads_synthetic_fields() {
        let info = parse_info_pcd(&synthetic_info_pcd());
        assert_eq!(info.disc_id, "PHOTO_CD");
        assert_eq!(info.spec_version, "1.00");
        assert_eq!(info.serial, "TESTSERIAL12");
        assert_eq!(info.creation_unix_ts, 0x1234_5678);
        assert_eq!(info.n_images, 2);
        assert_eq!(info.n_sessions, 1);
        assert_eq!(info.res_highest, 3);
        assert_eq!(info.res_lowest, 1);
        assert_eq!(info.writer_vendor, "KODAK");
        assert_eq!(info.image_descriptors.len(), 2);
        assert_eq!(info.image_descriptors[0].lba, 1055);
        assert_eq!(info.image_descriptors[0].resolution, 2);
        assert_eq!(info.image_descriptors[0].rotation, 1);
        assert_eq!(info.image_descriptors[1].lba, 2121);
        assert_eq!(info.image_descriptors[1].rotation, 0);
    }

    #[test]
    fn parse_info_pcd_tolerates_truncated_data() {
        let info = parse_info_pcd(&[0u8; 10]);
        assert_eq!(info.n_images, 0);
        assert!(info.image_descriptors.is_empty());
    }
}
