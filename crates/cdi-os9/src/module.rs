// SPDX-License-Identifier: GPL-3.0-or-later
//! OS-9/68k memory module headers.
//!
//! Reference: Microware "OS-9/68000 Operating System Technical Manual",
//! module header layout (all fields big-endian):
//!
//! ```text
//! $00  u16  sync bytes ($4AFC)
//! $02  u16  system revision check value
//! $04  u32  module size in bytes
//! $08  u32  owner ID
//! $0C  u32  offset of module name string
//! $10  u16  access permissions
//! $12  u16  type (high byte) / language (low byte)
//! $14  u16  attributes (high byte) / revision (low byte)
//! $16  u16  edition
//! $18  u32  usage comment offset
//! $1C  u32  symbol table offset
//! $20  ..   reserved
//! $2E  u16  header parity (ones' complement of XOR of words $00..$2C)
//! ```

pub const SYNC: u16 = 0x4AFC;
const HEADER_LEN: usize = 0x30;

/// OS-9 module type codes (header byte at $12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleType {
    Program,
    Subroutine,
    Multi,
    Data,
    CsdData,
    TrapLib,
    System,
    FileManager,
    Driver,
    Descriptor,
    Other(u8),
}

impl From<u8> for ModuleType {
    fn from(v: u8) -> Self {
        match v {
            1 => Self::Program,
            2 => Self::Subroutine,
            3 => Self::Multi,
            4 => Self::Data,
            5 => Self::CsdData,
            11 => Self::TrapLib,
            12 => Self::System,
            13 => Self::FileManager,
            14 => Self::Driver,
            15 => Self::Descriptor,
            other => Self::Other(other),
        }
    }
}

impl ModuleType {
    pub fn code(self) -> u8 {
        match self {
            Self::Program => 1,
            Self::Subroutine => 2,
            Self::Multi => 3,
            Self::Data => 4,
            Self::CsdData => 5,
            Self::TrapLib => 11,
            Self::System => 12,
            Self::FileManager => 13,
            Self::Driver => 14,
            Self::Descriptor => 15,
            Self::Other(v) => v,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Program => "Prgrm",
            Self::Subroutine => "Sbrtn",
            Self::Multi => "Multi",
            Self::Data => "Data",
            Self::CsdData => "CSD",
            Self::TrapLib => "TrapLib",
            Self::System => "Systm",
            Self::FileManager => "FlMgr",
            Self::Driver => "Drivr",
            Self::Descriptor => "Devic",
            Self::Other(_) => "?",
        }
    }
}

/// A parsed OS-9 module found in a ROM image.
#[derive(Debug, Clone)]
pub struct Module {
    /// Byte offset of the module header within the scanned image.
    pub offset: u32,
    pub size: u32,
    pub name: String,
    pub mod_type: ModuleType,
    pub language: u8,
    pub attributes: u8,
    pub revision: u8,
    pub edition: u16,
    /// CRC-24 over the whole module matched the OS-9 constant.
    pub crc_ok: bool,
}

/// One colon-delimited entry from an OS-9 configuration-status descriptor.
///
/// CSD records begin with a decimal device-class number and a device name.
/// Remaining fields are retained verbatim because their operators are
/// class-specific (for example, `SZ#8` and `RV="CTP2"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsdRecord {
    pub device_class: u16,
    pub name: String,
    pub fields: Vec<String>,
}

/// Why a module could not be decoded as configuration-status data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsdParseError {
    NotCsdData,
    ModuleOutOfBounds,
    MissingDataOffset,
    InvalidDataOffset,
    MissingTerminator,
    InvalidAscii,
    InvalidRecord,
}

fn be16(d: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([d[off], d[off + 1]])
}

fn be32(d: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]])
}

/// Header parity: XOR of the 24 header words (including the parity word)
/// must be $FFFF.
fn header_parity_ok(h: &[u8]) -> bool {
    let mut acc: u16 = 0;
    for off in (0..HEADER_LEN).step_by(2) {
        acc ^= be16(h, off);
    }
    acc == 0xFFFF
}

/// OS-9 CRC-24, polynomial $800063. Over a complete module (CRC bytes
/// included) the accumulator ends at the constant $800FE3.
const CRC24_POLY: u32 = 0x80_0063;
const CRC24_MAGIC: u32 = 0x80_0FE3;

pub fn crc24(data: &[u8]) -> u32 {
    let mut acc: u32 = 0xFF_FFFF;
    for &byte in data {
        acc ^= (byte as u32) << 16;
        for _ in 0..8 {
            acc <<= 1;
            if acc & 0x100_0000 != 0 {
                acc ^= CRC24_POLY;
            }
        }
        acc &= 0xFF_FFFF;
    }
    acc
}

fn parse_module_at(image: &[u8], off: usize) -> Option<Module> {
    if off + HEADER_LEN > image.len()
        || be16(image, off) != SYNC
        || !header_parity_ok(&image[off..off + HEADER_LEN])
    {
        return None;
    }
    let size = be32(image, off + 0x04);
    let name_off = be32(image, off + 0x0C) as usize;
    let end = off.checked_add(size as usize)?;
    if size < HEADER_LEN as u32 || end > image.len() || name_off >= size as usize {
        return None;
    }
    let name = read_cstr(&image[off..end], name_off);
    let type_lang = be16(image, off + 0x12);
    let attr_rev = be16(image, off + 0x14);
    Some(Module {
        offset: off as u32,
        size,
        name,
        mod_type: ModuleType::from((type_lang >> 8) as u8),
        language: (type_lang & 0xFF) as u8,
        attributes: (attr_rev >> 8) as u8,
        revision: (attr_rev & 0xFF) as u8,
        edition: be16(image, off + 0x16),
        crc_ok: crc24(&image[off..end]) == CRC24_MAGIC,
    })
}

/// Scan an image for OS-9 module headers at even offsets.
///
/// Candidate headers must have valid parity and valid in-image bounds.
/// Scanning resumes after each structurally valid module, including one with
/// a bad CRC, because ROM module sets are laid out without overlap.
pub fn scan_modules(image: &[u8]) -> Vec<Module> {
    let mut modules = Vec::new();
    let mut off = 0usize;
    while off + HEADER_LEN <= image.len() {
        let Some(module) = parse_module_at(image, off) else {
            off += 2;
            continue;
        };
        let end = off + module.size as usize;
        modules.push(module);
        off = end.next_multiple_of(2).max(off + 2);
    }
    modules
}

/// Find independently CRC-valid modules in arbitrary live memory.
///
/// Unlike [`scan_modules`], a parity-valid but CRC-invalid false header does
/// not skip over later candidates. The returned collection is capped at
/// `limit`, keeping diagnostic output and allocation bounded.
pub fn scan_valid_modules(image: &[u8], limit: usize) -> Vec<Module> {
    let mut modules = Vec::with_capacity(limit.min(64));
    let mut off = 0usize;
    while off + HEADER_LEN <= image.len() && modules.len() < limit {
        let Some(module) = parse_module_at(image, off) else {
            off += 2;
            continue;
        };
        if module.crc_ok {
            let end = off + module.size as usize;
            modules.push(module);
            off = end.next_multiple_of(2).max(off + 2);
        } else {
            off += 2;
        }
    }
    modules
}

/// Parse the carriage-return-delimited records in a CSD data module.
///
/// Type-5 OS-9 modules place a big-endian, module-relative data offset at
/// `$30`. Records run from that offset to the first NUL byte. This function
/// deliberately reports only the firmware declaration; priorities and live
/// memory allocation are OS-9 runtime state and are not encoded here.
pub fn parse_csd_records(image: &[u8], module: &Module) -> Result<Vec<CsdRecord>, CsdParseError> {
    if module.mod_type != ModuleType::CsdData {
        return Err(CsdParseError::NotCsdData);
    }

    let start = module.offset as usize;
    let end = start
        .checked_add(module.size as usize)
        .filter(|&end| end <= image.len())
        .ok_or(CsdParseError::ModuleOutOfBounds)?;
    let bytes = &image[start..end];
    if bytes.len() < HEADER_LEN + 4 {
        return Err(CsdParseError::MissingDataOffset);
    }

    let data_offset = be32(bytes, HEADER_LEN) as usize;
    if !(HEADER_LEN + 4..bytes.len()).contains(&data_offset) {
        return Err(CsdParseError::InvalidDataOffset);
    }
    let data = &bytes[data_offset..];
    let data_end = data
        .iter()
        .position(|&byte| byte == 0)
        .ok_or(CsdParseError::MissingTerminator)?;
    let data = &data[..data_end];
    if !data.is_ascii() {
        return Err(CsdParseError::InvalidAscii);
    }

    let mut records = Vec::new();
    for raw in data.split(|&byte| byte == b'\r') {
        if raw.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(raw).map_err(|_| CsdParseError::InvalidAscii)?;
        let mut parts = text.split(':');
        let device_class = parts
            .next()
            .and_then(|part| part.parse().ok())
            .ok_or(CsdParseError::InvalidRecord)?;
        let name = parts
            .next()
            .filter(|name| !name.is_empty())
            .ok_or(CsdParseError::InvalidRecord)?;
        let fields = parts
            .filter(|field| !field.is_empty())
            .map(str::to_owned)
            .collect();
        records.push(CsdRecord {
            device_class,
            name: name.to_owned(),
            fields,
        });
    }
    Ok(records)
}

fn read_cstr(module: &[u8], start: usize) -> String {
    // OS-9 module names are ASCII; some tools set bit 7 on the last char.
    let mut out = String::new();
    for &b in &module[start..] {
        let ch = b & 0x7F;
        if ch == 0 {
            break;
        }
        out.push(ch as char);
        if b & 0x80 != 0 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid module for tests.
    fn synth_module(name: &str, mod_type: u8, revision: u8, size: u32) -> Vec<u8> {
        let mut m = vec![0u8; size as usize];
        m[0] = 0x4A;
        m[1] = 0xFC;
        m[4..8].copy_from_slice(&size.to_be_bytes());
        let name_off = HEADER_LEN as u32;
        m[0x0C..0x10].copy_from_slice(&name_off.to_be_bytes());
        m[0x12] = mod_type;
        m[0x13] = 1; // Objct
        m[0x15] = revision;
        let name_bytes = name.as_bytes();
        m[HEADER_LEN..HEADER_LEN + name_bytes.len()].copy_from_slice(name_bytes);
        // Fix header parity.
        let mut acc: u16 = 0;
        for off in (0..0x2E).step_by(2) {
            acc ^= be16(&m, off);
        }
        let parity = acc ^ 0xFFFF;
        m[0x2E..0x30].copy_from_slice(&parity.to_be_bytes());
        // Fix CRC: compute over module with CRC bytes zeroed, then store the
        // complemented accumulator in the last 3 bytes.
        let len = m.len();
        let acc = crc24(&m[..len - 3]);
        let stored = !acc & 0xFF_FFFF;
        m[len - 3] = (stored >> 16) as u8;
        m[len - 2] = (stored >> 8) as u8;
        m[len - 1] = stored as u8;
        m
    }

    #[test]
    fn scans_synthetic_module() {
        let mut image = vec![0u8; 64];
        image.extend(synth_module("kernel", 12, 51, 0x60));
        image.extend(vec![0u8; 32]);
        let mods = scan_modules(&image);
        assert_eq!(mods.len(), 1);
        let m = &mods[0];
        assert_eq!(m.name, "kernel");
        assert_eq!(m.offset, 64);
        assert_eq!(m.size, 0x60);
        assert_eq!(m.mod_type, ModuleType::System);
        assert_eq!(m.revision, 51);
        assert!(m.crc_ok, "synthetic CRC must validate");
    }

    #[test]
    fn rejects_bad_parity() {
        let mut m = synth_module("kernel", 12, 1, 0x60);
        m[0x16] ^= 0xFF; // corrupt edition without fixing parity
        assert!(scan_modules(&m).is_empty());
    }

    #[test]
    fn live_scan_does_not_let_a_bad_crc_header_hide_a_valid_module() {
        let mut image = vec![0u8; 0x200];
        let false_header = synth_module("false", 12, 1, 0x180);
        image[..false_header.len()].copy_from_slice(&false_header);
        image[0x17f] ^= 1;

        let valid = synth_module("runtime", 12, 1, 0x60);
        image[0x80..0x80 + valid.len()].copy_from_slice(&valid);

        let modules = scan_valid_modules(&image, 64);
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].offset, 0x80);
        assert_eq!(modules[0].name, "runtime");
    }

    #[test]
    fn live_scan_obeys_its_output_limit() {
        let module = synth_module("runtime", 12, 1, 0x60);
        let mut image = Vec::new();
        image.extend_from_slice(&module);
        image.extend_from_slice(&module);

        assert!(scan_valid_modules(&image, 0).is_empty());
        assert_eq!(scan_valid_modules(&image, 1).len(), 1);
    }
}
