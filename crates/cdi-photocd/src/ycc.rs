//! Photo YCC → sRGB conversion.
//!
//! Spec Section IV.2.5 (CCIR 709 primaries). Neutrals:
//!   Cb = 156, Cr = 137 (Photo CD specific; *not* 128).
//! Y is stored in CCIR 601 video luma range (16..235).
//!
//! Decode matrix (matching the Python reference implementation):
//!   Y_s = (Y - 16) * 255 / 209, clamped to [0, 255]
//!   R = Y_s + 1.402   * (Cr - 137)
//!   G = Y_s - 0.34414 * (Cb - 156) - 0.71414 * (Cr - 137)
//!   B = Y_s + 1.772   * (Cb - 156)
//!
//! Followed by a gamma = 0.70 curve ((c/255)^0.70 * 255) to match the
//! viewer's tone reproduction choice.

pub const GAMMA_FWD: f32 = 0.70;
const GAMMA: f32 = GAMMA_FWD;
const Y_SCALE: f32 = 255.0 / 209.0;

/// Convert Photo YCC planar components to interleaved sRGB bytes.
///
/// `y` is `width * height` bytes (luminance).
/// `cb` and `cr` are already upsampled to `width * height` bytes.
/// Output is `width * height * 3` bytes, RGB order.
pub fn ycc_to_rgb(y: &[u8], cb: &[u8], cr: &[u8], width: usize, height: usize, out: &mut [u8]) {
    let n = width * height;
    assert_eq!(y.len(), n, "y plane size mismatch");
    assert_eq!(cb.len(), n, "cb plane size mismatch (expected upsampled)");
    assert_eq!(cr.len(), n, "cr plane size mismatch (expected upsampled)");
    assert_eq!(out.len(), n * 3, "output buffer size mismatch");

    // Precompute C1 (Cb) / C2 (Cr) per-value contributions.
    let mut c1_g = [0.0f32; 256];
    let mut c1_b = [0.0f32; 256];
    let mut c2_r = [0.0f32; 256];
    let mut c2_g = [0.0f32; 256];
    for v in 0..256usize {
        let c1 = (v as f32) - 156.0;
        let c2 = (v as f32) - 137.0;
        c1_g[v] = -0.34414 * c1;
        c1_b[v] = 1.772 * c1;
        c2_r[v] = 1.402 * c2;
        c2_g[v] = -0.71414 * c2;
    }

    // Match the Python reference (numpy):
    //   * clip matrix output to [0,255] (float)
    //   * apply gamma on the float value: (x/255)^0.70 * 255
    //   * clip again, then .astype(np.uint8) (truncation toward zero)
    let inv255 = 1.0f32 / 255.0;

    for i in 0..n {
        let ys_raw = (y[i] as f32 - 16.0) * Y_SCALE;
        let ys = clamp_f(ys_raw, 0.0, 255.0);

        let cb_v = cb[i] as usize;
        let cr_v = cr[i] as usize;

        let r_f = clamp_f(ys + c2_r[cr_v], 0.0, 255.0);
        let g_f = clamp_f(ys + c1_g[cb_v] + c2_g[cr_v], 0.0, 255.0);
        let b_f = clamp_f(ys + c1_b[cb_v], 0.0, 255.0);

        let r = (r_f * inv255).powf(GAMMA) * 255.0;
        let g = (g_f * inv255).powf(GAMMA) * 255.0;
        let b = (b_f * inv255).powf(GAMMA) * 255.0;

        let o = i * 3;
        out[o] = trunc_u8(r);
        out[o + 1] = trunc_u8(g);
        out[o + 2] = trunc_u8(b);
    }
}

#[inline(always)]
fn clamp_f(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

#[inline(always)]
fn trunc_u8(v: f32) -> u8 {
    // numpy .astype(np.uint8) truncates toward zero after clipping.
    if v < 0.0 {
        0
    } else if v >= 255.0 {
        255
    } else {
        v as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_grey_is_grey() {
        // Neutral chroma (Cb=156, Cr=137) with mid luma should yield neutral grey.
        let y = vec![128u8; 4];
        let cb = vec![156u8; 4];
        let cr = vec![137u8; 4];
        let mut out = vec![0u8; 12];
        ycc_to_rgb(&y, &cb, &cr, 2, 2, &mut out);
        // R == G == B for neutral chroma.
        for px in out.chunks_exact(3) {
            assert_eq!(px[0], px[1]);
            assert_eq!(px[1], px[2]);
        }
    }

    #[test]
    fn black_stays_black() {
        let y = vec![16u8; 1];
        let cb = vec![156u8; 1];
        let cr = vec![137u8; 1];
        let mut out = vec![0u8; 3];
        ycc_to_rgb(&y, &cb, &cr, 1, 1, &mut out);
        assert_eq!(out, [0, 0, 0]);
    }
}
