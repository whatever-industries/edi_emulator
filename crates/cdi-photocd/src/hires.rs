//! 4Base (1536×1024) and 16Base (3072×2048) Huffman residual decoder.
//!
//! Layout (spec Sections IV.3.12–21, confirmed via pcdtojpeg):
//!   sector 384:       4Base ICA
//!   sectors 385–386:  4Base LPT-MRS
//!   sector 387:       4Base LPT (1 sector, 512 BE u32 entries)
//!   sector 388:       4Base HCT
//!   sectors 389+:     4Base ICD (Huffman-coded residuals)
//!   then immediately: 16Base (ICA at +0, LPT at +9 [2 sectors], HCT at +11,
//!                     ICD at +13).
//!
//! Huffman class is read from the IPA byte (sector 1, byte 10) bits 6–5.
//! For 16Base the Huffman class is in the 16Base ICA byte 0 bits 6–5.
//!
//! This module works on a fully-read image pack byte buffer — no I/O.

use crate::huffman::{decode_row, BitStream, HuffmanTable};
use crate::ycc::GAMMA_FWD;

pub const SECTOR: usize = 2048;

pub const FOURBASE_W: usize = 1536;
pub const FOURBASE_H: usize = 1024;
pub const FOURBASE_CW: usize = FOURBASE_W / 2;
pub const FOURBASE_CH: usize = FOURBASE_H / 2;

pub const SIXTEENBASE_W: usize = 3072;
pub const SIXTEENBASE_H: usize = 2048;
pub const SIXTEENBASE_CW: usize = SIXTEENBASE_W / 2;
pub const SIXTEENBASE_CH: usize = SIXTEENBASE_H / 2;

const IPA_SECTOR: usize = 1;
const FOURBASE_ICA_SECTOR: usize = 384;
const FOURBASE_LPT_SECTOR: usize = 387;
const FOURBASE_ICD_SECTOR: usize = 389;

#[derive(Debug, thiserror::Error)]
pub enum HiresError {
    #[error("image pack too short: need {need} bytes, got {have}")]
    PackTooShort { need: usize, have: usize },
    #[error("invalid IPA signature")]
    BadIpa,
    #[error("invalid Huffman class {0}")]
    BadClass(u8),
}

/// Read IPA byte from sector 1 byte 10. Returns 0 if signature wrong.
pub fn read_ipa_byte(pack: &[u8]) -> u8 {
    let sec = IPA_SECTOR * SECTOR;
    if pack.len() < sec + 11 {
        return 0;
    }
    if &pack[sec..sec + 7] != b"PCD_IPI" {
        return 0;
    }
    pack[sec + 10]
}

/// Huffman class 1..=4 from IPA byte bits 6–5.
pub fn huffman_class(ipa_byte: u8) -> u8 {
    ((ipa_byte >> 5) & 0x03) + 1
}

/// Resolution order 0..=2 from IPA byte bits 3–2 (0=Base, 1=4Base, 2=16Base).
pub fn resolution_order(ipa_byte: u8) -> u8 {
    (ipa_byte >> 2) & 0x03
}

/// Rotation bits 1-0 from IPA byte.
pub fn rotation_bits(ipa_byte: u8) -> u8 {
    ipa_byte & 0x03
}

/// De-interleave an MRS-formatted sector: skip 64-byte header, take every 4th byte.
fn demrs(raw: &[u8]) -> Vec<u8> {
    if raw.len() < 64 {
        return raw.to_vec();
    }
    let payload = &raw[64..];
    let n = payload.len() / 4;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(payload[i * 4]);
    }
    out
}

/// Find the first sector of 16Base data by reading the 4Base ICA.
/// Returns `FOURBASE_ICD_SECTOR + 4base_icd_sector_count`.
pub fn find_sixteenbase_start(pack: &[u8]) -> usize {
    let off = FOURBASE_ICA_SECTOR * SECTOR;
    if pack.len() < off + SECTOR {
        return FOURBASE_ICD_SECTOR + 512;
    }
    let mut ica: &[u8] = &pack[off..off + SECTOR];
    let demrs_buf;
    if ica.len() >= 32 && ica[..32] == [0xFFu8; 32] {
        demrs_buf = demrs(ica);
        ica = &demrs_buf;
    }
    if ica.len() >= 6 {
        let sector_count = u16::from_be_bytes([ica[4], ica[5]]) as usize;
        if sector_count > 0 && sector_count < 4096 {
            return FOURBASE_ICD_SECTOR + sector_count;
        }
    }
    FOURBASE_ICD_SECTOR + 512
}

/// Read LPT: `n_sectors` sectors of 4-byte big-endian row offsets.
fn read_lpt(pack: &[u8], start_sector: usize, n_sectors: usize, max_entries: usize) -> Vec<u32> {
    let off = start_sector * SECTOR;
    let end = (off + n_sectors * SECTOR).min(pack.len());
    let bytes = &pack[off..end];
    let cap = (bytes.len() / 4).min(max_entries);
    let mut out = Vec::with_capacity(cap);
    for i in 0..cap {
        let b = &bytes[i * 4..i * 4 + 4];
        out.push(u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    }
    out
}

/// Decode one residual plane: `n_rows × row_width` signed i16 values.
fn decode_plane(
    icd: &[u8],
    lpt: &[u32],
    table: &HuffmanTable,
    n_rows: usize,
    row_width: usize,
    plane_offset: usize,
) -> Vec<i16> {
    let mut plane = vec![0i16; n_rows * row_width];
    for row in 0..n_rows {
        let lpt_idx = plane_offset + row;
        if lpt_idx >= lpt.len() {
            break;
        }
        let byte_off = lpt[lpt_idx] as usize;
        if byte_off >= icd.len() {
            break;
        }
        let mut bs = BitStream::new(&icd[byte_off..]);
        let row_slice = &mut plane[row * row_width..(row + 1) * row_width];
        decode_row(&mut bs, table, row_width, row_slice);
    }
    plane
}

/// Bilinear 2× upsample of a `w × h` interleaved RGB buffer to `(2w) × (2h)`.
/// Matches PIL: `src = (dst + 0.5) * scale - 0.5`.
fn bilinear_upsample_rgb_2x(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let ow = w * 2;
    let oh = h * 2;
    let mut out = vec![0u8; ow * oh * 3];
    for oy in 0..oh {
        let sy = (oy as f32) * 0.5 - 0.25;
        let y0f = sy.floor();
        let y0 = y0f as isize;
        let y1 = y0 + 1;
        let fy = sy - y0f;
        let y0c = clamp_idx(y0, h);
        let y1c = clamp_idx(y1, h);
        for ox in 0..ow {
            let sx = (ox as f32) * 0.5 - 0.25;
            let x0f = sx.floor();
            let x0 = x0f as isize;
            let x1 = x0 + 1;
            let fx = sx - x0f;
            let x0c = clamp_idx(x0, w);
            let x1c = clamp_idx(x1, w);
            for c in 0..3 {
                let p00 = src[(y0c * w + x0c) * 3 + c] as f32;
                let p01 = src[(y0c * w + x1c) * 3 + c] as f32;
                let p10 = src[(y1c * w + x0c) * 3 + c] as f32;
                let p11 = src[(y1c * w + x1c) * 3 + c] as f32;
                let top = p00 + (p01 - p00) * fx;
                let bot = p10 + (p11 - p10) * fx;
                let v = top + (bot - top) * fy;
                out[(oy * ow + ox) * 3 + c] = clamp_u8_round(v);
            }
        }
    }
    out
}

/// Bilinear 2× upsample of a u8 plane, matching PIL BILINEAR convention.
fn bilinear_upsample_plane_to(src: &[u8], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u8> {
    let mut out = vec![0u8; dw * dh];
    let sx_scale = sw as f32 / dw as f32;
    let sy_scale = sh as f32 / dh as f32;
    for oy in 0..dh {
        let sy = (oy as f32 + 0.5) * sy_scale - 0.5;
        let y0f = sy.floor();
        let y0 = y0f as isize;
        let y1 = y0 + 1;
        let fy = sy - y0f;
        let y0c = clamp_idx(y0, sh);
        let y1c = clamp_idx(y1, sh);
        for ox in 0..dw {
            let sx = (ox as f32 + 0.5) * sx_scale - 0.5;
            let x0f = sx.floor();
            let x0 = x0f as isize;
            let x1 = x0 + 1;
            let fx = sx - x0f;
            let x0c = clamp_idx(x0, sw);
            let x1c = clamp_idx(x1, sw);
            let p00 = src[y0c * sw + x0c] as f32;
            let p01 = src[y0c * sw + x1c] as f32;
            let p10 = src[y1c * sw + x0c] as f32;
            let p11 = src[y1c * sw + x1c] as f32;
            let top = p00 + (p01 - p00) * fx;
            let bot = p10 + (p11 - p10) * fx;
            let v = top + (bot - top) * fy;
            out[oy * dw + ox] = clamp_u8_round(v);
        }
    }
    out
}

/// Apply Huffman residuals to an upsampled tier's RGB. Writes a new RGB buffer.
///
/// The upsampled RGB is inverse-gamma'd and converted back to approximate
/// YCbCr (matching Python reference), then residuals are added:
///   - Y residual: full-resolution, applied in display space (×255/209)
///   - Cb/Cr residuals: half-resolution, added to downsampled chroma
///
/// Then the chroma is upsampled and the result re-encoded to sRGB with gamma.
fn apply_residuals(
    upsampled_rgb: &[u8],
    width: usize,
    height: usize,
    y_res: &[i16],
    cb_res: &[i16],
    cr_res: &[i16],
) -> Vec<u8> {
    let cw = width / 2;
    let ch = height / 2;
    let inv_gamma = 1.0f32 / GAMMA_FWD;
    let y_scale = 255.0f32 / 209.0;

    // Build display-space Y and full-res Cb/Cr (approx) planes.
    let n = width * height;
    let mut y_disp = vec![0f32; n];
    let mut cb_full_f = vec![0f32; n];
    let mut cr_full_f = vec![0f32; n];

    for i in 0..n {
        let r = upsampled_rgb[i * 3] as f32 / 255.0;
        let g = upsampled_rgb[i * 3 + 1] as f32 / 255.0;
        let b = upsampled_rgb[i * 3 + 2] as f32 / 255.0;
        let rl = r.powf(inv_gamma) * 255.0;
        let gl = g.powf(inv_gamma) * 255.0;
        let bl = b.powf(inv_gamma) * 255.0;
        let yv = (0.299 * rl + 0.587 * gl + 0.114 * bl).clamp(0.0, 255.0);
        let cbv = (-0.169 * rl - 0.331 * gl + 0.499 * bl + 156.0).clamp(0.0, 255.0);
        let crv = (0.499 * rl - 0.418 * gl - 0.0813 * bl + 137.0).clamp(0.0, 255.0);
        y_disp[i] = yv;
        cb_full_f[i] = cbv;
        cr_full_f[i] = crv;
    }

    // Y: add residual scaled to display space.
    let mut y_new = vec![0f32; n];
    for i in 0..n {
        let v = y_disp[i] + y_res[i] as f32 * y_scale;
        y_new[i] = v.clamp(0.0, 255.0);
    }

    // Chroma: downsample approximate Cb/Cr with step-2 (matching Python's [::2,::2]),
    // add residual, clamp to [0,255], convert to u8, then bilinear-upsample back.
    let mut cb_small = vec![0u8; cw * ch];
    let mut cr_small = vec![0u8; cw * ch];
    for oy in 0..ch {
        let sy = oy * 2;
        for ox in 0..cw {
            let sx = ox * 2;
            let src = sy * width + sx;
            let dst = oy * cw + ox;
            let cbv = (cb_full_f[src] + cb_res[dst] as f32).clamp(0.0, 255.0);
            let crv = (cr_full_f[src] + cr_res[dst] as f32).clamp(0.0, 255.0);
            // Python: .astype(np.uint8) truncates toward zero after clipping.
            cb_small[dst] = cbv as u8;
            cr_small[dst] = crv as u8;
        }
    }

    let cb_full = bilinear_upsample_plane_to(&cb_small, cw, ch, width, height);
    let cr_full = bilinear_upsample_plane_to(&cr_small, cw, ch, width, height);

    // Final YCbCr → RGB with display-space Y and gamma.
    let mut out = vec![0u8; n * 3];
    let inv255 = 1.0f32 / 255.0;
    for i in 0..n {
        let ys = y_new[i];
        let cc1 = cb_full[i] as f32 - 156.0;
        let cc2 = cr_full[i] as f32 - 137.0;
        let r_f = (ys + 1.402 * cc2).clamp(0.0, 255.0);
        let g_f = (ys - 0.34414 * cc1 - 0.71414 * cc2).clamp(0.0, 255.0);
        let b_f = (ys + 1.772 * cc1).clamp(0.0, 255.0);
        let r = (r_f * inv255).powf(GAMMA_FWD) * 255.0;
        let g = (g_f * inv255).powf(GAMMA_FWD) * 255.0;
        let b = (b_f * inv255).powf(GAMMA_FWD) * 255.0;
        out[i * 3] = trunc_u8(r);
        out[i * 3 + 1] = trunc_u8(g);
        out[i * 3 + 2] = trunc_u8(b);
    }
    out
}

/// Decode 4Base (1536×1024) from pack bytes + Base RGB (768×512).
pub fn decode_4base(pack: &[u8], base_rgb: &[u8]) -> Result<Vec<u8>, HiresError> {
    let ipa = read_ipa_byte(pack);
    let class = huffman_class(ipa);
    if !(1..=4).contains(&class) {
        return Err(HiresError::BadClass(class));
    }
    let table = HuffmanTable::for_class(class);

    let total_rows = FOURBASE_H + FOURBASE_CH * 2;
    let lpt = read_lpt(pack, FOURBASE_LPT_SECTOR, 1, total_rows);

    let icd_start = FOURBASE_ICD_SECTOR * SECTOR;
    let stop_sector = find_sixteenbase_start(pack);
    let icd_end = (stop_sector * SECTOR).min(pack.len());
    if icd_end <= icd_start {
        return Err(HiresError::PackTooShort {
            need: icd_start + 1,
            have: pack.len(),
        });
    }
    let icd = &pack[icd_start..icd_end];

    let base_up = bilinear_upsample_rgb_2x(base_rgb, 768, 512);

    let y_res = decode_plane(icd, &lpt, &table, FOURBASE_H, FOURBASE_W, 0);
    let cb_res = decode_plane(icd, &lpt, &table, FOURBASE_CH, FOURBASE_CW, FOURBASE_H);
    let cr_res = decode_plane(
        icd,
        &lpt,
        &table,
        FOURBASE_CH,
        FOURBASE_CW,
        FOURBASE_H + FOURBASE_CH,
    );

    Ok(apply_residuals(
        &base_up, FOURBASE_W, FOURBASE_H, &y_res, &cb_res, &cr_res,
    ))
}

/// Decode 16Base (3072×2048) from pack bytes + 4Base RGB (1536×1024).
pub fn decode_16base(pack: &[u8], fourbase_rgb: &[u8]) -> Result<Vec<u8>, HiresError> {
    let start = find_sixteenbase_start(pack);

    const SB_ICA_OFF: usize = 0;
    const SB_LPT_OFF: usize = 9;
    const SB_ICD_OFF: usize = 13;

    let ica_off = (start + SB_ICA_OFF) * SECTOR;
    if pack.len() < ica_off + SECTOR {
        return Err(HiresError::PackTooShort {
            need: ica_off + SECTOR,
            have: pack.len(),
        });
    }
    let ica_byte = pack[ica_off];
    let class = ((ica_byte >> 5) & 0x03) + 1;
    if !(1..=4).contains(&class) {
        return Err(HiresError::BadClass(class));
    }
    let table = HuffmanTable::for_class(class);

    let total_rows = SIXTEENBASE_H + SIXTEENBASE_CH * 2;
    let lpt = read_lpt(pack, start + SB_LPT_OFF, 2, total_rows);

    let icd_start = (start + SB_ICD_OFF) * SECTOR;
    if pack.len() <= icd_start {
        return Err(HiresError::PackTooShort {
            need: icd_start + 1,
            have: pack.len(),
        });
    }
    let icd = &pack[icd_start..];

    let up = bilinear_upsample_rgb_2x(fourbase_rgb, FOURBASE_W, FOURBASE_H);

    let y_res = decode_plane(icd, &lpt, &table, SIXTEENBASE_H, SIXTEENBASE_W, 0);
    let cb_res = decode_plane(
        icd,
        &lpt,
        &table,
        SIXTEENBASE_CH,
        SIXTEENBASE_CW,
        SIXTEENBASE_H,
    );
    let cr_res = decode_plane(
        icd,
        &lpt,
        &table,
        SIXTEENBASE_CH,
        SIXTEENBASE_CW,
        SIXTEENBASE_H + SIXTEENBASE_CH,
    );

    Ok(apply_residuals(
        &up,
        SIXTEENBASE_W,
        SIXTEENBASE_H,
        &y_res,
        &cb_res,
        &cr_res,
    ))
}

#[inline(always)]
fn clamp_idx(i: isize, dim: usize) -> usize {
    if i < 0 {
        0
    } else if (i as usize) >= dim {
        dim - 1
    } else {
        i as usize
    }
}

#[inline(always)]
fn clamp_u8_round(v: f32) -> u8 {
    if v < 0.0 {
        0
    } else if v > 255.0 {
        255
    } else {
        (v + 0.5) as u8
    }
}

#[inline(always)]
fn trunc_u8(v: f32) -> u8 {
    if v < 0.0 {
        0
    } else if v >= 255.0 {
        255
    } else {
        v as u8
    }
}
