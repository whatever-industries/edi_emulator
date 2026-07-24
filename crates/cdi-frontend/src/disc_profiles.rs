// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact-disc identification and hardware-standard profiles.
//!
//! CD-i application data does not carry a universal PAL/NTSC declaration.
//! Profiles therefore identify an exact Redump pressing by the ordered SHA-1
//! hashes of the files named by its CUE sheet. Profiles may recommend player
//! hardware timing, but presentation geometry always comes from the MCD212.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

const PROFILE_DATA: &str = include_str!("../../../data/cdi-disc-profiles.json");

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoStandardRecommendation {
    Pal,
    Ntsc,
}

impl VideoStandardRecommendation {
    pub fn is_pal(self) -> bool {
        self == Self::Pal
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct DiscProfile {
    pub redump_id: u32,
    pub name: String,
    pub serial: Option<String>,
    pub version: Option<String>,
    /// One hash per distinct CUE FILE, in CUE order. CUE text is not part of
    /// the identity because local filenames and whitespace may differ.
    pub track_sha1: Vec<String>,
    pub video_standard: VideoStandardRecommendation,
    pub note: Option<String>,
}

impl DiscProfile {
    pub fn fingerprint(&self) -> String {
        self.track_sha1.join("+")
    }
}

#[derive(Debug, Deserialize)]
struct ProfileDatabase {
    schema: u32,
    source: String,
    profiles: Vec<DiscProfile>,
}

#[derive(Clone, Debug)]
pub struct DiscIdentity {
    pub fingerprint: String,
    pub profile: Option<DiscProfile>,
}

fn database() -> Result<ProfileDatabase, String> {
    let database: ProfileDatabase = serde_json::from_str(PROFILE_DATA)
        .map_err(|error| format!("disc profile data: {error}"))?;
    if database.schema != 1 {
        return Err(format!(
            "unsupported disc profile schema {} from {}",
            database.schema, database.source
        ));
    }
    let mut fingerprints = BTreeSet::new();
    for profile in &database.profiles {
        if profile.track_sha1.is_empty()
            || profile
                .track_sha1
                .iter()
                .any(|hash| hash.len() != 40 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(format!("invalid SHA-1 list for {}", profile.name));
        }
        if !fingerprints.insert(profile.fingerprint()) {
            return Err(format!("duplicate exact-disc profile for {}", profile.name));
        }
    }
    Ok(database)
}

fn sha1_file(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Sha1::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Hash each distinct CUE FILE in its declared order and match the compact,
/// tracked profile database. Large media is streamed and never retained.
pub fn identify_disc(cue_path: &Path) -> Result<DiscIdentity, String> {
    let text = std::fs::read_to_string(cue_path)
        .map_err(|error| format!("read {}: {error}", cue_path.display()))?;
    let base = cue_path.parent().unwrap_or_else(|| Path::new("."));
    let cue_files = cdi_disc::parse_cue(&text, base)?;
    let mut seen = BTreeSet::<PathBuf>::new();
    let mut hashes = Vec::new();
    for cue_file in cue_files {
        if seen.insert(cue_file.path.clone()) {
            hashes.push(sha1_file(&cue_file.path)?);
        }
    }
    let fingerprint = hashes.join("+");
    let profile = database()?
        .profiles
        .into_iter()
        .find(|candidate| candidate.fingerprint() == fingerprint);
    Ok(DiscIdentity {
        fingerprint,
        profile,
    })
}

#[cfg(test)]
mod tests {
    use super::{database, VideoStandardRecommendation};

    #[test]
    fn tracked_profiles_are_valid_and_unique() {
        let profiles = database().unwrap().profiles;
        assert_eq!(profiles.len(), 4);
        assert!(profiles
            .iter()
            .all(|profile| !profile.track_sha1.is_empty()));
    }

    #[test]
    fn merlin_pressings_recommend_player_timing_only() {
        let profiles = database().unwrap().profiles;
        let europe = profiles
            .iter()
            .find(|profile| profile.redump_id == 54833)
            .unwrap();
        assert_eq!(europe.video_standard, VideoStandardRecommendation::Pal);

        for usa in profiles
            .iter()
            .filter(|profile| profile.name.starts_with("Merlin's Apprentice (USA"))
        {
            assert_eq!(usa.video_standard, VideoStandardRecommendation::Ntsc);
        }
    }

    #[test]
    fn apprentice_usa_recommends_ntsc_without_presentation_policy() {
        let profiles = database().unwrap().profiles;
        let apprentice = profiles
            .iter()
            .find(|profile| profile.redump_id == 78866)
            .unwrap();
        assert_eq!(apprentice.video_standard, VideoStandardRecommendation::Ntsc);
    }
}
