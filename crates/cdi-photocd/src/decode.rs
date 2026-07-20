// SPDX-License-Identifier: GPL-2.0-or-later
//! One-call decoding of a disc image entry to interleaved RGB, wrapping the
//! Base / 4Base / 16Base pipeline, Kodak USA raw variants, and INFO.PCD
//! rotation. Ported from the Photo CD Player GUI's decode worker.

use crate::base::{decode_base_plane, BASE_H, BASE_RAW_LEN, BASE_W};
use crate::disc::{read_image_pack, read_raw_rgb_variant, OpenedDisc};
use crate::hires::{
    decode_16base, decode_4base, read_ipa_byte, resolution_order, FOURBASE_H, FOURBASE_W, SECTOR,
    SIXTEENBASE_H, SIXTEENBASE_W,
};

/// Sector offset of the Base image data inside an image pack.
const BASE_OFF: usize = 96 * SECTOR;
/// Sectors needed to decode Base only.
const BASE_PACK_SECTORS: usize = 384;
/// Sectors covering the largest observed image packs (matches the GUI).
const FULL_PACK_SECTORS: usize = 3000;

pub const TIER_LABELS: [&str; 3] = ["Base", "4Base", "16Base"];

#[derive(Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// Interleaved 8-bit RGB, row-major.
    pub rgb: Vec<u8>,
}

/// The highest decodable tier (0=Base, 1=4Base, 2=16Base) for one image.
pub fn image_max_tier(disc: &mut OpenedDisc, index: usize) -> usize {
    let Some(img) = disc.images.get(index).cloned() else {
        return 0;
    };
    if let Some(vs) = &img.rgb_variants {
        return vs.max_tier();
    }
    match read_image_pack(&mut *disc.reader, &img, 2) {
        Ok(head) => resolution_order(read_ipa_byte(&head)).min(2) as usize,
        Err(_) => 0,
    }
}

/// Decode image `index` at `tier` (clamped to what the image offers).
pub fn decode_image(
    disc: &mut OpenedDisc,
    index: usize,
    tier: usize,
) -> Result<DecodedImage, String> {
    let img = disc
        .images
        .get(index)
        .cloned()
        .ok_or_else(|| "image index out of range".to_owned())?;

    // Kodak Photo CD (USA) raw uncompressed variants.
    if let Some(vs) = &img.rgb_variants {
        let v = vs
            .best_for(tier)
            .ok_or_else(|| "no RGB variant present".to_owned())?;
        let rgb = read_raw_rgb_variant(&mut *disc.reader, v).map_err(|e| e.to_string())?;
        return Ok(DecodedImage {
            width: v.width,
            height: v.height,
            rgb,
        });
    }

    let sectors = if tier == 0 {
        BASE_PACK_SECTORS
    } else {
        FULL_PACK_SECTORS
    };
    let pack =
        read_image_pack(&mut *disc.reader, &img, sectors).map_err(|e| format!("read pack: {e}"))?;
    if pack.len() < BASE_OFF + BASE_RAW_LEN {
        return Err("image pack too short for Base".to_owned());
    }
    let base_rgb =
        decode_base_plane(&pack[BASE_OFF..BASE_OFF + BASE_RAW_LEN]).map_err(|e| e.to_string())?;

    let tier = tier.min(resolution_order(read_ipa_byte(&pack)) as usize);
    let (width, height, rgb) = match tier {
        0 => (BASE_W as u32, BASE_H as u32, base_rgb),
        1 => {
            let rgb = decode_4base(&pack, &base_rgb).map_err(|e| format!("4base: {e}"))?;
            (FOURBASE_W as u32, FOURBASE_H as u32, rgb)
        }
        _ => {
            let fb = decode_4base(&pack, &base_rgb).map_err(|e| format!("4base: {e}"))?;
            let sb = decode_16base(&pack, &fb).map_err(|e| format!("16base: {e}"))?;
            (SIXTEENBASE_W as u32, SIXTEENBASE_H as u32, sb)
        }
    };

    let rotation = disc
        .info
        .image_descriptors
        .get(index)
        .map(|d| d.rotation)
        .unwrap_or(0);
    let (width, height, rgb) = apply_rotation(width, height, rgb, rotation);
    Ok(DecodedImage { width, height, rgb })
}

/// Rotate an RGB buffer by the 2-bit rotation code from INFO.PCD:
/// 0=none, 1=90° CCW, 2=180°, 3=270° CCW.
fn apply_rotation(w: u32, h: u32, rgb: Vec<u8>, rotation: u8) -> (u32, u32, Vec<u8>) {
    match rotation & 0x03 {
        0 => (w, h, rgb),
        2 => {
            let mut out = vec![0u8; rgb.len()];
            let n_pixels = (w * h) as usize;
            for i in 0..n_pixels {
                let src = i * 3;
                let dst = (n_pixels - 1 - i) * 3;
                out[dst..dst + 3].copy_from_slice(&rgb[src..src + 3]);
            }
            (w, h, out)
        }
        rot => {
            // 90 CCW: (x, y) -> (y, w-1-x); 270 CCW: (x, y) -> (h-1-y, x).
            let (nw, nh) = (h, w);
            let mut out = vec![0u8; rgb.len()];
            for y in 0..h {
                for x in 0..w {
                    let src = (y * w + x) as usize * 3;
                    let (nx, ny) = if rot == 1 {
                        (y, w - 1 - x)
                    } else {
                        (h - 1 - y, x)
                    };
                    let dst = (ny * nw + nx) as usize * 3;
                    out[dst..dst + 3].copy_from_slice(&rgb[src..src + 3]);
                }
            }
            (nw, nh, out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_180_reverses_pixels() {
        let rgb = vec![1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4];
        let (w, h, out) = apply_rotation(2, 2, rgb, 2);
        assert_eq!((w, h), (2, 2));
        assert_eq!(out, vec![4, 4, 4, 3, 3, 3, 2, 2, 2, 1, 1, 1]);
    }

    #[test]
    fn rotation_90ccw_transposes_dimensions() {
        let rgb = vec![1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 5, 6, 6, 6];
        let (w, h, out) = apply_rotation(3, 2, rgb, 1);
        assert_eq!((w, h), (2, 3));
        // (0,0)->(0, 2), so pixel 1 lands at row 2, col 0.
        assert_eq!(&out[(2 * 2) * 3..(2 * 2) * 3 + 3], &[1, 1, 1]);
    }
}
