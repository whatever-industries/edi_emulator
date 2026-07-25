// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only CD-i disc inventory and real-time-file provenance.
//!
//! This module intentionally records metadata, bounds, and hashes rather than
//! extracting media.  It is used by compatibility diagnostics; it never
//! changes how the emulated player reads or presents a disc.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::cuesheet::{parse_cue, TrackMode};
use crate::sector::{submode, Mode2Subheader, SectorHeader};
use crate::{DiscImage, RAW_SECTOR_SIZE};

const NORMAL_LBA_BASE: u32 = 150;
const DESCRIPTOR_LBA: u32 = 16;
const MAX_DIRECTORY_DEPTH: usize = 64;
const MAX_DIRECTORY_BYTES: u32 = 16 * 1024 * 1024;
const MAX_MODULE_SCAN_BYTES: u32 = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscFingerprint {
    /// Stable identifier for the complete ordered CUE file set.
    pub sha1: String,
    pub files: Vec<FingerprintFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FingerprintFile {
    /// CUE-relative path, never an absolute host path.
    pub path: String,
    pub bytes: u64,
    pub sha1: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscInventory {
    pub schema_version: u32,
    pub fingerprint: DiscFingerprint,
    pub leadout_frame: u32,
    pub lba_base: u32,
    pub tracks: Vec<TrackInventory>,
    pub cdi_volume: Option<CdiVolumeInventory>,
    pub iso_volume: Option<IsoVolumeInventory>,
    pub os9_modules: Vec<Os9ModuleInventory>,
    pub realtime_files: Vec<RealtimeFileInventory>,
    pub requires_dvc: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackInventory {
    pub number: u8,
    pub mode: String,
    pub region_start: u32,
    pub index01: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CdiVolumeInventory {
    pub descriptor_abs_frame: u32,
    pub album: String,
    pub volume_id: String,
    pub path_table_lba: u32,
    pub root_lba: u32,
    pub files: Vec<CdiFileEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsoVolumeInventory {
    pub descriptor_abs_frame: u32,
    pub volume_id: String,
    pub files: Vec<CdiFileEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CdiFileEntry {
    pub path: String,
    pub lba: u32,
    pub bytes: u32,
    pub directory: bool,
    pub attributes: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeFileInventory {
    pub path: String,
    pub lba: u32,
    pub bytes: u32,
    pub sectors_scanned: u32,
    pub sector_classes: Vec<SectorClassInventory>,
    pub mpeg_sequences: Vec<MpegSequenceInventory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Os9ModuleInventory {
    pub file_path: String,
    pub offset: u32,
    pub bytes: u32,
    pub name: String,
    pub module_type: String,
    pub revision: u8,
    pub edition: u16,
    pub crc_ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SectorClassInventory {
    pub file: u8,
    pub channel: u8,
    pub submode: u8,
    pub coding: u8,
    pub form: u8,
    pub kind: String,
    pub coding_name: String,
    pub realtime: bool,
    pub eor: bool,
    pub eof: bool,
    pub trigger: bool,
    pub sectors: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MpegSequenceInventory {
    pub width: u16,
    pub height: u16,
    pub aspect_code: u8,
    pub frame_rate_code: u8,
}

/// Inspect a CUE and all referenced files without retaining any media payload.
pub fn inspect_cue(cue_path: &Path) -> Result<DiscInventory, String> {
    let fingerprint = fingerprint_cue(cue_path)?;
    let disc = DiscImage::load(cue_path)?;
    disc.inspect_with_fingerprint(fingerprint)
}

/// Compute the exact ordered-set identity used by diagnostics and profiles.
pub fn fingerprint_cue(cue_path: &Path) -> Result<DiscFingerprint, String> {
    let text = std::fs::read_to_string(cue_path)
        .map_err(|error| format!("read {}: {error}", cue_path.display()))?;
    let base = cue_path.parent().unwrap_or_else(|| Path::new("."));
    let parsed = parse_cue(&text, base)?;
    let mut seen = BTreeSet::new();
    let mut files = Vec::new();
    let mut combined = Sha1::new();
    for cue_file in parsed {
        let canonical = cue_file
            .path
            .canonicalize()
            .unwrap_or_else(|_| cue_file.path.clone());
        if !seen.insert(canonical.clone()) {
            continue;
        }
        let mut reader = std::fs::File::open(&canonical)
            .map_err(|error| format!("open {}: {error}", canonical.display()))?;
        let expected_bytes = reader
            .metadata()
            .map_err(|error| format!("stat {}: {error}", canonical.display()))?
            .len();
        combined.update(expected_bytes.to_be_bytes());
        let mut file_hash = Sha1::new();
        let mut bytes = 0u64;
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let count = reader
                .read(&mut chunk)
                .map_err(|error| format!("read {}: {error}", canonical.display()))?;
            if count == 0 {
                break;
            }
            bytes += count as u64;
            file_hash.update(&chunk[..count]);
            combined.update(&chunk[..count]);
        }
        let relative = cue_file
            .path
            .strip_prefix(base)
            .ok()
            .filter(|path| !path.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .or_else(|| cue_file.path.file_name().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("disc-file"))
            .to_string_lossy()
            .replace('\\', "/");
        files.push(FingerprintFile {
            path: relative,
            bytes,
            sha1: format!("{:x}", file_hash.finalize()),
        });
    }
    Ok(DiscFingerprint {
        sha1: format!("{:x}", combined.finalize()),
        files,
    })
}

impl DiscImage {
    /// Build a metadata-only inventory for this already-loaded image.
    ///
    /// When the original CUE path is available, [`inspect_cue`] is preferred
    /// because it records per-file identities. This method still supplies a
    /// stable whole-image identity for callers that only retain `DiscImage`.
    pub fn inspect(&self) -> Result<DiscInventory, String> {
        let mut hash = Sha1::new();
        for frame in 0..self.leadout() {
            if let Some(sector) = self.read_sector_raw(frame) {
                hash.update(sector);
            }
        }
        self.inspect_with_fingerprint(DiscFingerprint {
            sha1: format!("{:x}", hash.finalize()),
            files: Vec::new(),
        })
    }

    fn inspect_with_fingerprint(
        &self,
        fingerprint: DiscFingerprint,
    ) -> Result<DiscInventory, String> {
        let lba_base = self.tracks().first().map_or(NORMAL_LBA_BASE, |track| {
            if track.region_start == 0 {
                0
            } else {
                NORMAL_LBA_BASE
            }
        });
        let tracks = self
            .tracks()
            .iter()
            .map(|track| TrackInventory {
                number: track.number,
                mode: track_mode_name(track.mode).to_owned(),
                region_start: track.region_start,
                index01: track.start,
                end: track.end,
            })
            .collect();
        let mut warnings = Vec::new();
        let cdi_volume = self.inspect_cdi_volume(lba_base, &mut warnings)?;
        let iso_volume = self.inspect_iso_volume(lba_base, &mut warnings)?;
        let filesystem_files = cdi_volume
            .as_ref()
            .map(|volume| volume.files.as_slice())
            .or_else(|| iso_volume.as_ref().map(|volume| volume.files.as_slice()))
            .unwrap_or_default();
        let iso_filesystem = cdi_volume.is_none() && iso_volume.is_some();
        let os9_modules = self.inspect_os9_modules(lba_base, filesystem_files, &mut warnings);
        let realtime_files = filesystem_files
            .iter()
            .filter(|file| !file.directory)
            .filter_map(|file| {
                self.inspect_realtime_file(lba_base, file, iso_filesystem, &mut warnings)
            })
            .collect::<Vec<_>>();
        let requires_dvc = realtime_files.iter().any(|file| {
            !file.mpeg_sequences.is_empty()
                || file
                    .sector_classes
                    .iter()
                    .any(|class| class.submode & submode::VIDEO != 0 && class.coding & 0x0F == 0x0F)
        });
        Ok(DiscInventory {
            schema_version: 1,
            fingerprint,
            leadout_frame: self.leadout(),
            lba_base,
            tracks,
            cdi_volume,
            iso_volume,
            os9_modules,
            realtime_files,
            requires_dvc,
            warnings,
        })
    }

    fn inspect_cdi_volume(
        &self,
        lba_base: u32,
        warnings: &mut Vec<String>,
    ) -> Result<Option<CdiVolumeInventory>, String> {
        let abs = lba_base + DESCRIPTOR_LBA;
        let Some(sector) = self.read_sector_data(abs) else {
            return Ok(None);
        };
        let Some(user) = form1_user_data(&sector) else {
            return Ok(None);
        };
        let Some(record_offset) = find_cdi_descriptor(user) else {
            return Ok(None);
        };
        let record = &user[record_offset..];
        let volume_id = text_field(record.get(40..72).unwrap_or_default());
        let album = volume_id.clone();
        let path_table_offset = if record_offset == 8 { 156 } else { 148 };
        let path_table_lba = be_u32(record, path_table_offset).unwrap_or(0);
        let root_lba = self
            .read_form1_lba(lba_base, path_table_lba)
            .and_then(|table| {
                let offset = if record_offset == 8 { 0 } else { 2 };
                be_u32(&table, offset)
            })
            .unwrap_or(0);
        let mut files = Vec::new();
        if root_lba == 0 {
            warnings.push("CD-i label has no root-directory LBA".to_owned());
        } else {
            self.walk_cdi_directories(lba_base, root_lba, &mut files, warnings)?;
        }
        Ok(Some(CdiVolumeInventory {
            descriptor_abs_frame: abs,
            album,
            volume_id,
            path_table_lba,
            root_lba,
            files,
        }))
    }

    fn inspect_iso_volume(
        &self,
        lba_base: u32,
        warnings: &mut Vec<String>,
    ) -> Result<Option<IsoVolumeInventory>, String> {
        let abs = lba_base + DESCRIPTOR_LBA;
        let Some(sector) = self.read_sector_data(abs) else {
            return Ok(None);
        };
        let Some(user) = form1_user_data(&sector) else {
            return Ok(None);
        };
        if user.get(0..7) != Some(&[1, b'C', b'D', b'0', b'0', b'1', 1]) {
            return Ok(None);
        }
        let volume_id = text_field(user.get(40..72).unwrap_or_default());
        let root = user
            .get(156..)
            .and_then(parse_iso_directory_record)
            .ok_or("invalid ISO 9660 root directory record")?;
        let mut files = Vec::new();
        self.walk_iso_directories(lba_base, root.lba, root.bytes, &mut files, warnings)?;
        Ok(Some(IsoVolumeInventory {
            descriptor_abs_frame: abs,
            volume_id,
            files,
        }))
    }

    fn walk_iso_directories(
        &self,
        lba_base: u32,
        root_lba: u32,
        root_bytes: u32,
        files: &mut Vec<CdiFileEntry>,
        warnings: &mut Vec<String>,
    ) -> Result<(), String> {
        let mut queue = VecDeque::from([(String::new(), root_lba, root_bytes, 0usize)]);
        let mut visited = BTreeSet::new();
        while let Some((prefix, lba, bytes, depth)) = queue.pop_front() {
            if depth > MAX_DIRECTORY_DEPTH || !visited.insert(lba) {
                continue;
            }
            let bytes = bytes.min(MAX_DIRECTORY_BYTES);
            let mut data = Vec::with_capacity(bytes as usize);
            for offset in 0..bytes.div_ceil(2048) {
                let Some(block) = self.read_form1_lba(lba_base, lba + offset) else {
                    warnings.push(format!("short ISO directory read at LBA {}", lba + offset));
                    break;
                };
                data.extend(block);
            }
            data.truncate(bytes as usize);
            let mut offset = 0usize;
            while offset < data.len() {
                let length = data[offset] as usize;
                if length == 0 {
                    offset = (offset / 2048 + 1) * 2048;
                    continue;
                }
                let Some(entry) = parse_iso_directory_record(&data[offset..]) else {
                    break;
                };
                offset += length;
                if entry.path == "." || entry.path == ".." {
                    continue;
                }
                let path = if prefix.is_empty() {
                    entry.path.clone()
                } else {
                    format!("{prefix}/{}", entry.path)
                };
                if entry.directory {
                    queue.push_back((path.clone(), entry.lba, entry.bytes, depth + 1));
                }
                files.push(CdiFileEntry { path, ..entry });
            }
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(())
    }

    fn walk_cdi_directories(
        &self,
        lba_base: u32,
        root_lba: u32,
        files: &mut Vec<CdiFileEntry>,
        warnings: &mut Vec<String>,
    ) -> Result<(), String> {
        let mut queue = VecDeque::from([(String::new(), root_lba, 2048u32, 0usize)]);
        let mut visited = BTreeSet::new();
        while let Some((prefix, lba, hinted_size, depth)) = queue.pop_front() {
            if depth > MAX_DIRECTORY_DEPTH || !visited.insert(lba) {
                continue;
            }
            let first = self
                .read_form1_lba(lba_base, lba)
                .ok_or_else(|| format!("unable to read CD-i directory LBA {lba}"))?;
            let size = directory_self_size(&first)
                .unwrap_or(hinted_size)
                .min(MAX_DIRECTORY_BYTES);
            let mut data = Vec::with_capacity(size as usize);
            for sector_index in 0..size.div_ceil(2048) {
                let Some(chunk) = self.read_form1_lba(lba_base, lba + sector_index) else {
                    warnings.push(format!(
                        "short CD-i directory read at LBA {}",
                        lba + sector_index
                    ));
                    break;
                };
                data.extend_from_slice(&chunk);
            }
            data.truncate(size as usize);
            for entry in parse_directory_records(&data) {
                if entry.path == "." || entry.path == ".." {
                    continue;
                }
                let path = if prefix.is_empty() {
                    entry.path.clone()
                } else {
                    format!("{prefix}/{}", entry.path)
                };
                let stored = CdiFileEntry {
                    path: path.clone(),
                    lba: entry.lba,
                    bytes: entry.bytes,
                    directory: entry.directory,
                    attributes: entry.attributes,
                };
                if entry.directory {
                    queue.push_back((path, entry.lba, entry.bytes, depth + 1));
                }
                files.push(stored);
            }
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(())
    }

    fn read_form1_lba(&self, lba_base: u32, lba: u32) -> Option<Vec<u8>> {
        let sector = self.read_sector_data(lba_base + lba)?;
        form1_user_data(&sector).map(Vec::from)
    }

    fn inspect_realtime_file(
        &self,
        lba_base: u32,
        file: &CdiFileEntry,
        iso_filesystem: bool,
        warnings: &mut Vec<String>,
    ) -> Option<RealtimeFileInventory> {
        let bytes_per_sector = if iso_filesystem
            && self
                .read_sector_data(lba_base + file.lba)
                .and_then(|sector| Mode2Subheader::parse(&sector))
                .is_some_and(|subheader| subheader.is_form2())
        {
            2324
        } else {
            2048
        };
        let sectors = file.bytes.div_ceil(bytes_per_sector);
        let mut classes = BTreeMap::<(u8, u8, u8, u8, u8), u32>::new();
        let mut sequences = BTreeSet::new();
        let mut realtime = false;
        let mut scanned = 0;
        for offset in 0..sectors {
            let Some(sector) = self.read_sector_data(lba_base + file.lba + offset) else {
                warnings.push(format!(
                    "{}: stopped at unreadable sector {}",
                    file.path, offset
                ));
                break;
            };
            scanned += 1;
            if SectorHeader::parse(&sector).is_none() || sector[15] != 2 {
                continue;
            }
            let Some(sub) = Mode2Subheader::parse(&sector) else {
                continue;
            };
            realtime |= sub.is_realtime() || sub.is_audio() || sub.is_video();
            let form = if sub.is_form2() { 2 } else { 1 };
            *classes
                .entry((sub.file, sub.channel, sub.submode, sub.coding, form))
                .or_default() += 1;
            if sub.is_video() && sub.coding & 0x0F == 0x0F {
                let range = sub.user_data_range();
                scan_mpeg_sequences(&sector[range], &mut sequences);
            }
        }
        if !realtime && sequences.is_empty() {
            return None;
        }
        Some(RealtimeFileInventory {
            path: file.path.clone(),
            lba: file.lba,
            bytes: file.bytes,
            sectors_scanned: scanned,
            sector_classes: classes
                .into_iter()
                .map(
                    |((file, channel, submode, coding, form), sectors)| SectorClassInventory {
                        file,
                        channel,
                        submode,
                        coding,
                        form,
                        kind: sector_kind(submode).to_owned(),
                        coding_name: coding_name(submode, coding).to_owned(),
                        realtime: submode & submode::RT != 0,
                        eor: submode & submode::EOR != 0,
                        eof: submode & submode::EOF != 0,
                        trigger: submode & submode::TRIGGER != 0,
                        sectors,
                    },
                )
                .collect(),
            mpeg_sequences: sequences.into_iter().collect(),
        })
    }

    fn inspect_os9_modules(
        &self,
        lba_base: u32,
        files: &[CdiFileEntry],
        warnings: &mut Vec<String>,
    ) -> Vec<Os9ModuleInventory> {
        let mut modules = Vec::new();
        for file in files
            .iter()
            .filter(|file| !file.directory && file.bytes <= MAX_MODULE_SCAN_BYTES)
        {
            let Some(bytes) = self.read_regular_file(lba_base, file) else {
                continue;
            };
            for module in cdi_os9::scan_modules(&bytes) {
                modules.push(Os9ModuleInventory {
                    file_path: file.path.clone(),
                    offset: module.offset,
                    bytes: module.size,
                    name: module.name,
                    module_type: module.mod_type.name().to_owned(),
                    revision: module.revision,
                    edition: module.edition,
                    crc_ok: module.crc_ok,
                });
            }
        }
        if files
            .iter()
            .any(|file| !file.directory && file.bytes > MAX_MODULE_SCAN_BYTES)
        {
            warnings.push(format!(
                "OS-9 scanning skips files larger than {MAX_MODULE_SCAN_BYTES} bytes"
            ));
        }
        modules
    }

    fn read_regular_file(&self, lba_base: u32, file: &CdiFileEntry) -> Option<Vec<u8>> {
        let mut bytes = Vec::with_capacity(file.bytes as usize);
        for offset in 0..file.bytes.div_ceil(2048) {
            bytes.extend(self.read_form1_lba(lba_base, file.lba + offset)?);
        }
        bytes.truncate(file.bytes as usize);
        Some(bytes)
    }
}

fn track_mode_name(mode: TrackMode) -> &'static str {
    match mode {
        TrackMode::Audio => "audio",
        TrackMode::Mode1_2352 => "mode1/2352",
        TrackMode::Mode2_2352 => "mode2/2352",
        TrackMode::Cdi2352 => "cdi/2352",
    }
}

fn form1_user_data(sector: &[u8; RAW_SECTOR_SIZE]) -> Option<&[u8]> {
    match sector[15] {
        1 => Some(&sector[16..16 + 2048]),
        2 => {
            let sub = Mode2Subheader::parse(sector)?;
            if sub.is_form2() {
                None
            } else {
                Some(&sector[24..24 + 2048])
            }
        }
        _ => None,
    }
}

fn find_cdi_descriptor(user: &[u8]) -> Option<usize> {
    [8usize, 0].into_iter().find(|offset| {
        user.get(*offset) == Some(&1)
            && matches!(
                user.get(*offset + 1..*offset + 6),
                Some(id) if id == b"CD-I " || id == b"CD_I "
            )
    })
}

fn text_field(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_matches(|character: char| character == '\0' || character == ' ')
        .to_owned()
}

fn be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn directory_self_size(first: &[u8]) -> Option<u32> {
    parse_directory_records(first)
        .into_iter()
        .find(|entry| entry.path == ".")
        .map(|entry| entry.bytes)
}

fn parse_directory_records(data: &[u8]) -> Vec<CdiFileEntry> {
    let mut entries = Vec::new();
    let mut offset = 0usize;
    while offset + 34 <= data.len() {
        let length = data[offset] as usize;
        if length == 0 {
            offset = (offset / 2048 + 1) * 2048;
            continue;
        }
        if length < 34 || offset + length > data.len() {
            break;
        }
        let record = &data[offset..offset + length];
        let lba = be_u32(record, 6).unwrap_or(0);
        let bytes = be_u32(record, 14).unwrap_or(0);
        let name_len = record[32] as usize;
        if 33 + name_len > record.len() {
            break;
        }
        let name_bytes = &record[33..33 + name_len];
        let path = match name_bytes {
            [0] => ".".to_owned(),
            [1] => "..".to_owned(),
            _ => String::from_utf8_lossy(name_bytes)
                .trim_end_matches(";1")
                .to_owned(),
        };
        let attributes_offset = 33 + name_len + usize::from(name_len % 2 == 0) + 4;
        let attributes = record
            .get(attributes_offset..attributes_offset + 2)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u16::from_be_bytes)
            .unwrap_or(0);
        entries.push(CdiFileEntry {
            path,
            lba,
            bytes,
            directory: attributes & 0x8000 != 0,
            attributes,
        });
        offset += length;
    }
    entries
}

fn parse_iso_directory_record(data: &[u8]) -> Option<CdiFileEntry> {
    let length = usize::from(*data.first()?);
    if length < 34 || data.len() < length {
        return None;
    }
    let record = &data[..length];
    let lba = u32::from_le_bytes(record.get(2..6)?.try_into().ok()?);
    let bytes = u32::from_le_bytes(record.get(10..14)?.try_into().ok()?);
    let name_length = usize::from(record[32]);
    let name = record.get(33..33 + name_length)?;
    let path = match name {
        [0] => ".".to_owned(),
        [1] => "..".to_owned(),
        _ => String::from_utf8_lossy(name)
            .trim_end_matches(";1")
            .to_owned(),
    };
    Some(CdiFileEntry {
        path,
        lba,
        bytes,
        directory: record[25] & 0x02 != 0,
        attributes: u16::from(record[25]),
    })
}

fn scan_mpeg_sequences(bytes: &[u8], output: &mut BTreeSet<MpegSequenceInventory>) {
    for window in bytes.windows(8) {
        if window[..4] != [0, 0, 1, 0xB3] {
            continue;
        }
        output.insert(MpegSequenceInventory {
            width: u16::from(window[4]) << 4 | u16::from(window[5] >> 4),
            height: u16::from(window[5] & 0x0F) << 8 | u16::from(window[6]),
            aspect_code: window[7] >> 4,
            frame_rate_code: window[7] & 0x0F,
        });
    }
}

fn sector_kind(value: u8) -> &'static str {
    if value & submode::VIDEO != 0 {
        "video"
    } else if value & submode::AUDIO != 0 {
        "audio"
    } else if value & submode::DATA != 0 {
        "data"
    } else {
        "control"
    }
}

fn coding_name(mode: u8, coding: u8) -> &'static str {
    if mode & submode::VIDEO == 0 {
        return if mode & submode::AUDIO != 0 {
            "xa-audio"
        } else {
            "not-video"
        };
    }
    match coding & 0x0F {
        0 => "clut4",
        1 => "clut7",
        2 => "clut8",
        3 => "rl3",
        4 => "rl7",
        5 => "dyuv",
        6 | 7 => "rgb555",
        8 => "qhy",
        9..=14 => "reserved",
        15 => "mpeg",
        _ => unreachable!("coding was masked to four bits"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory_record(name: &[u8], lba: u32, bytes: u32, attributes: u16) -> Vec<u8> {
        let attribute_offset = 33 + name.len() + usize::from(name.len() % 2 == 0) + 4;
        let mut record = vec![0u8; attribute_offset + 2];
        record[0] = record.len() as u8;
        record[6..10].copy_from_slice(&lba.to_be_bytes());
        record[14..18].copy_from_slice(&bytes.to_be_bytes());
        record[32] = name.len() as u8;
        record[33..33 + name.len()].copy_from_slice(name);
        record[attribute_offset..attribute_offset + 2].copy_from_slice(&attributes.to_be_bytes());
        record
    }

    #[test]
    fn parses_green_book_directory_records() {
        let mut bytes = directory_record(b"INTRO.RTF;1", 1234, 8192, 0);
        bytes.extend(directory_record(b"IMAGES", 2000, 2048, 0x8000));
        let entries = parse_directory_records(&bytes);
        assert_eq!(entries[0].path, "INTRO.RTF");
        assert_eq!((entries[0].lba, entries[0].bytes), (1234, 8192));
        assert!(entries[1].directory);
    }

    #[test]
    fn identifies_mpeg_sequence_headers_without_retaining_payload() {
        let mut sequences = BTreeSet::new();
        scan_mpeg_sequences(&[0, 0, 1, 0xB3, 0x16, 0x01, 0x20, 0x34], &mut sequences);
        assert_eq!(
            sequences.into_iter().next(),
            Some(MpegSequenceInventory {
                width: 352,
                height: 288,
                aspect_code: 3,
                frame_rate_code: 4,
            })
        );
    }

    #[test]
    fn green_book_video_coding_and_event_flags_are_explicit() {
        assert_eq!(coding_name(submode::VIDEO, 4), "rl7");
        assert_eq!(coding_name(submode::VIDEO, 5), "dyuv");
        assert_eq!(coding_name(submode::VIDEO, 8), "qhy");
        assert_eq!(coding_name(submode::VIDEO, 15), "mpeg");
        assert_eq!(
            sector_kind(submode::VIDEO | submode::FORM2 | submode::RT),
            "video"
        );
        let flags = submode::EOR | submode::EOF | submode::TRIGGER;
        assert_ne!(flags & submode::EOR, 0);
        assert_ne!(flags & submode::EOF, 0);
        assert_ne!(flags & submode::TRIGGER, 0);
    }

    #[test]
    fn parses_iso_directory_record_for_vcd_filesystem_inventory() {
        let mut record = vec![0u8; 48];
        record[0] = 48;
        record[2..6].copy_from_slice(&1234u32.to_le_bytes());
        record[10..14].copy_from_slice(&4096u32.to_le_bytes());
        record[32] = 11;
        record[33..44].copy_from_slice(b"AVSEQ01.DAT");
        let entry = parse_iso_directory_record(&record).unwrap();
        assert_eq!(entry.path, "AVSEQ01.DAT");
        assert_eq!((entry.lba, entry.bytes), (1234, 4096));
        assert!(!entry.directory);
    }

    #[test]
    fn cue_fingerprint_never_records_absolute_host_paths() {
        let directory =
            std::env::temp_dir().join(format!("cdi-inventory-privacy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("track.bin"), [0u8; 16]).unwrap();
        std::fs::write(
            directory.join("disc.cue"),
            "FILE \"track.bin\" BINARY\n TRACK 01 CDI/2352\n INDEX 01 00:00:00\n",
        )
        .unwrap();
        let fingerprint = fingerprint_cue(&directory.join("disc.cue")).unwrap();
        assert_eq!(fingerprint.files[0].path, "track.bin");
        assert!(!fingerprint.files[0].path.starts_with('/'));
        let _ = std::fs::remove_dir_all(directory);
    }
}
