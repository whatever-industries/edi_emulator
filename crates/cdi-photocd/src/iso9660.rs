//! ISO 9660 / High Sierra PVD + directory walker.
//!
//! Handles the CD-ROM XA quirk where directory records have an extra
//! system-use byte, pushing the file-identifier length from byte 31 to 32.

use crate::reader::{ReaderError, SectorReader, ISO_SECTOR_SIZE};

pub const PVD_SECTOR: u32 = 16;

#[derive(Debug, thiserror::Error)]
pub enum IsoError {
    #[error("reader: {0}")]
    Reader(#[from] ReaderError),
    #[error("not an ISO 9660 or High Sierra volume")]
    NotAVolume,
    #[error("sector 16 is not a PVD (type={0})")]
    NotPvd(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscFormat {
    Iso9660,
    HighSierra,
}

#[derive(Debug, Clone)]
pub struct Pvd {
    pub volume_id: String,
    pub root_lba: u32,
    pub root_size: u32,
    pub format: DiscFormat,
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub lba: u32,
    pub size: u32,
    pub is_dir: bool,
}

pub fn read_pvd<R: SectorReader + ?Sized>(reader: &mut R) -> Result<Pvd, IsoError> {
    let sector = reader.read_sector(PVD_SECTOR)?;
    // ISO 9660
    if &sector[1..6] == b"CD001" {
        let vd_type = sector[0];
        if vd_type != 1 {
            return Err(IsoError::NotPvd(vd_type));
        }
        let volume_id = String::from_utf8_lossy(&sector[40..72]).trim().to_string();
        let root_rec = &sector[156..190];
        let root_lba = u32::from_le_bytes([root_rec[2], root_rec[3], root_rec[4], root_rec[5]]);
        let root_size =
            u32::from_le_bytes([root_rec[10], root_rec[11], root_rec[12], root_rec[13]]);
        return Ok(Pvd {
            volume_id,
            root_lba,
            root_size,
            format: DiscFormat::Iso9660,
        });
    }
    // High Sierra Group
    if &sector[9..14] == b"CDROM" {
        let volume_id = String::from_utf8_lossy(&sector[40..72]).trim().to_string();
        let root_rec = &sector[180..214];
        let root_lba = u32::from_le_bytes([root_rec[2], root_rec[3], root_rec[4], root_rec[5]]);
        let root_size =
            u32::from_le_bytes([root_rec[10], root_rec[11], root_rec[12], root_rec[13]]);
        return Ok(Pvd {
            volume_id,
            root_lba,
            root_size,
            format: DiscFormat::HighSierra,
        });
    }
    Err(IsoError::NotAVolume)
}

/// Parse one directory record at `pos` in `sector`. Returns None on zero-length
/// (sector padding) or malformed record.
fn parse_dir_record(sector: &[u8], pos: usize) -> Option<(DirEntry, usize)> {
    if pos >= sector.len() {
        return None;
    }
    let record_len = sector[pos] as usize;
    if record_len == 0 || pos + record_len > sector.len() {
        return None;
    }
    let rec = &sector[pos..pos + record_len];
    if rec.len() < 33 {
        return None;
    }
    let lba = u32::from_le_bytes([rec[2], rec[3], rec[4], rec[5]]);
    let size = u32::from_le_bytes([rec[10], rec[11], rec[12], rec[13]]);
    let flags = rec[24];
    let mut is_dir = (flags & 0x02) != 0;

    // XA discs push fi_len to byte 32; fall back to byte 31.
    let fi_len_32 = rec[32] as usize;
    let fi_len_31 = rec[31] as usize;
    let (fi_len, fi_start) = if fi_len_32 > 0 && 33 + fi_len_32 <= record_len {
        (fi_len_32, 33)
    } else {
        (fi_len_31, 32)
    };
    if fi_start + fi_len > record_len {
        return None;
    }
    let fi = &rec[fi_start..fi_start + fi_len];

    let name = if fi == b"\x00" || fi == b"\x00\x00" {
        is_dir = true;
        ".".to_string()
    } else if fi == b"\x01" || fi == b"\x01\x01" {
        is_dir = true;
        "..".to_string()
    } else {
        let mut s = String::from_utf8_lossy(fi).into_owned();
        if let Some(semi) = s.find(';') {
            s.truncate(semi);
        }
        if !is_dir && !s.contains('.') {
            is_dir = true;
        }
        s
    };

    Some((
        DirEntry {
            name,
            lba,
            size,
            is_dir,
        },
        record_len,
    ))
}

/// List all entries in a directory, deduplicating by uppercase name.
pub fn list_directory<R: SectorReader + ?Sized>(
    reader: &mut R,
    dir_lba: u32,
    dir_size: u32,
) -> Result<Vec<DirEntry>, IsoError> {
    let mut entries = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let sectors = (dir_size as usize).div_ceil(ISO_SECTOR_SIZE);
    for s in 0..sectors {
        let sector = reader.read_sector(dir_lba + s as u32)?;
        let mut pos = 0usize;
        while pos < ISO_SECTOR_SIZE {
            match parse_dir_record(&sector, pos) {
                Some((entry, rec_len)) => {
                    pos += rec_len;
                    if entry.name == "." || entry.name == ".." {
                        continue;
                    }
                    let key = entry.name.to_uppercase();
                    if seen.contains(&key) {
                        continue;
                    }
                    seen.push(key);
                    entries.push(entry);
                }
                None => break,
            }
        }
    }
    Ok(entries)
}

/// Case-insensitive lookup of a named entry in a directory.
pub fn find_entry<R: SectorReader + ?Sized>(
    reader: &mut R,
    dir_lba: u32,
    dir_size: u32,
    target: &str,
) -> Result<Option<DirEntry>, IsoError> {
    let entries = list_directory(reader, dir_lba, dir_size)?;
    let upper = target.to_uppercase();
    Ok(entries.into_iter().find(|e| e.name.to_uppercase() == upper))
}
