// SPDX-License-Identifier: GPL-3.0-or-later
//! Store-only ZIP support for CUE/BIN disc images.
//!
//! Compressed disc data cannot be sector-read efficiently by the existing
//! file-backed core. E-Di therefore accepts only unencrypted Store entries,
//! matching the companion disc-player behavior, and streams them into a
//! temporary directory whose lifetime is retained by each consumer.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ExtractedDisc {
    pub cue_path: PathBuf,
    pub temp_dir: tempfile::TempDir,
}

fn inspect(zip: &mut zip::ZipArchive<File>) -> Result<PathBuf, String> {
    let mut cue_paths = Vec::new();
    for index in 0..zip.len() {
        let entry = zip
            .by_index_raw(index)
            .map_err(|error| format!("read ZIP directory: {error}"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        if entry.encrypted() {
            return Err(format!("{name} is encrypted"));
        }
        if entry.compression() != zip::CompressionMethod::Stored {
            return Err(format!(
                "{name} is compressed; re-pack the disc using Store/no compression"
            ));
        }
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| format!("unsafe ZIP path: {name}"))?;
        if enclosed
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("cue"))
        {
            cue_paths.push(enclosed);
        }
    }
    match cue_paths.len() {
        0 => Err("archive contains no CUE sheet".to_owned()),
        1 => Ok(cue_paths.remove(0)),
        count => Err(format!(
            "archive contains {count} CUE sheets; keep one disc per ZIP"
        )),
    }
}

/// Fast central-directory check used during library scanning.
pub fn is_eligible(path: &Path) -> bool {
    File::open(path)
        .map_err(|error| error.to_string())
        .and_then(|file| zip::ZipArchive::new(file).map_err(|error| error.to_string()))
        .and_then(|mut zip| inspect(&mut zip))
        .is_ok()
}

/// Validate and extract one store-only ZIP. Paths are retained rather than
/// flattened so relative FILE references in the CUE sheet keep working.
pub fn extract(path: &Path) -> Result<ExtractedDisc, String> {
    let file = File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|error| format!("invalid ZIP: {error}"))?;
    let cue_relative = inspect(&mut zip)?;
    let temp_dir = tempfile::Builder::new()
        .prefix("edi-store-zip-")
        .tempdir()
        .map_err(|error| format!("create temporary disc directory: {error}"))?;

    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| format!("read ZIP entry {index}: {error}"))?;
        let name = entry.name().to_owned();
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| format!("unsafe ZIP path: {name}"))?;
        let destination = temp_dir.path().join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&destination)
                .map_err(|error| format!("create {}: {error}", destination.display()))?;
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create {}: {error}", parent.display()))?;
        }
        let mut output = File::create(&destination)
            .map_err(|error| format!("create {}: {error}", destination.display()))?;
        io::copy(&mut entry, &mut output).map_err(|error| format!("extract {name}: {error}"))?;
    }

    let cue_path = temp_dir.path().join(cue_relative);
    Ok(ExtractedDisc { cue_path, temp_dir })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::{extract, is_eligible};

    fn make_zip(method: zip::CompressionMethod) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("disc.zip");
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default().compression_method(method);
        writer.start_file("release/disc.cue", options).unwrap();
        writer
            .write_all(b"FILE \"disc.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n")
            .unwrap();
        writer.start_file("release/disc.bin", options).unwrap();
        writer.write_all(&[0x5A; 2352]).unwrap();
        writer.finish().unwrap();
        (dir, path)
    }

    #[test]
    fn store_zip_is_detected_and_keeps_relative_layout() {
        let (_dir, path) = make_zip(zip::CompressionMethod::Stored);
        assert!(is_eligible(&path));
        let disc = extract(&path).unwrap();
        assert_eq!(disc.cue_path.file_name().unwrap(), "disc.cue");
        assert_eq!(
            std::fs::read(disc.cue_path.with_file_name("disc.bin")).unwrap(),
            vec![0x5A; 2352]
        );
    }

    #[test]
    fn compressed_zip_is_not_eligible() {
        let (_dir, path) = make_zip(zip::CompressionMethod::Deflated);
        assert!(!is_eligible(&path));
        assert!(extract(&path).unwrap_err().contains("compressed"));
    }
}
