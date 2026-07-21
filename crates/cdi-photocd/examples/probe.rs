// SPDX-License-Identifier: GPL-3.0-or-later
//! Probe a CUE file for Photo CD content and decode the first image.
//! Usage: cargo run -p cdi-photocd --example probe -- <path.cue> [tier]

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: probe <cue> [tier]");
    let tier: usize = args.next().and_then(|t| t.parse().ok()).unwrap_or(0);
    let mut disc = cdi_photocd::disc::open_disc(std::path::Path::new(&path)).expect("open disc");
    println!(
        "photo cd: {} images, serial {:?}, spec {:?}",
        disc.images.len(),
        disc.info.serial,
        disc.info.spec_version
    );
    for (i, img) in disc.images.iter().enumerate().take(8) {
        println!("  [{i}] {} lba={} size={}", img.name, img.lba, img.size);
    }
    let has_cdi = cdi_photocd::iso9660::find_entry(
        &mut *disc.reader,
        disc.pvd.root_lba,
        disc.pvd.root_size,
        "CDI",
    )
    .ok()
    .flatten()
    .map(|entry| entry.is_dir)
    .unwrap_or(false);
    println!("cd-i application directory present: {has_cdi}");
    let max_tier = cdi_photocd::decode::image_max_tier(&mut disc, 0);
    println!("image 0 max tier: {max_tier}");
    let image = cdi_photocd::decode::decode_image(&mut disc, 0, tier).expect("decode");
    println!(
        "decoded image 0 at tier {tier}: {}x{} ({} bytes RGB)",
        image.width,
        image.height,
        image.rgb.len()
    );
}
