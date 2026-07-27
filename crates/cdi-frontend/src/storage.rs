// SPDX-License-Identifier: GPL-3.0-or-later
//! Host persistence for the player's battery-backed SRAM.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Per-user data directory for files the app owns, such as saved NVRAM.
fn app_data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dir = if cfg!(target_os = "macos") {
        home?.join("Library/Application Support/cdi-frontend")
    } else if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var_os("APPDATA")?).join("cdi-frontend")
    } else {
        match std::env::var_os("XDG_DATA_HOME") {
            Some(base) => PathBuf::from(base).join("cdi-frontend"),
            None => home?.join(".local/share/cdi-frontend"),
        }
    };
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

pub(super) fn configured_nvram_path(
    board_name: &str,
    pal: bool,
    dvc_inserted: bool,
    persistent: bool,
) -> Option<PathBuf> {
    if !persistent {
        return None;
    }
    let standard = if pal { "pal" } else { "ntsc" };
    let cartridge = if dvc_inserted { "vmpeg" } else { "base" };
    app_data_dir().map(|dir| dir.join(format!("{board_name}-{standard}-{cartridge}.nvr")))
}

pub(super) fn load_nvram(path: Option<&Path>, expected_len: usize) -> Vec<u8> {
    let Some(path) = path else {
        return vec![0; expected_len];
    };
    match std::fs::read(path) {
        Ok(saved) if saved.len() == expected_len => {
            log::info!("nvram restored from {}", path.display());
            saved
        }
        Ok(saved) => {
            log::warn!(
                "nvram {}: expected {} bytes, found {}; ignoring",
                path.display(),
                expected_len,
                saved.len()
            );
            vec![0; expected_len]
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => vec![0; expected_len],
        Err(error) => {
            log::warn!("nvram {}: {error}", path.display());
            vec![0; expected_len]
        }
    }
}

/// Write battery-backed SRAM atomically, replacing any previous contents.
pub(super) fn write_nvram(path: Option<&Path>, data: &[u8]) -> Result<(), String> {
    let Some(path) = path else {
        return Ok(());
    };
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        format!(
            "create temporary NVRAM file in {}: {error}",
            parent.display()
        )
    })?;
    temporary
        .write_all(data)
        .map_err(|error| format!("write temporary NVRAM file: {error}"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("sync temporary NVRAM file: {error}"))?;
    temporary
        .persist(path)
        .map_err(|error| format!("replace {}: {}", path.display(), error.error))?;
    log::debug!("nvram saved to {}", path.display());
    Ok(())
}

pub(super) fn backup_nvram(path: Option<&Path>, data: &[u8]) -> Result<Option<PathBuf>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    if data.iter().all(|&byte| byte == 0) {
        return Ok(None);
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock: {error}"))?
        .as_secs();
    let backup = path.with_extension(format!("nvr.backup-{timestamp}"));
    std::fs::write(&backup, data)
        .map_err(|error| format!("write backup {}: {error}", backup.display()))?;
    Ok(Some(backup))
}
