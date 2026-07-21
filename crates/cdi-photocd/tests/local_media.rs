// SPDX-License-Identifier: GPL-3.0-or-later
//! Local, media-gated integration tests.
//!
//! These run only when the private Photo CD disc library exists on this
//! machine; on any other machine (including CI) they skip silently. Disc
//! media is never committed to the repository.

use std::path::PathBuf;

const LIBRARY: &str = "/Volumes/Projects/Coding/Disc Images/Photo CD";

fn cue_for(disc_dir: &str) -> Option<PathBuf> {
    let path = PathBuf::from(LIBRARY)
        .join(disc_dir)
        .join(format!("{disc_dir}.cue"));
    path.is_file().then_some(path)
}

fn has_cdi_dir(disc: &mut cdi_photocd::disc::OpenedDisc) -> bool {
    cdi_photocd::iso9660::find_entry(
        &mut *disc.reader,
        disc.pvd.root_lba,
        disc.pvd.root_size,
        "CDI",
    )
    .ok()
    .flatten()
    .map(|entry| entry.is_dir)
    .unwrap_or(false)
}

#[test]
fn bridge_disc_with_cdi_application_decodes() {
    let Some(cue) = cue_for("Ghost in the Shell - Premium Photo-CD (Japan)") else {
        eprintln!("local media library not present; skipping");
        return;
    };
    let mut disc = cdi_photocd::disc::open_disc(&cue).expect("open bridge disc");
    assert_eq!(disc.images.len(), 124);
    assert!(
        has_cdi_dir(&mut disc),
        "bridge disc must expose its CD-i application directory"
    );
    let image = cdi_photocd::decode::decode_image(&mut disc, 0, 0).expect("decode Base");
    assert_eq!((image.width, image.height), (768, 512));
    assert_eq!(image.rgb.len(), 768 * 512 * 3);
}

#[test]
fn non_compliant_disc_without_cdi_application_decodes() {
    let Some(cue) = cue_for("Aktuelles Berlin - Sightseeing - Kultur - Erlebnis (Germany)") else {
        eprintln!("local media library not present; skipping");
        return;
    };
    let mut disc = cdi_photocd::disc::open_disc(&cue).expect("open non-compliant disc");
    assert_eq!(disc.images.len(), 100);
    assert!(
        !has_cdi_dir(&mut disc),
        "this disc is the known no-CDI-application exception"
    );
    let image = cdi_photocd::decode::decode_image(&mut disc, 0, 0).expect("decode Base");
    assert_eq!((image.width, image.height), (768, 512));
}
