//! Base (768×512) image plane decoder.
//!
//! Layout of the 288-sector (589,824-byte) Base data area (confirmed by
//! autocorrelation analysis of real discs — see Python reference):
//!
//! ```text
//!   256 groups of three 768-byte rows:
//!     [Y row 0] [Y row 1] [CbCr row]   ← first group
//!     [Y row 2] [Y row 3] [CbCr row]   ← second group
//!     ...
//! ```
//!
//! Each CbCr row is planar within the row: the first 384 bytes are Cb,
//! the following 384 bytes are Cr. The chroma planes are 384 × 256, i.e.
//! 4:2:0 subsampled from the 768 × 512 luma plane.

use crate::ycc;

pub const BASE_W: usize = 768;
pub const BASE_H: usize = 512;
pub const CHROMA_W: usize = BASE_W / 2; // 384
pub const CHROMA_H: usize = BASE_H / 2; // 256
pub const BASE_RAW_LEN: usize = 589_824; // 288 sectors × 2048

/// Error type for base-plane decoding.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("expected {expected} bytes of Base data, got {actual}")]
    WrongLength { expected: usize, actual: usize },
}

/// Decode the raw 589,824-byte Base data stream to a 768×512 RGB image.
///
/// Returns `(width, height, rgb_bytes)` with `rgb_bytes` being
/// `768 * 512 * 3` bytes (interleaved R, G, B).
pub fn decode_base_plane(raw: &[u8]) -> Result<Vec<u8>, DecodeError> {
    if raw.len() < BASE_RAW_LEN {
        return Err(DecodeError::WrongLength {
            expected: BASE_RAW_LEN,
            actual: raw.len(),
        });
    }
    let raw = &raw[..BASE_RAW_LEN];

    // Separate Y and CbCr rows.
    let mut y_plane: Vec<u8> = Vec::with_capacity(BASE_W * BASE_H);
    let mut cb_plane: Vec<u8> = Vec::with_capacity(CHROMA_W * CHROMA_H);
    let mut cr_plane: Vec<u8> = Vec::with_capacity(CHROMA_W * CHROMA_H);

    let mut pos = 0usize;
    for _ in 0..CHROMA_H {
        // Two Y rows.
        y_plane.extend_from_slice(&raw[pos..pos + BASE_W]);
        pos += BASE_W;
        y_plane.extend_from_slice(&raw[pos..pos + BASE_W]);
        pos += BASE_W;
        // One CbCr row: first half Cb, second half Cr.
        cb_plane.extend_from_slice(&raw[pos..pos + CHROMA_W]);
        cr_plane.extend_from_slice(&raw[pos + CHROMA_W..pos + BASE_W]);
        pos += BASE_W;
    }
    debug_assert_eq!(pos, BASE_RAW_LEN);
    debug_assert_eq!(y_plane.len(), BASE_W * BASE_H);
    debug_assert_eq!(cb_plane.len(), CHROMA_W * CHROMA_H);
    debug_assert_eq!(cr_plane.len(), CHROMA_W * CHROMA_H);

    // Upsample chroma 2× in both axes with bilinear interpolation,
    // matching PIL.Image.resize(..., Image.BILINEAR).
    let cb_up = bilinear_upsample_2x(&cb_plane, CHROMA_W, CHROMA_H);
    let cr_up = bilinear_upsample_2x(&cr_plane, CHROMA_W, CHROMA_H);

    let mut rgb = vec![0u8; BASE_W * BASE_H * 3];
    ycc::ycc_to_rgb(&y_plane, &cb_up, &cr_up, BASE_W, BASE_H, &mut rgb);
    Ok(rgb)
}

/// Bilinear 2× upsample of a `w × h` u8 plane → `(2w) × (2h)`.
///
/// Follows PIL's `Image.resize(..., BILINEAR)` convention: output pixel
/// centers map back to input via `src = (dst + 0.5) * (src_dim / dst_dim) - 0.5`.
fn bilinear_upsample_2x(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let out_w = w * 2;
    let out_h = h * 2;
    let mut out = vec![0u8; out_w * out_h];

    for oy in 0..out_h {
        // Source y: (oy + 0.5) * (h / out_h) - 0.5 = (oy + 0.5) * 0.5 - 0.5
        //         = (oy - 0.5) * 0.5 = oy * 0.5 - 0.25
        let sy = (oy as f32) * 0.5 - 0.25;
        let y0f = sy.floor();
        let y0 = y0f as isize;
        let y1 = y0 + 1;
        let fy = sy - y0f;
        let y0c = clamp_idx(y0, h);
        let y1c = clamp_idx(y1, h);

        for ox in 0..out_w {
            let sx = (ox as f32) * 0.5 - 0.25;
            let x0f = sx.floor();
            let x0 = x0f as isize;
            let x1 = x0 + 1;
            let fx = sx - x0f;
            let x0c = clamp_idx(x0, w);
            let x1c = clamp_idx(x1, w);

            let p00 = src[y0c * w + x0c] as f32;
            let p01 = src[y0c * w + x1c] as f32;
            let p10 = src[y1c * w + x0c] as f32;
            let p11 = src[y1c * w + x1c] as f32;

            let top = p00 + (p01 - p00) * fx;
            let bot = p10 + (p11 - p10) * fx;
            let v = top + (bot - top) * fy;

            out[oy * out_w + ox] = if v < 0.0 {
                0
            } else if v > 255.0 {
                255
            } else {
                (v + 0.5) as u8
            };
        }
    }

    out
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
