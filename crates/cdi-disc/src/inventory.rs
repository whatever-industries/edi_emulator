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
    /// Strongest content classification supported by on-disc evidence.
    #[serde(default)]
    pub content_kind: DiscContentKind,
    /// Whether the primary descriptor carries the CD-ROM XA Bridge markers.
    #[serde(default)]
    pub cd_rom_xa_bridge: bool,
    /// Whether the ISO filesystem contains a root `CDI` application tree.
    #[serde(default)]
    pub has_cdi_application: bool,
    pub cdi_volume: Option<CdiVolumeInventory>,
    pub iso_volume: Option<IsoVolumeInventory>,
    /// White Book entry points and PSD list topology, without media payload.
    #[serde(default)]
    pub vcd_navigation: Option<VcdNavigationInventory>,
    pub os9_modules: Vec<Os9ModuleInventory>,
    pub realtime_files: Vec<RealtimeFileInventory>,
    pub requires_dvc: bool,
    pub warnings: Vec<String>,
}

/// Payload-free media classification derived from volume and filesystem
/// metadata. More specific Bridge profiles take precedence over the generic
/// CD-ROM XA Bridge classification.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiscContentKind {
    NativeCdi,
    PhotoCd,
    VideoCd,
    CdRomXaBridge,
    #[default]
    Unknown,
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
    #[serde(default)]
    pub system_id: String,
    pub volume_id: String,
    #[serde(default)]
    pub application_id: String,
    pub files: Vec<CdiFileEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcdNavigationInventory {
    pub specification_version: u16,
    pub album_id: String,
    pub volume_count: u16,
    pub volume_number: u16,
    pub psd_bytes: u32,
    pub offset_multiplier: u8,
    pub maximum_list_id: u16,
    pub entries: Vec<VcdEntryInventory>,
    pub lists: Vec<VcdListInventory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcdEntryInventory {
    pub number: u16,
    pub track: u8,
    pub minute: u8,
    pub second: u8,
    pub frame: u8,
    pub absolute_frame: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcdListInventory {
    pub list_id: u16,
    pub rejected: bool,
    pub offset_units: u16,
    pub offset_bytes: u32,
    pub kind: String,
    pub previous_offset: Option<u16>,
    pub next_offset: Option<u16>,
    pub return_offset: Option<u16>,
    pub default_offset: Option<u16>,
    pub timeout_offset: Option<u16>,
    pub play_items: Vec<u16>,
    pub selection_base: Option<u8>,
    pub selection_offsets: Vec<u16>,
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
        let cd_rom_xa_bridge = self.is_cd_rom_xa_bridge();
        let (content_kind, has_cdi_application) =
            classify_disc_content(cdi_volume.is_some(), cd_rom_xa_bridge, filesystem_files);
        let vcd_navigation = (content_kind == DiscContentKind::VideoCd)
            .then(|| self.inspect_vcd_navigation(lba_base, filesystem_files, &mut warnings))
            .flatten();
        Ok(DiscInventory {
            schema_version: 3,
            fingerprint,
            leadout_frame: self.leadout(),
            lba_base,
            tracks,
            content_kind,
            cd_rom_xa_bridge,
            has_cdi_application,
            cdi_volume,
            iso_volume,
            vcd_navigation,
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
        let system_id = text_field(user.get(8..40).unwrap_or_default());
        let volume_id = text_field(user.get(40..72).unwrap_or_default());
        // ISO 9660 PVD application identifier. Philips Bridge applications
        // use this metadata as an entry-point hint; preserve it verbatim for
        // diagnostics instead of interpreting it as a host launch command.
        let application_id = text_field(user.get(574..702).unwrap_or_default());
        let root = user
            .get(156..)
            .and_then(parse_iso_directory_record)
            .ok_or("invalid ISO 9660 root directory record")?;
        let mut files = Vec::new();
        self.walk_iso_directories(lba_base, root.lba, root.bytes, &mut files, warnings)?;
        Ok(Some(IsoVolumeInventory {
            descriptor_abs_frame: abs,
            system_id,
            volume_id,
            application_id,
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

    fn inspect_vcd_navigation(
        &self,
        lba_base: u32,
        files: &[CdiFileEntry],
        warnings: &mut Vec<String>,
    ) -> Option<VcdNavigationInventory> {
        let find = |wanted: &str| {
            files.iter().find(|file| {
                !file.directory && file.path.replace('\\', "/").eq_ignore_ascii_case(wanted)
            })
        };
        let info_file = find("VCD/INFO.VCD")?;
        let entries_file = find("VCD/ENTRIES.VCD")?;
        let info_bytes = self.read_regular_file(lba_base, info_file)?;
        let Some(info) = parse_vcd_info(&info_bytes) else {
            warnings.push("VCD/INFO.VCD has an invalid White Book header".to_owned());
            return None;
        };
        let entries_bytes = self.read_regular_file(lba_base, entries_file)?;
        let entries = parse_vcd_entries(&entries_bytes, warnings);
        let mut navigation = VcdNavigationInventory {
            specification_version: info.specification_version,
            album_id: info.album_id,
            volume_count: info.volume_count,
            volume_number: info.volume_number,
            psd_bytes: info.psd_bytes,
            offset_multiplier: info.offset_multiplier,
            maximum_list_id: info.maximum_list_id,
            entries,
            lists: Vec::new(),
        };

        if info.psd_bytes == 0 {
            return Some(navigation);
        }
        let (Some(lot_file), Some(psd_file)) = (find("VCD/LOT.VCD"), find("VCD/PSD.VCD")) else {
            warnings.push("VCD declares a PSD but LOT.VCD or PSD.VCD is missing".to_owned());
            return Some(navigation);
        };
        if lot_file.bytes > 64 * 1024 || psd_file.bytes > 256 * 2048 {
            warnings.push("VCD navigation file exceeds its White Book maximum size".to_owned());
            return Some(navigation);
        }
        let Some(lot) = self.read_regular_file(lba_base, lot_file) else {
            return Some(navigation);
        };
        let Some(mut psd) = self.read_regular_file(lba_base, psd_file) else {
            return Some(navigation);
        };
        psd.truncate(info.psd_bytes as usize);
        navigation.lists = parse_vcd_lists(
            &lot,
            &psd,
            info.offset_multiplier,
            info.maximum_list_id,
            warnings,
        );
        Some(navigation)
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

struct VcdInfoHeader {
    specification_version: u16,
    album_id: String,
    volume_count: u16,
    volume_number: u16,
    psd_bytes: u32,
    offset_multiplier: u8,
    maximum_list_id: u16,
}

// Video CD 2.0 (July 1994), III.2.5.1-III.2.5.4 and VI.1-VI.6.
// Keep this metadata-only: native CD-i software remains responsible for
// interpreting and presenting the authored navigation sequence.
fn parse_vcd_info(bytes: &[u8]) -> Option<VcdInfoHeader> {
    if bytes.get(..8)? != b"VIDEO_CD" || bytes.len() < 56 {
        return None;
    }
    Some(VcdInfoHeader {
        specification_version: be_u16(bytes, 8)?,
        album_id: text_field(bytes.get(10..26)?),
        volume_count: be_u16(bytes, 26)?,
        volume_number: be_u16(bytes, 28)?,
        psd_bytes: be_u32(bytes, 44)?,
        offset_multiplier: *bytes.get(51)?,
        maximum_list_id: be_u16(bytes, 52)?,
    })
}

fn parse_vcd_entries(bytes: &[u8], warnings: &mut Vec<String>) -> Vec<VcdEntryInventory> {
    if bytes.get(..8) != Some(b"ENTRYVCD".as_slice()) || bytes.len() < 12 {
        warnings.push("VCD/ENTRIES.VCD has an invalid White Book header".to_owned());
        return Vec::new();
    }
    let used = usize::from(be_u16(bytes, 10).unwrap_or(0)).min(500);
    if 12 + used * 4 > bytes.len() {
        warnings.push("VCD/ENTRIES.VCD ends before its declared entries".to_owned());
    }
    bytes[12..]
        .chunks_exact(4)
        .take(used)
        .enumerate()
        .filter_map(|(index, entry)| {
            let decoded = [entry[0], entry[1], entry[2], entry[3]].map(decode_bcd);
            let [Some(track), Some(minute), Some(second), Some(frame)] = decoded else {
                warnings.push(format!("VCD entry {} contains invalid BCD", index + 1));
                return None;
            };
            Some(VcdEntryInventory {
                number: u16::try_from(index + 1).ok()?,
                track,
                minute,
                second,
                frame,
                absolute_frame: u32::from(minute) * 60 * 75
                    + u32::from(second) * 75
                    + u32::from(frame),
            })
        })
        .collect()
}

fn parse_vcd_lists(
    lot: &[u8],
    psd: &[u8],
    offset_multiplier: u8,
    maximum_list_id: u16,
    warnings: &mut Vec<String>,
) -> Vec<VcdListInventory> {
    if offset_multiplier == 0 {
        warnings.push("VCD PSD has a zero offset multiplier".to_owned());
        return Vec::new();
    }
    let mut lists = Vec::new();
    for list_id in 1..=maximum_list_id.min(0x7fff) {
        let lot_offset = usize::from(list_id) * 2;
        let Some(offset_units) = be_u16(lot, lot_offset) else {
            warnings.push("VCD/LOT.VCD ends before Maximum List ID".to_owned());
            break;
        };
        if offset_units == 0xffff {
            continue;
        }
        let offset_bytes = u32::from(offset_units) * u32::from(offset_multiplier);
        let Some(data) = psd.get(offset_bytes as usize..) else {
            warnings.push(format!(
                "VCD list {list_id} points beyond PSD.VCD ({offset_bytes} bytes)"
            ));
            continue;
        };
        let Some((list, encoded_id)) = parse_vcd_list(list_id, offset_units, offset_bytes, data)
        else {
            warnings.push(format!(
                "VCD list {list_id} has an invalid or truncated descriptor"
            ));
            continue;
        };
        if encoded_id.is_some_and(|encoded| encoded != list_id) {
            warnings.push(format!(
                "VCD LOT list {list_id} points to descriptor list {}",
                encoded_id.unwrap()
            ));
        }
        lists.push(list);
    }
    lists
}

fn parse_vcd_list(
    lot_list_id: u16,
    offset_units: u16,
    offset_bytes: u32,
    bytes: &[u8],
) -> Option<(VcdListInventory, Option<u16>)> {
    let header = *bytes.first()?;
    let mut list = VcdListInventory {
        list_id: lot_list_id,
        rejected: false,
        offset_units,
        offset_bytes,
        kind: "unknown".to_owned(),
        previous_offset: None,
        next_offset: None,
        return_offset: None,
        default_offset: None,
        timeout_offset: None,
        play_items: Vec::new(),
        selection_base: None,
        selection_offsets: Vec::new(),
    };
    let encoded_id = match header {
        0x10 => {
            let item_count = usize::from(*bytes.get(1)?);
            bytes.get(..14 + item_count * 2)?;
            let raw_id = be_u16(bytes, 2)?;
            list.list_id = raw_id & 0x7fff;
            list.rejected = raw_id & 0x8000 != 0;
            list.kind = "play".to_owned();
            list.previous_offset = enabled_list_offset(be_u16(bytes, 4)?);
            list.next_offset = enabled_list_offset(be_u16(bytes, 6)?);
            list.return_offset = enabled_list_offset(be_u16(bytes, 8)?);
            for offset in (14..14 + item_count * 2).step_by(2) {
                list.play_items.push(be_u16(bytes, offset)?);
            }
            Some(list.list_id)
        }
        0x18 | 0x1a => {
            let selection_count = usize::from(*bytes.get(2)?);
            let base_bytes = 20 + selection_count * 2;
            let total_bytes = if header == 0x1a {
                base_bytes + (4 + selection_count) * 4
            } else {
                base_bytes
            };
            bytes.get(..total_bytes)?;
            let raw_id = be_u16(bytes, 4)?;
            list.list_id = raw_id & 0x7fff;
            list.rejected = raw_id & 0x8000 != 0;
            list.kind = if header == 0x1a {
                "extended-selection"
            } else {
                "selection"
            }
            .to_owned();
            list.previous_offset = enabled_list_offset(be_u16(bytes, 6)?);
            list.next_offset = enabled_list_offset(be_u16(bytes, 8)?);
            list.return_offset = enabled_list_offset(be_u16(bytes, 10)?);
            list.default_offset = enabled_list_offset(be_u16(bytes, 12)?);
            list.timeout_offset = enabled_list_offset(be_u16(bytes, 14)?);
            list.play_items.push(be_u16(bytes, 18)?);
            list.selection_base = bytes.get(3).copied();
            for offset in (20..20 + selection_count * 2).step_by(2) {
                list.selection_offsets.push(be_u16(bytes, offset)?);
            }
            Some(list.list_id)
        }
        0x1f => {
            bytes.get(..8)?;
            list.kind = "end".to_owned();
            None
        }
        _ => None,
    };
    Some((list, encoded_id))
}

fn enabled_list_offset(offset: u16) -> Option<u16> {
    (offset != 0xffff).then_some(offset)
}

fn decode_bcd(byte: u8) -> Option<u8> {
    let high = byte >> 4;
    let low = byte & 0x0f;
    (high < 10 && low < 10).then_some(high * 10 + low)
}

fn classify_disc_content(
    has_cdi_volume: bool,
    cd_rom_xa_bridge: bool,
    files: &[CdiFileEntry],
) -> (DiscContentKind, bool) {
    let paths = files
        .iter()
        .map(|file| file.path.replace('\\', "/").to_ascii_uppercase())
        .collect::<BTreeSet<_>>();
    let has_cdi_application = paths
        .iter()
        .any(|path| path == "CDI" || path.starts_with("CDI/"));
    let is_photo_cd = paths.contains("PHOTO_CD/INFO.PCD")
        && paths
            .iter()
            .any(|path| path == "PHOTO_CD/IMAGES" || path.starts_with("PHOTO_CD/IMAGES/"));
    let has_mpegav_sequence = paths
        .iter()
        .any(|path| path.starts_with("MPEGAV/AVSEQ") && path.ends_with(".DAT"));
    let has_vcd_control_files = paths.contains("VCD/INFO.VCD") && paths.contains("VCD/ENTRIES.VCD");
    let has_philips_vcd_engine = paths.contains("CDI/CDI_VCD.APP");
    let is_video_cd = has_mpegav_sequence && (has_vcd_control_files || has_philips_vcd_engine);

    let kind = if cd_rom_xa_bridge && is_photo_cd {
        DiscContentKind::PhotoCd
    } else if cd_rom_xa_bridge && is_video_cd {
        DiscContentKind::VideoCd
    } else if has_cdi_volume {
        DiscContentKind::NativeCdi
    } else if cd_rom_xa_bridge {
        DiscContentKind::CdRomXaBridge
    } else {
        DiscContentKind::Unknown
    };
    (kind, has_cdi_application)
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

fn be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
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
    use crate::sector::SYNC;
    use crate::Msf;
    use std::io::Write;

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

    fn file(path: &str, directory: bool) -> CdiFileEntry {
        CdiFileEntry {
            path: path.to_owned(),
            lba: 0,
            bytes: 0,
            directory,
            attributes: 0,
        }
    }

    fn iso_record(name: &[u8], lba: u32, bytes: u32, directory: bool) -> Vec<u8> {
        let padding = usize::from(name.len() % 2 == 0);
        let mut record = vec![0u8; 33 + name.len() + padding];
        record[0] = record.len() as u8;
        record[2..6].copy_from_slice(&lba.to_le_bytes());
        record[6..10].copy_from_slice(&lba.to_be_bytes());
        record[10..14].copy_from_slice(&bytes.to_le_bytes());
        record[14..18].copy_from_slice(&bytes.to_be_bytes());
        record[25] = if directory { 0x02 } else { 0 };
        record[28..30].copy_from_slice(&1u16.to_le_bytes());
        record[30..32].copy_from_slice(&1u16.to_be_bytes());
        record[32] = name.len() as u8;
        record[33..33 + name.len()].copy_from_slice(name);
        record
    }

    fn append_records(target: &mut [u8], records: &[Vec<u8>]) {
        let mut offset = 0;
        for record in records {
            target[offset..offset + record.len()].copy_from_slice(record);
            offset += record.len();
        }
    }

    fn form1_sector(abs_frame: u32) -> [u8; RAW_SECTOR_SIZE] {
        let mut sector = [0u8; RAW_SECTOR_SIZE];
        sector[..12].copy_from_slice(&SYNC);
        sector[12..15].copy_from_slice(&Msf::from_frames(abs_frame).to_bcd());
        sector[15] = 2;
        let subheader = [0, 0, submode::DATA, 0];
        sector[16..20].copy_from_slice(&subheader);
        sector[20..24].copy_from_slice(&subheader);
        sector
    }

    #[test]
    fn classifies_bridge_profiles_from_standardized_filesystem_evidence() {
        let cases = [
            (
                false,
                false,
                vec![file("TITLE.RTF", false)],
                DiscContentKind::Unknown,
                false,
            ),
            (
                true,
                false,
                vec![file("TITLE.RTF", false)],
                DiscContentKind::NativeCdi,
                false,
            ),
            (
                false,
                true,
                vec![file("CDI", true), file("CDI/PLAYER.APP", false)],
                DiscContentKind::CdRomXaBridge,
                true,
            ),
            (
                false,
                true,
                vec![
                    file("photo_cd/info.pcd", false),
                    file("PHOTO_CD/IMAGES", true),
                    file("CDI/PHOTO_CD.APP", false),
                ],
                DiscContentKind::PhotoCd,
                true,
            ),
            (
                false,
                true,
                vec![
                    file("CDI/CDI_VCD.APP", false),
                    file("MPEGAV/AVSEQ01.DAT", false),
                ],
                DiscContentKind::VideoCd,
                true,
            ),
            (
                false,
                true,
                vec![
                    file("VCD/INFO.VCD", false),
                    file("VCD/ENTRIES.VCD", false),
                    file("MPEGAV/AVSEQ01.DAT", false),
                    file("CDI/VIDEO.EXE", false),
                ],
                DiscContentKind::VideoCd,
                true,
            ),
        ];

        for (has_cdi, bridge, files, expected_kind, expected_application) in cases {
            assert_eq!(
                classify_disc_content(has_cdi, bridge, &files),
                (expected_kind, expected_application)
            );
        }
    }

    #[test]
    fn bridge_profile_requires_both_signature_and_complete_profile_evidence() {
        let photo_files = [file("PHOTO_CD/INFO.PCD", false)];
        assert_eq!(
            classify_disc_content(false, true, &photo_files).0,
            DiscContentKind::CdRomXaBridge
        );
        assert_eq!(
            classify_disc_content(false, false, &photo_files).0,
            DiscContentKind::Unknown
        );

        let vcd_files = [file("CDI/CDI_VCD.APP", false)];
        assert_eq!(
            classify_disc_content(false, true, &vcd_files).0,
            DiscContentKind::CdRomXaBridge
        );
    }

    #[test]
    fn synthetic_bridge_pvd_and_cdi_application_classify_photo_cd() {
        const ROOT_LBA: u32 = 20;
        const PHOTO_CD_LBA: u32 = 21;
        const IMAGES_LBA: u32 = 22;

        let directory =
            std::env::temp_dir().join(format!("cdi-inventory-photo-bridge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        let mut sectors = (0..24)
            .map(|lba| form1_sector(NORMAL_LBA_BASE + lba))
            .collect::<Vec<_>>();
        let pvd = &mut sectors[DESCRIPTOR_LBA as usize][24..24 + 2048];
        pvd[..8].copy_from_slice(b"\x01CD001\x01\x00");
        pvd[8..8 + 17].copy_from_slice(b"CD-RTOS CD-BRIDGE");
        pvd[40..48].copy_from_slice(b"SYNTHETC");
        pvd[574..592].copy_from_slice(b"CDI/PHOTO_CD.APP;1");
        let root_record = iso_record(&[0], ROOT_LBA, 2048, true);
        pvd[156..156 + root_record.len()].copy_from_slice(&root_record);
        pvd[1024..1032].copy_from_slice(b"CD-XA001");

        let root = &mut sectors[ROOT_LBA as usize][24..24 + 2048];
        append_records(
            root,
            &[
                iso_record(&[0], ROOT_LBA, 2048, true),
                iso_record(&[1], ROOT_LBA, 2048, true),
                iso_record(b"PHOTO_CD", PHOTO_CD_LBA, 2048, true),
                iso_record(b"CDI", 23, 2048, true),
            ],
        );
        let photo_cd = &mut sectors[PHOTO_CD_LBA as usize][24..24 + 2048];
        append_records(
            photo_cd,
            &[
                iso_record(&[0], PHOTO_CD_LBA, 2048, true),
                iso_record(&[1], ROOT_LBA, 2048, true),
                iso_record(b"INFO.PCD;1", 0, 0, false),
                iso_record(b"IMAGES", IMAGES_LBA, 2048, true),
            ],
        );
        let images = &mut sectors[IMAGES_LBA as usize][24..24 + 2048];
        append_records(
            images,
            &[
                iso_record(&[0], IMAGES_LBA, 2048, true),
                iso_record(&[1], PHOTO_CD_LBA, 2048, true),
            ],
        );
        let cdi = &mut sectors[23][24..24 + 2048];
        append_records(
            cdi,
            &[
                iso_record(&[0], 23, 2048, true),
                iso_record(&[1], ROOT_LBA, 2048, true),
            ],
        );

        let mut bin = std::fs::File::create(directory.join("photo.bin")).unwrap();
        for sector in sectors {
            bin.write_all(&sector).unwrap();
        }
        std::fs::write(
            directory.join("photo.cue"),
            "FILE \"photo.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n",
        )
        .unwrap();

        let inventory = inspect_cue(&directory.join("photo.cue")).unwrap();
        assert_eq!(inventory.schema_version, 3);
        assert!(inventory.cd_rom_xa_bridge);
        assert!(inventory.has_cdi_application);
        assert_eq!(inventory.content_kind, DiscContentKind::PhotoCd);
        assert!(inventory.iso_volume.as_ref().is_some_and(|volume| {
            volume.application_id == "CDI/PHOTO_CD.APP;1"
                && volume
                    .files
                    .iter()
                    .any(|file| file.path == "PHOTO_CD/INFO.PCD")
        }));

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn synthetic_bridge_pvd_and_cdi_entry_point_classify_video_cd() {
        const ROOT_LBA: u32 = 20;
        const CDI_LBA: u32 = 21;
        const MPEGAV_LBA: u32 = 22;
        const VCD_LBA: u32 = 23;
        const INFO_LBA: u32 = 24;
        const ENTRIES_LBA: u32 = 25;
        const LOT_LBA: u32 = 26;
        const PSD_LBA: u32 = 58;

        let directory =
            std::env::temp_dir().join(format!("cdi-inventory-video-bridge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        let mut sectors = (0..59)
            .map(|lba| form1_sector(NORMAL_LBA_BASE + lba))
            .collect::<Vec<_>>();
        let pvd = &mut sectors[DESCRIPTOR_LBA as usize][24..24 + 2048];
        pvd[..8].copy_from_slice(b"\x01CD001\x01\x00");
        pvd[8..8 + 17].copy_from_slice(b"CD-RTOS CD-BRIDGE");
        pvd[40..48].copy_from_slice(b"SYNTHVCD");
        pvd[574..591].copy_from_slice(b"CDI/CDI_VCD.APP;1");
        let root_record = iso_record(&[0], ROOT_LBA, 2048, true);
        pvd[156..156 + root_record.len()].copy_from_slice(&root_record);
        pvd[1024..1032].copy_from_slice(b"CD-XA001");

        let root = &mut sectors[ROOT_LBA as usize][24..24 + 2048];
        append_records(
            root,
            &[
                iso_record(&[0], ROOT_LBA, 2048, true),
                iso_record(&[1], ROOT_LBA, 2048, true),
                iso_record(b"CDI", CDI_LBA, 2048, true),
                iso_record(b"MPEGAV", MPEGAV_LBA, 2048, true),
                iso_record(b"VCD", VCD_LBA, 2048, true),
            ],
        );
        let cdi = &mut sectors[CDI_LBA as usize][24..24 + 2048];
        append_records(
            cdi,
            &[
                iso_record(&[0], CDI_LBA, 2048, true),
                iso_record(&[1], ROOT_LBA, 2048, true),
                iso_record(b"CDI_VCD.APP;1", 0, 0, false),
                iso_record(b"CDI_IMAG.RTF;1", 0, 0, false),
                iso_record(b"CDI_TEXT.FNT;1", 0, 0, false),
            ],
        );
        let mpegav = &mut sectors[MPEGAV_LBA as usize][24..24 + 2048];
        append_records(
            mpegav,
            &[
                iso_record(&[0], MPEGAV_LBA, 2048, true),
                iso_record(&[1], ROOT_LBA, 2048, true),
                iso_record(b"AVSEQ01.DAT;1", 0, 0, false),
            ],
        );
        let vcd = &mut sectors[VCD_LBA as usize][24..24 + 2048];
        append_records(
            vcd,
            &[
                iso_record(&[0], VCD_LBA, 2048, true),
                iso_record(&[1], ROOT_LBA, 2048, true),
                iso_record(b"INFO.VCD;1", INFO_LBA, 2048, false),
                iso_record(b"ENTRIES.VCD;1", ENTRIES_LBA, 2048, false),
                iso_record(b"LOT.VCD;1", LOT_LBA, 64 * 1024, false),
                iso_record(b"PSD.VCD;1", PSD_LBA, 56, false),
            ],
        );
        let info = &mut sectors[INFO_LBA as usize][24..24 + 2048];
        info[..8].copy_from_slice(b"VIDEO_CD");
        info[8..10].copy_from_slice(&0x0200u16.to_be_bytes());
        info[10..18].copy_from_slice(b"SYNTHPSD");
        info[26..28].copy_from_slice(&1u16.to_be_bytes());
        info[28..30].copy_from_slice(&1u16.to_be_bytes());
        info[44..48].copy_from_slice(&56u32.to_be_bytes());
        info[51] = 8;
        info[52..54].copy_from_slice(&3u16.to_be_bytes());

        let entries = &mut sectors[ENTRIES_LBA as usize][24..24 + 2048];
        entries[..8].copy_from_slice(b"ENTRYVCD");
        entries[8..10].copy_from_slice(&0x0200u16.to_be_bytes());
        entries[10..12].copy_from_slice(&2u16.to_be_bytes());
        entries[12..16].copy_from_slice(&[0x02, 0x00, 0x02, 0x00]);
        entries[16..20].copy_from_slice(&[0x02, 0x00, 0x03, 0x00]);

        let lot = &mut sectors[LOT_LBA as usize][24..24 + 2048];
        lot[2..4].copy_from_slice(&0u16.to_be_bytes());
        lot[4..6].copy_from_slice(&3u16.to_be_bytes());
        lot[6..8].copy_from_slice(&6u16.to_be_bytes());

        let psd = &mut sectors[PSD_LBA as usize][24..24 + 2048];
        psd[0] = 0x18;
        psd[2] = 2;
        psd[3] = 1;
        psd[4..6].copy_from_slice(&1u16.to_be_bytes());
        psd[6..8].copy_from_slice(&0xffffu16.to_be_bytes());
        psd[8..10].copy_from_slice(&3u16.to_be_bytes());
        psd[10..12].copy_from_slice(&0xffffu16.to_be_bytes());
        psd[12..14].copy_from_slice(&3u16.to_be_bytes());
        psd[14..16].copy_from_slice(&0xffffu16.to_be_bytes());
        psd[18..20].copy_from_slice(&100u16.to_be_bytes());
        psd[20..22].copy_from_slice(&3u16.to_be_bytes());
        psd[22..24].copy_from_slice(&6u16.to_be_bytes());
        psd[24] = 0x10;
        psd[25] = 1;
        psd[26..28].copy_from_slice(&2u16.to_be_bytes());
        psd[28..30].copy_from_slice(&0u16.to_be_bytes());
        psd[30..32].copy_from_slice(&6u16.to_be_bytes());
        psd[32..34].copy_from_slice(&0u16.to_be_bytes());
        psd[38..40].copy_from_slice(&101u16.to_be_bytes());
        psd[48] = 0x1f;

        let mut bin = std::fs::File::create(directory.join("video.bin")).unwrap();
        for sector in sectors {
            bin.write_all(&sector).unwrap();
        }
        std::fs::write(
            directory.join("video.cue"),
            "FILE \"video.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n",
        )
        .unwrap();

        let inventory = inspect_cue(&directory.join("video.cue")).unwrap();
        assert!(inventory.cd_rom_xa_bridge);
        assert!(inventory.has_cdi_application);
        assert_eq!(inventory.content_kind, DiscContentKind::VideoCd);
        let navigation = inventory.vcd_navigation.as_ref().unwrap();
        assert_eq!(navigation.specification_version, 0x0200);
        assert_eq!(navigation.album_id, "SYNTHPSD");
        assert_eq!(navigation.entries.len(), 2);
        assert_eq!(navigation.entries[1].absolute_frame, 225);
        assert_eq!(navigation.lists.len(), 3);
        assert_eq!(navigation.lists[0].kind, "selection");
        assert_eq!(navigation.lists[0].selection_offsets, [3, 6]);
        assert_eq!(navigation.lists[1].play_items, [101]);
        assert_eq!(navigation.lists[2].kind, "end");
        assert!(inventory.iso_volume.as_ref().is_some_and(|volume| {
            volume.application_id == "CDI/CDI_VCD.APP;1"
                && volume
                    .files
                    .iter()
                    .any(|file| file.path == "CDI/CDI_VCD.APP")
        }));

        let _ = std::fs::remove_dir_all(directory);
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
