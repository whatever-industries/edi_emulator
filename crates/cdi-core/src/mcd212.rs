// SPDX-License-Identifier: GPL-2.0-or-later
//! MCD212 Video/Display System Controller: register file, frame and line
//! timing (DA/PA status bits), ICA/DCA control-program execution, interrupt
//! generation, and the pixel pipeline (display-file decoding, mattes,
//! plane mixing, hardware cursor).
//!
//! Ported with reference to MAME `src/mame/philips/mcd212.cpp`
//! (BSD-3-Clause, Ryan Holtz) and the MCD212 datasheet — see NOTICE.md.
//! Note: MAME indexes plane RAM with `^1` because it views it as
//! host-endian u16 words; our plane RAM is raw CPU byte order, indexed
//! directly.

use std::sync::OnceLock;

/// CSR1R status bits.
pub const CSR1R_DA: u8 = 0x80; // Display Active
pub const CSR1R_PA: u8 = 0x20; // Parity (odd/even field)

/// CSR2R status bits.
pub const CSR2R_IT1: u8 = 0x04;
pub const CSR2R_IT2: u8 = 0x02;
pub const CSR2R_BE: u8 = 0x01;

const CSR1W_DI1: u16 = 1 << 15;
const CSR2W_DI2: u16 = 1 << 15;

/// DCR bits.
pub const DCR_DE: u16 = 1 << 15; // Display Enable
pub const DCR_CF: u16 = 1 << 14; // Crystal Frequency
pub const DCR_FD: u16 = 1 << 13; // Frame Duration
pub const DCR_SM: u16 = 1 << 12; // Scan Mode
pub const DCR_CM: u16 = 1 << 11; // Color Mode
pub const DCR_ICA: u16 = 1 << 9; // ICA Enable
pub const DCR_DCA: u16 = 1 << 8; // DCA Enable

#[derive(Debug, Clone)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct Mcd212 {
    // Main registers, [path 0 (plane A), path 1 (plane B)].
    pub csrw: [u16; 2],
    pub csrr: [u8; 2],
    pub dcr: [u16; 2],
    pub vsr: [u16; 2],
    pub ddr: [u16; 2],
    pub dcp: [u16; 2],
    pub dca: [u32; 2],

    // Internal display registers (set via ICA/DCA).
    #[cfg_attr(feature = "savestate", serde(with = "serde_arrays"))]
    pub clut: [u32; 256],
    pub clut_bank: [u8; 2],
    pub image_coding_method: u32,
    pub transparency_control: u32,
    pub plane_order: u32,
    pub transparent_color: [u32; 2],
    pub mask_color: [u32; 2],
    pub dyuv_abs_start: [u32; 2],
    pub cursor_position: u32,
    pub cursor_control: u32,
    #[cfg_attr(feature = "savestate", serde(with = "serde_arrays"))]
    pub cursor_pattern: [u16; 16],
    pub matte_control: [u32; 8],
    pub backdrop_color: u8,
    pub mosaic_hold: [u32; 2],
    /// Base weight factors (per plane); expanded per-pixel by the mattes.
    pub weight_factor_base: [u8; 2],

    // Per-pixel matte results, recomputed when matte state changes.
    #[cfg_attr(feature = "savestate", serde(skip, default = "default_weights"))]
    weight_factor: [Vec<u8>; 2],
    #[cfg_attr(feature = "savestate", serde(skip, default = "default_flags"))]
    matte_flag: [Vec<bool>; 2],

    // Cursor blink state
    blink_time: u32,
    blink_active: bool,

    // Timing
    pub pal: bool,
    line: u32,
    line_accum: u64,
    /// INT line to the 68070 INT1 pin.
    int_asserted: bool,
    /// Frames completed (diagnostics).
    pub frame_count: u64,

    /// Output framebuffer, `FB_WIDTH` × `FB_HEIGHT` 0RGB pixels.
    #[cfg_attr(feature = "savestate", serde(skip, default = "default_fb"))]
    framebuffer: Vec<u32>,
}

pub const FB_WIDTH: usize = 768;
pub const FB_HEIGHT: usize = 560;

#[cfg(feature = "savestate")]
fn default_fb() -> Vec<u32> {
    vec![0; FB_WIDTH * FB_HEIGHT]
}

#[cfg(feature = "savestate")]
fn default_weights() -> [Vec<u8>; 2] {
    [vec![0; FB_WIDTH], vec![0; FB_WIDTH]]
}

#[cfg(feature = "savestate")]
fn default_flags() -> [Vec<bool>; 2] {
    [vec![false; FB_WIDTH], vec![false; FB_WIDTH]]
}

/// EBU 4bpp system colors (half- and full-intensity RGB).
const COLOR_4BPP: [u32; 16] = [
    0x0010_1010,
    0x0010_107A,
    0x0010_7A10,
    0x0010_7A7A,
    0x007A_1010,
    0x007A_107A,
    0x007A_7A10,
    0x007A_7A7A,
    0x0010_1010,
    0x0010_10E6,
    0x0010_E610,
    0x0010_E6E6,
    0x00E6_1010,
    0x00E6_10E6,
    0x00E6_E610,
    0x00E6_E6E6,
];

/// Transparency-control condition codes (low 3 bits + invert bit 3).
const TCR_ALWAYS: u8 = 0x0;
const TCR_KEY: u8 = 0x1;
const TCR_RGB: u8 = 0x2;
const TCR_MF0: u8 = 0x3;
const TCR_MF1_KEY1: u8 = 0x6;
const TCR_DISABLE_MX: u32 = 0x80_0000;

/// Image coding methods (per-plane nibble of the ICM register).
const ICM_CLUT8_OR_RGB555: u8 = 1;
const ICM_CLUT7: u8 = 3;
const ICM_CLUT77: u8 = 4;
const ICM_DYUV: u8 = 5;
const ICM_CLUT4: u8 = 11;

/// DDR display file types.
const DDR_FT_MASK: u16 = 0x0300;
const DDR_FT_RLE: u16 = 0x0200;
const DDR_FT_MOSAIC: u16 = 0x0300;

/// Cursor control bits.
const CURCNT_COLOR: u32 = 0x0F;
const CURCNT_CUW: u32 = 0x8000;
const CURCNT_COF_SHIFT: u32 = 16;
const CURCNT_CON_SHIFT: u32 = 19;
const CURCNT_BLKC: u32 = 0x40_0000;
const CURCNT_EN: u32 = 0x80_0000;

/// DYUV lookup tables (pure functions of the datasheet delta table).
struct DyuvLuts {
    delta_y: [u8; 256],
    delta_uv: [u8; 256],
    limit: [u8; 0x300],
    u_to_b: [i16; 256],
    u_to_g: [i16; 256],
    v_to_g: [i16; 256],
    v_to_r: [i16; 256],
}

fn dyuv_luts() -> &'static DyuvLuts {
    static LUTS: OnceLock<DyuvLuts> = OnceLock::new();
    LUTS.get_or_init(|| {
        const DELTAS: [u8; 16] = [
            0, 1, 4, 9, 16, 27, 44, 79, 128, 177, 212, 229, 240, 247, 252, 255,
        ];
        let mut l = DyuvLuts {
            delta_y: [0; 256],
            delta_uv: [0; 256],
            limit: [0; 0x300],
            u_to_b: [0; 256],
            u_to_g: [0; 256],
            v_to_g: [0; 256],
            v_to_r: [0; 256],
        };
        for d in 0..256usize {
            l.delta_y[d] = DELTAS[d & 15];
            l.delta_uv[d] = DELTAS[d >> 4];
        }
        for w in 0..0x300usize {
            l.limit[w] = if w < 0x100 {
                0
            } else if w < 0x200 {
                (w - 0x100) as u8
            } else {
                0xFF
            };
        }
        for sw in 0..256i32 {
            l.u_to_b[sw as usize] = ((444 * (sw - 128)) / 256) as i16;
            l.u_to_g[sw as usize] = (-(86 * (sw - 128)) / 256) as i16;
            l.v_to_g[sw as usize] = (-(179 * (sw - 128)) / 256) as i16;
            l.v_to_r[sw as usize] = ((351 * (sw - 128)) / 256) as i16;
        }
        l
    })
}

/// Lines in the ICA (vertical-blank) region and total lines per frame.
fn geometry(pal: bool) -> (u32, u32) {
    if pal {
        (32, 312)
    } else {
        (22, 262)
    }
}

/// CPU cycles (15 MHz) per display line.
fn cycles_per_line(pal: bool) -> u64 {
    let (fps, (_, total)) = if pal {
        (50, geometry(true))
    } else {
        (60, geometry(false))
    };
    15_000_000 / (fps * u64::from(total))
}

impl Default for Mcd212 {
    fn default() -> Self {
        Self::new(true)
    }
}

impl Mcd212 {
    pub fn new(pal: bool) -> Self {
        Self {
            csrw: [0; 2],
            csrr: [0; 2],
            dcr: [0; 2],
            vsr: [0; 2],
            ddr: [0; 2],
            dcp: [0; 2],
            dca: [0; 2],
            clut: [0; 256],
            clut_bank: [0; 2],
            image_coding_method: 0,
            transparency_control: 0,
            plane_order: 0,
            transparent_color: [0; 2],
            mask_color: [0; 2],
            dyuv_abs_start: [0; 2],
            cursor_position: 0,
            cursor_control: 0,
            cursor_pattern: [0; 16],
            matte_control: [0; 8],
            backdrop_color: 0,
            mosaic_hold: [0; 2],
            weight_factor_base: [0; 2],
            weight_factor: [vec![0; FB_WIDTH], vec![0; FB_WIDTH]],
            matte_flag: [vec![false; FB_WIDTH], vec![false; FB_WIDTH]],
            blink_time: 0,
            blink_active: false,
            pal,
            line: 0,
            line_accum: 0,
            int_asserted: false,
            frame_count: 0,
            framebuffer: vec![0; FB_WIDTH * FB_HEIGHT],
        }
    }

    /// The rendered frame (768×560 0RGB; NTSC uses the top 480 lines).
    pub fn framebuffer(&self) -> &[u32] {
        &self.framebuffer
    }

    /// Visible output size for the current video standard.
    pub fn visible_size(&self) -> (usize, usize) {
        let (ica, total) = geometry(self.pal);
        (FB_WIDTH, ((total - ica) * 2) as usize)
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.pal);
    }

    /// INT1 line state.
    pub fn int_line(&self) -> bool {
        self.int_asserted
    }

    fn get_dcp(&self, path: usize) -> u32 {
        ((u32::from(self.ddr[path]) & 0x3F) << 16) | u32::from(self.dcp[path])
    }

    fn set_dcp(&mut self, path: usize, value: u32) {
        self.dcp[path] = value as u16;
        self.ddr[path] = (self.ddr[path] & 0xFFC0) | ((value >> 16) as u16 & 0x3F);
    }

    fn set_vsr(&mut self, path: usize, value: u32) {
        self.vsr[path] = value as u16;
        self.dcr[path] = (self.dcr[path] & 0xFFC0) | ((value >> 16) as u16 & 0x3F);
    }

    pub fn get_vsr(&self, path: usize) -> u32 {
        ((u32::from(self.dcr[path]) & 0x3F) << 16) | u32::from(self.vsr[path])
    }

    fn set_display_parameters(&mut self, path: usize, value: u8) {
        self.ddr[path] = (self.ddr[path] & 0xF0FF) | (u16::from(value & 0x0F) << 8);
        self.dcr[path] = (self.dcr[path] & 0xF7FF) | (u16::from(value & 0x10) << 7);
    }

    fn raise_it(&mut self, path: usize) {
        self.csrr[1] |= 1 << (2 - path);
        self.refresh_int_line();
    }

    fn refresh_int_line(&mut self) {
        // DI1/DI2 suppress propagation to the shared active-low INT pin; they
        // do not prevent the ICA/DCA engines from recording IT1/IT2 status.
        self.int_asserted = (self.csrr[1] & CSR2R_IT1 != 0 && self.csrw[0] & CSR1W_DI1 == 0)
            || (self.csrr[1] & CSR2R_IT2 != 0 && self.csrw[1] & CSR2W_DI2 == 0);
    }

    fn ica_enabled(&self, path: usize) -> bool {
        self.dcr[0] & DCR_DE != 0 && self.dcr[path] & DCR_ICA != 0
    }

    fn dca_enabled(&self, path: usize) -> bool {
        self.dcr[0] & DCR_DE != 0 && self.dcr[path] & (DCR_ICA | DCR_DCA) == (DCR_ICA | DCR_DCA)
    }

    /// Register write from a control program (register numbers $80-$FF).
    fn set_register(&mut self, path: usize, reg: u8, value: u32) {
        match reg {
            0x80..=0xBF => {
                let index = usize::from(self.clut_bank[path]) * 0x40 + usize::from(reg - 0x80);
                self.clut[index & 0xFF] = value & 0x00FC_FCFC;
            }
            0xC0 => {
                if path == 0 {
                    self.image_coding_method = value;
                }
            }
            0xC1 => {
                if path == 0 {
                    self.transparency_control = value;
                }
            }
            0xC2 => {
                if path == 0 {
                    self.plane_order = value & 7;
                }
            }
            0xC3 => {
                // Plane A owns CLUT banks 0/1 and plane B owns 2/3. The
                // plane-B register exposes only its low bank-select bit.
                self.clut_bank[path] = if path == 0 {
                    (value & 3) as u8
                } else {
                    2 | (value & 1) as u8
                };
            }
            0xC4 => {
                if path == 0 {
                    self.transparent_color[0] = value & 0x00FC_FCFC;
                }
            }
            0xC6 => {
                if path == 1 {
                    self.transparent_color[1] = value & 0x00FC_FCFC;
                }
            }
            0xC7 => {
                if path == 0 {
                    self.mask_color[0] = value & 0x00FC_FCFC;
                }
            }
            0xC9 => {
                if path == 1 {
                    self.mask_color[1] = value & 0x00FC_FCFC;
                }
            }
            0xCA => {
                if path == 0 {
                    self.dyuv_abs_start[0] = value;
                }
            }
            0xCB => {
                if path == 1 {
                    self.dyuv_abs_start[1] = value;
                }
            }
            0xCD => {
                if path == 0 {
                    self.cursor_position = value;
                }
            }
            0xCE => {
                if path == 0 {
                    self.cursor_control = value;
                }
            }
            0xCF => {
                if path == 0 {
                    let y = ((value >> 16) & 0xF) as usize;
                    self.cursor_pattern[y] = value as u16;
                }
            }
            0xD0..=0xD7 => {
                self.matte_control[usize::from(reg - 0xD0)] = value;
                self.update_matte_arrays();
            }
            0xD8 => {
                if path == 0 {
                    self.backdrop_color = (value & 0xF) as u8;
                }
            }
            0xD9 => self.mosaic_hold[0] = value,
            0xDA => self.mosaic_hold[1] = value,
            0xDB => {
                if path == 0 {
                    self.weight_factor_base[0] = value as u8;
                    self.update_matte_arrays();
                }
            }
            0xDC => {
                if path == 1 {
                    self.weight_factor_base[1] = value as u8;
                    self.update_matte_arrays();
                }
            }
            _ => log::trace!("mcd212: unhandled control register {reg:#04x} = {value:#08x}"),
        }
    }

    // --- Pixel pipeline ---------------------------------------------------

    /// Active picture width: 720 unless the 28.5 MHz crystal (CF) is
    /// selected without the 'Standard' override.
    pub fn screen_width(&self) -> usize {
        if self.dcr[0] & DCR_CF == 0 || self.csrw[0] & 0x0002 != 0 {
            720
        } else {
            768
        }
    }

    /// Side border width inside the framebuffer (Standard/720 mode).
    pub fn border_width(&self) -> usize {
        if self.screen_width() == 720 {
            24
        } else {
            0
        }
    }

    fn icm_for(&self, path: usize) -> u8 {
        ((self.image_coding_method >> (path * 8)) & 0xF) as u8
    }

    fn backdrop(&self) -> u32 {
        const ICM_EV: u32 = 0x04_0000;
        if self.image_coding_method & ICM_EV != 0 {
            0 // external video: black without a DVC
        } else {
            COLOR_4BPP[self.backdrop_color as usize]
        }
    }

    /// Recompute the per-pixel weight/matte-flag arrays from the matte
    /// control registers (MCD212 section 5.10).
    fn update_matte_arrays(&mut self) {
        const ICM_NM: u32 = 0x08_0000;
        const MC_X: u32 = 0x0000_03FF;
        const MC_WF_SHIFT: u32 = 10;
        const MC_WF: u32 = 0x00_FC00;
        const MC_MF_BIT: u32 = 16;
        const MC_OP_SHIFT: u32 = 20;

        let width = self.screen_width();
        let num_mattes = if self.image_coding_method & ICM_NM != 0 {
            2
        } else {
            1
        };
        let mut latched_mf = [false; 2];
        let mut latched_wf = [self.weight_factor_base[0], self.weight_factor_base[1]];
        let mut matte_idx = [0usize, 4usize];

        #[allow(clippy::needless_range_loop)]
        for x in 0..width {
            for matte in 0..num_mattes {
                let max_matte_id =
                    if num_mattes == 2 { 4 } else { 8 } + if matte == 1 { 4 } else { 0 };
                if matte_idx[matte] >= max_matte_id {
                    continue;
                }
                let ctrl = self.matte_control[matte_idx[matte]];
                if x as u32 == ctrl & MC_X {
                    let op = (ctrl >> MC_OP_SHIFT) & 0xF;
                    let flag = if num_mattes == 2 {
                        matte
                    } else {
                        ((ctrl >> MC_MF_BIT) & 1) as usize
                    };
                    let wf = ((ctrl & MC_WF) >> MC_WF_SHIFT) as u8;
                    match op {
                        0 => matte_idx[matte] = 8,
                        4 | 6 => latched_wf[((op >> 1) & 1) as usize] = wf,
                        8 | 9 => latched_mf[flag] = op & 1 != 0,
                        12..=15 => {
                            latched_wf[((op >> 1) & 1) as usize] = wf;
                            latched_mf[flag] = op & 1 != 0;
                        }
                        _ => {}
                    }
                    if op != 0 {
                        matte_idx[matte] += 1;
                    }
                }
            }
            self.weight_factor[0][x] = latched_wf[0];
            self.weight_factor[1][x] = latched_wf[1];
            self.matte_flag[0][x] = latched_mf[0];
            self.matte_flag[1][x] = latched_mf[1];
        }
    }

    fn byte_to_clut(&self, path: usize, icm: u8, byte: u8) -> u8 {
        const ICM_CS: u32 = 0x40_0000;
        match icm {
            ICM_CLUT8_OR_RGB555 => byte,
            ICM_CLUT7 => (if path == 1 { 0x80 } else { 0 }) | (byte & 0x7F),
            ICM_CLUT77 if path == 0 => {
                (if self.image_coding_method & ICM_CS != 0 {
                    0x80
                } else {
                    0
                }) | (byte & 0x7F)
            }
            ICM_CLUT4 => (if path == 1 { 0x80 } else { 0 }) | (byte & 0x0F),
            _ => 0,
        }
    }

    /// Decode one line of a plane's display file into pixel + transparency
    /// buffers, advancing the VSR.
    #[allow(clippy::needless_range_loop)]
    fn process_vsr(
        &mut self,
        path: usize,
        plane: &[u8],
        other_plane: &[u8],
        pixels: &mut [u32],
        transparent: &mut [bool],
    ) {
        let luts = dyuv_luts();
        let icm = self.icm_for(path);
        let tp_ctrl = ((self.transparency_control >> (path * 8)) & 0x0F) as u8;
        let width = self.screen_width();

        let mut vsr = self.get_vsr(path);
        let mut vsr2 = self.get_vsr(1 - path);

        if tp_ctrl == TCR_ALWAYS || icm == 0 || vsr == 0 {
            pixels[..width].fill(COLOR_4BPP[0]);
            transparent[..width].fill(tp_ctrl == TCR_ALWAYS);
            return;
        }

        let decoding_mode = self.ddr[path] & DDR_FT_MASK;
        let mosaic_enable = decoding_mode == DDR_FT_MOSAIC;
        let mosaic_factor: u32 = 1 << (((self.ddr[path] & 0x0C00) >> 10) + 1);

        let dyuv_start = self.dyuv_abs_start[path];
        let mut y = ((dyuv_start >> 16) & 0xFF) as u8;
        let mut u = ((dyuv_start >> 8) & 0xFF) as u8;
        let mut v = (dyuv_start & 0xFF) as u8;

        let mask_bits = !self.mask_color[path] & 0x00FC_FCFC;
        let tp_color_match = self.transparent_color[path] & mask_bits;
        let tp_ctrl_type = tp_ctrl & 0x07;
        let use_rgb_tp_bit = tp_ctrl_type == TCR_RGB;
        let tp_check_parity = tp_ctrl & 0x08 == 0;
        let tp_always = tp_ctrl_type == TCR_ALWAYS && tp_check_parity;
        let matte_flag_index = usize::from(!tp_ctrl_type & 1);
        let use_matte_flag = (TCR_MF0..=TCR_MF1_KEY1).contains(&tp_ctrl_type);
        let is_dyuv_rgb = icm == ICM_DYUV || (icm == ICM_CLUT8_OR_RGB555 && path == 1);
        let use_color_key =
            !is_dyuv_rgb && (tp_ctrl_type == TCR_KEY || tp_ctrl_type == 0x5 || tp_ctrl_type == 0x6);

        let fetch =
            |data: &[u8], addr: u32| data[(addr & 0x0007_FFFF) as usize % data.len().max(1)];

        let mut x = 0usize;
        while x < width {
            let byte = fetch(plane, vsr);
            vsr = vsr.wrapping_add(1);
            if icm == ICM_DYUV {
                let byte1 = fetch(plane, vsr);
                vsr = vsr.wrapping_add(1);
                let y2 = y.wrapping_add(luts.delta_y[byte as usize]);
                y = y2.wrapping_add(luts.delta_y[byte1 as usize]);
                u = u.wrapping_add(luts.delta_uv[byte as usize]);
                v = v.wrapping_add(luts.delta_uv[byte1 as usize]);

                let lim = |base: u8, ofs: i16| {
                    luts.limit[(0x100 + i32::from(base) + i32::from(ofs)) as usize]
                };
                let color0 = (u32::from(lim(y2, luts.v_to_r[v as usize])) << 16)
                    | (u32::from(lim(y2, luts.u_to_g[u as usize] + luts.v_to_g[v as usize])) << 8)
                    | u32::from(lim(y2, luts.u_to_b[u as usize]));

                // Half-step interpolation using the next pair's deltas.
                let byte2 = fetch(plane, vsr);
                let byte3 = fetch(plane, vsr.wrapping_add(1));
                let u8n = u.wrapping_add(luts.delta_uv[byte2 as usize]);
                let v8n = v.wrapping_add(luts.delta_uv[byte3 as usize]);
                let u6 = (u >> 1).wrapping_add(u8n >> 1).wrapping_add(u & u8n & 1);
                let v6 = (v >> 1).wrapping_add(v8n >> 1).wrapping_add(v & v8n & 1);
                let color1 = (u32::from(lim(y, luts.v_to_r[v6 as usize])) << 16)
                    | (u32::from(lim(y, luts.u_to_g[u6 as usize] + luts.v_to_g[v6 as usize])) << 8)
                    | u32::from(lim(y, luts.u_to_b[u6 as usize]));

                for (i, c) in [color0, color0, color1, color1].into_iter().enumerate() {
                    if x + i < width {
                        pixels[x + i] = c;
                        transparent[x + i] = tp_always
                            || (use_matte_flag
                                && self.matte_flag[matte_flag_index][x + i] == tp_check_parity);
                    }
                }
                x += 4;
            } else {
                let mut rgb_tp_bit = false;
                let (color0, color1);
                if icm == ICM_CLUT8_OR_RGB555 && path == 1 {
                    // RGB555: plane B supplies low bytes, plane A high bytes.
                    let byte1 = fetch(other_plane, vsr2);
                    vsr2 = vsr2.wrapping_add(1);
                    let blue = u32::from(byte & 0x1F) << 3;
                    let green = (u32::from(byte & 0xE0) >> 2) + (u32::from(byte1 & 0x03) << 6);
                    let red = u32::from(byte1 & 0x7C) << 1;
                    rgb_tp_bit = use_rgb_tp_bit && ((byte1 >> 7 != 0) == tp_check_parity);
                    color0 = (red << 16) | (green << 8) | blue;
                    color1 = color0;
                } else if icm == ICM_CLUT4 {
                    let mask = if decoding_mode == DDR_FT_RLE {
                        0x7
                    } else {
                        0xF
                    };
                    color0 = self.clut[self.byte_to_clut(path, icm, mask & (byte >> 4)) as usize];
                    color1 = self.clut[self.byte_to_clut(path, icm, mask & byte) as usize];
                } else {
                    color0 = self.clut[self.byte_to_clut(path, icm, byte) as usize];
                    color1 = color0;
                }

                let mut length_m: usize = if mosaic_enable {
                    (mosaic_factor * 2) as usize
                } else {
                    2
                };
                if decoding_mode == DDR_FT_RLE {
                    let length = if byte & 0x80 != 0 {
                        let l = fetch(plane, vsr);
                        vsr = vsr.wrapping_add(1);
                        u32::from(l)
                    } else {
                        1
                    };
                    length_m = if length != 0 {
                        (length * 2) as usize
                    } else {
                        width
                    };
                }

                let color_match0 = ((mask_bits & color0) == tp_color_match) == tp_check_parity;
                let color_match1 = ((mask_bits & color1) == tp_color_match) == tp_check_parity;
                let end = width.min(x + length_m);
                let mut i = x;
                while i < end {
                    pixels[i] = color0;
                    transparent[i] = tp_always
                        || rgb_tp_bit
                        || (use_color_key && color_match0)
                        || (use_matte_flag
                            && self.matte_flag[matte_flag_index][i] == tp_check_parity);
                    if i + 1 < end {
                        pixels[i + 1] = color1;
                        transparent[i + 1] = tp_always
                            || rgb_tp_bit
                            || (use_color_key && color_match1)
                            || (use_matte_flag
                                && self.matte_flag[matte_flag_index][i + 1] == tp_check_parity);
                    }
                    i += 2;
                }
                x = end;
            }
        }
        self.set_vsr(path, vsr);
        self.set_vsr(1 - path, vsr2);
    }

    /// Mix the two decoded plane lines into an output line.
    #[allow(clippy::too_many_arguments)]
    fn mix_line(
        &self,
        plane_a: &[u32],
        transparent_a: &[bool],
        plane_b: &[u32],
        transparent_b: &[bool],
        out: &mut [u32],
    ) {
        let width = self.screen_width();
        let border = self.border_width();
        let order_ab = self.plane_order & 1 == 0;
        let mosaic_a = self.mosaic_hold[0] & 0x80_0000 != 0;
        let mosaic_b = self.mosaic_hold[1] & 0x80_0000 != 0;
        let mut mosaic_count_a = ((self.mosaic_hold[0] & 0xFF) << 1) as usize;
        let mut mosaic_count_b = ((self.mosaic_hold[1] & 0xFF) << 1) as usize;
        if self.icm_for(0) == ICM_CLUT4 {
            mosaic_count_a >>= 1;
        }
        if self.icm_for(1) == ICM_CLUT4 {
            mosaic_count_b >>= 1;
        }

        for px in out[..border].iter_mut() {
            *px = COLOR_4BPP[0];
        }
        let out_line = &mut out[border..];
        for x in 0..width {
            if transparent_a[x] && transparent_b[x] {
                out_line[x] = self.backdrop();
                continue;
            }
            let mut a = if mosaic_a && mosaic_count_a != 0 {
                plane_a[x - (x % mosaic_count_a)]
            } else {
                plane_a[x]
            };
            let mut b = if mosaic_b && mosaic_count_b != 0 {
                plane_b[x - (x % mosaic_count_b)]
            } else {
                plane_b[x]
            };

            if transparent_a[x] {
                a = 0;
            } else if order_ab && self.transparency_control & TCR_DISABLE_MX != 0 {
                b = 0;
            }
            if transparent_b[x] {
                b = 0;
            } else if !order_ab && self.transparency_control & TCR_DISABLE_MX != 0 {
                a = 0;
            }

            let weigh = |c: u32, w: u8| -> (i32, i32, i32) {
                let f = |v: i32| ((v - 16).clamp(0, 255) * i32::from(w)) >> 6;
                (
                    f((c >> 16) as i32 & 0xFF).clamp(0, 255),
                    f((c >> 8) as i32 & 0xFF).clamp(0, 255),
                    f(c as i32 & 0xFF).clamp(0, 255),
                )
            };
            let (ar, ag, ab) = weigh(a, self.weight_factor[0][x]);
            let (br, bg, bb) = weigh(b, self.weight_factor[1][x]);
            let r = (ar + br + 16).clamp(0, 255) as u32;
            let g = (ag + bg + 16).clamp(0, 255) as u32;
            let bl = (ab + bb + 16).clamp(0, 255) as u32;
            out_line[x] = (r << 16) | (g << 8) | bl;
        }
        for px in out[border + width..FB_WIDTH.min(border + width + border)].iter_mut() {
            *px = COLOR_4BPP[0];
        }
    }

    fn draw_cursor(&self, out: &mut [u32], scanline: u32) {
        if self.cursor_control & CURCNT_EN == 0 {
            return;
        }
        let mut color_index = (self.cursor_control & CURCNT_COLOR) as usize;
        if self.blink_active {
            if self.cursor_control & CURCNT_BLKC == 0 {
                return;
            }
            color_index ^= 0x7;
        }
        let (ica_height, _) = geometry(self.pal);
        let cursor_x = (self.cursor_position & 0x3FF) as usize;
        let cursor_y = ((self.cursor_position >> 12) & 0x3FF) + ica_height;
        let y = scanline as i64 - i64::from(cursor_y);
        if !(0..16).contains(&y) {
            return;
        }
        let color = COLOR_4BPP[color_index];
        let resolution: usize = if self.cursor_control & CURCNT_CUW != 0 {
            1
        } else {
            2
        };
        let width = self.screen_width();
        for x in 0..16usize {
            if self.cursor_pattern[y as usize] & (1 << (15 - x)) != 0 {
                for j in 0..resolution {
                    let index = cursor_x + x * resolution + j;
                    if index < width {
                        out[index] = color;
                    }
                }
            }
        }
    }

    /// Render the current line into the framebuffer (both field rows).
    fn render_line(&mut self, planea: &[u8], planeb: &[u8]) {
        let (ica_height, total_height) = geometry(self.pal);
        let scanline = self.line;
        let row = ((scanline - ica_height) * 2) as usize;
        if row + 1 >= FB_HEIGHT {
            return;
        }

        // PAL 'Standard' mode: 20-line top/bottom borders.
        if self.dcr[0] & DCR_FD == 0
            && self.csrw[0] & 0x0002 != 0
            && (scanline - ica_height < 20 || scanline >= total_height - 20)
        {
            let start = row * FB_WIDTH;
            self.framebuffer[start..start + FB_WIDTH * 2].fill(COLOR_4BPP[0]);
            return;
        }

        let mut plane_a = [0u32; FB_WIDTH];
        let mut plane_b = [0u32; FB_WIDTH];
        let mut ta = [false; FB_WIDTH];
        let mut tb = [false; FB_WIDTH];
        self.process_vsr(0, planea, planeb, &mut plane_a, &mut ta);
        self.process_vsr(1, planeb, planea, &mut plane_b, &mut tb);

        let mut line = [0u32; FB_WIDTH];
        self.mix_line(&plane_a, &ta, &plane_b, &tb, &mut line);
        self.draw_cursor(&mut line[self.border_width()..], scanline);

        let start = row * FB_WIDTH;
        self.framebuffer[start..start + FB_WIDTH].copy_from_slice(&line);
        self.framebuffer[start + FB_WIDTH..start + FB_WIDTH * 2].copy_from_slice(&line);
    }

    fn plane_word(plane: &[u8], word_addr: u32) -> u32 {
        let i = ((word_addr as usize) * 2) % plane.len().max(2);
        (u32::from(plane[i]) << 8) | u32::from(plane[i + 1])
    }

    /// Run the Image Control Area program for `path`.
    fn process_ica(&mut self, path: usize, plane: &[u8]) {
        let (ica_height, _) = geometry(self.pal);
        let max = ica_height * 120;
        // Start address depends on frame parity.
        let mut addr: u32 = if self.csrr[0] & CSR1R_PA == 0 {
            0x200
        } else {
            0x202
        };
        for _ in 0..max {
            let cmd = (Self::plane_word(plane, addr) << 16) | Self::plane_word(plane, addr + 1);
            addr += 2;
            match cmd >> 24 {
                0x00..=0x0F => return, // STOP
                0x10..=0x1F => {}      // NOP
                0x20..=0x2F => self.set_dcp(path, cmd & 0x003F_FFFC),
                0x30..=0x3F => {
                    self.set_dcp(path, cmd & 0x003F_FFFC);
                    return;
                }
                0x40..=0x4F => addr = (cmd & 0x0007_FFFF) / 2,
                0x50..=0x5F => {
                    self.set_vsr(path, cmd & 0x003F_FFFF);
                    return;
                }
                0x60..=0x6F => self.raise_it(path),
                0x78..=0x7F => self.set_display_parameters(path, (cmd & 0x1F) as u8),
                reg => self.set_register(path, reg as u8, cmd & 0x00FF_FFFF),
            }
        }
    }

    /// Run the Dynamic Control Area program for `path` (one line's worth).
    fn process_dca(&mut self, path: usize, plane: &[u8]) {
        let mut addr = (self.dca[path] & 0x0007_FFFF) / 2;
        let mut count = 0u32;
        // MCD212 table 5-10: the retrace-time fetch budget is 32 bytes when
        // CF is clear and 64 when set. Storage/stride remains 64 bytes in
        // either mode; exceeding the fetch budget performs an automatic stop.
        let fetch_max = if self.dcr[0] & DCR_CF != 0 { 64 } else { 32 };
        loop {
            if count >= fetch_max {
                break;
            }
            let cmd = (Self::plane_word(plane, addr) << 16) | Self::plane_word(plane, addr + 1);
            addr += 2;
            count += 4;
            match cmd >> 24 {
                0x00..=0x0F => break, // STOP
                0x10..=0x1F => {}     // NOP
                0x20..=0x2F => {}     // RELOAD DCP: NOP in DCA
                0x30..=0x3F => {
                    self.set_dcp(path, cmd & 0x003F_FFFC);
                    self.dca[path] = cmd & 0x0007_FFFC;
                    return;
                }
                0x40..=0x4F => self.set_vsr(path, cmd & 0x003F_FFFF),
                0x50..=0x5F => {
                    self.set_vsr(path, cmd & 0x003F_FFFF);
                    break;
                }
                0x60..=0x6F => self.raise_it(path),
                0x78..=0x7F => self.set_display_parameters(path, (cmd & 0x1F) as u8),
                reg => self.set_register(path, reg as u8, cmd & 0x00FF_FFFF),
            }
        }
        addr += (64 - count) / 2;
        self.dca[path] = (addr * 2) & 0x0007_FFFC;
    }

    /// Advance by `cycles` CPU cycles; runs per-line and per-frame work.
    pub fn tick(&mut self, cycles: u64, planea: &[u8], planeb: &[u8]) {
        let (ica_height, total_height) = geometry(self.pal);
        self.line_accum += cycles;
        let per_line = cycles_per_line(self.pal);
        while self.line_accum >= per_line {
            self.line_accum -= per_line;
            self.line += 1;
            if self.line >= total_height {
                self.line = 0;
            }

            if self.line == 0 {
                // Frame start: DA drops and each field-control table runs.
                // Hardware immediately executes the first linked line-control
                // table after that field program, before the first visible
                // line (Philips Technical Note 69, section 3.3).
                self.csrr[0] &= !CSR1R_DA;
                self.frame_count += 1;
                if self.ica_enabled(0) {
                    self.process_ica(0, planea);
                    if self.dca_enabled(0) {
                        self.dca[0] = self.get_dcp(0);
                        self.process_dca(0, planea);
                    }
                }
                if self.ica_enabled(1) {
                    self.process_ica(1, planeb);
                    if self.dca_enabled(1) {
                        self.dca[1] = self.get_dcp(1);
                        self.process_dca(1, planeb);
                    }
                }
                // Cursor blink cadence (MCD212 section 7.5, as modeled by
                // MAME): advance once per frame.
                self.blink_time += 5 + u32::from(self.dcr[0] & DCR_FD != 0);
                let on_time = (self.cursor_control >> CURCNT_CON_SHIFT) & 7;
                let off_time = (self.cursor_control >> CURCNT_COF_SHIFT) & 7;
                if !self.blink_active && self.blink_time >= on_time * 60 {
                    self.blink_active = true;
                    self.blink_time = 0;
                }
                if self.blink_active && self.blink_time >= off_time * 60 {
                    self.blink_active = false;
                    self.blink_time = 0;
                }
            } else if self.line >= ica_height {
                // Active display region.
                self.csrr[0] |= CSR1R_DA;
                if self.dcr[0] & DCR_DE != 0 {
                    self.render_line(planea, planeb);
                }
                // The first DCA slot was fetched after ICA. Fetch the next
                // slot after each visible line except the final one so the
                // frame still consumes exactly one slot per displayed line.
                if self.line + 1 < total_height && self.dca_enabled(0) {
                    self.process_dca(0, planea);
                }
                if self.line + 1 < total_height && self.dca_enabled(1) {
                    self.process_dca(1, planeb);
                }
            }

            if self.line == total_height - 1 {
                self.csrr[0] ^= CSR1R_PA;
            }
        }
    }

    /// Byte read from the register window ($4FFFE0 + offset).
    /// Window layout (MAME map): channel 2 at 0x00-0x0B, channel 1 at
    /// 0x10-0x1B; 16-bit registers carry their high byte at even offsets.
    pub fn read8(&mut self, offset: u32) -> u8 {
        let path2 = 1usize; // offsets 0x00.. are channel 2 (plane B path)
        match offset {
            0x01 => {
                // CSR2R read returns status, then clears IT1, IT2, and BE.
                let data = self.csrr[1];
                self.csrr[1] &= !(CSR2R_IT1 | CSR2R_IT2 | CSR2R_BE);
                self.refresh_int_line();
                data
            }
            0x02 => (self.dcr[path2] >> 8) as u8,
            0x03 => self.dcr[path2] as u8,
            0x04 => (self.vsr[path2] >> 8) as u8,
            0x05 => self.vsr[path2] as u8,
            0x08 => (self.ddr[path2] >> 8) as u8,
            0x09 => self.ddr[path2] as u8,
            0x0A => (self.dca[path2] >> 8) as u8,
            0x0B => self.dca[path2] as u8,
            0x11 => self.csrr[0],
            0x12 => (self.dcr[0] >> 8) as u8,
            0x13 => self.dcr[0] as u8,
            0x14 => (self.vsr[0] >> 8) as u8,
            0x15 => self.vsr[0] as u8,
            0x18 => (self.ddr[0] >> 8) as u8,
            0x19 => self.ddr[0] as u8,
            0x1A => (self.dca[0] >> 8) as u8,
            0x1B => self.dca[0] as u8,
            _ => 0,
        }
    }

    pub fn write8(&mut self, offset: u32, val: u8) {
        let v = u16::from(val);
        match offset {
            0x00 => {
                self.csrw[1] = (self.csrw[1] & 0x00FF) | (v << 8);
                self.refresh_int_line();
            }
            0x01 => self.csrw[1] = (self.csrw[1] & 0xFF00) | v,
            0x02 => self.dcr[1] = (self.dcr[1] & 0x00FF) | (v << 8),
            0x03 => self.dcr[1] = (self.dcr[1] & 0xFF00) | v,
            0x04 => self.vsr[1] = (self.vsr[1] & 0x00FF) | (v << 8),
            0x05 => self.vsr[1] = (self.vsr[1] & 0xFF00) | v,
            0x08 => self.ddr[1] = (self.ddr[1] & 0x00FF) | (v << 8),
            0x09 => self.ddr[1] = (self.ddr[1] & 0xFF00) | v,
            0x0A => self.dca[1] = (self.dca[1] & 0x00FF) | (u32::from(v) << 8),
            0x0B => self.dca[1] = (self.dca[1] & 0xFF00) | u32::from(v),
            0x10 => {
                self.csrw[0] = (self.csrw[0] & 0x00FF) | (v << 8);
                self.refresh_int_line();
            }
            0x11 => self.csrw[0] = (self.csrw[0] & 0xFF00) | v,
            0x12 => self.dcr[0] = (self.dcr[0] & 0x00FF) | (v << 8),
            0x13 => self.dcr[0] = (self.dcr[0] & 0xFF00) | v,
            0x14 => self.vsr[0] = (self.vsr[0] & 0x00FF) | (v << 8),
            0x15 => self.vsr[0] = (self.vsr[0] & 0xFF00) | v,
            0x18 => self.ddr[0] = (self.ddr[0] & 0x00FF) | (v << 8),
            0x19 => self.ddr[0] = (self.ddr[0] & 0xFF00) | v,
            0x1A => self.dca[0] = (self.dca[0] & 0x00FF) | (u32::from(v) << 8),
            0x1B => self.dca[0] = (self.dca[0] & 0xFF00) | u32::from(v),
            _ => log::trace!("mcd212: write8 +{offset:#04x} = {val:#04x} (unhandled)"),
        }
    }
}

#[cfg(feature = "savestate")]
mod serde_arrays {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer, T: Serialize + Copy, const N: usize>(
        arr: &[T; N],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        arr.as_slice().serialize(s)
    }

    pub fn deserialize<'de, D, T, const N: usize>(d: D) -> Result<[T; N], D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de> + Copy + Default,
    {
        let v = Vec::<T>::deserialize(d)?;
        let mut arr = [T::default(); N];
        for (dst, src) in arr.iter_mut().zip(v) {
            *dst = src;
        }
        Ok(arr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_b_clut_register_selects_only_banks_two_and_three() {
        let mut m = Mcd212::new(true);

        m.set_register(1, 0xC3, 0);
        assert_eq!(m.clut_bank[1], 2);
        m.set_register(1, 0x80, 0x0012_3456);
        assert_eq!(m.clut[0x80], 0x0010_3454);

        m.set_register(1, 0xC3, 3);
        assert_eq!(m.clut_bank[1], 3);
        m.set_register(1, 0x80, 0x00AB_CDEF);
        assert_eq!(m.clut[0xC0], 0x00A8_CCEC);
    }

    #[test]
    fn dca_fetch_budget_depends_on_clock_factor_but_stride_is_64_bytes() {
        let mut plane = vec![0u8; 0x80000];
        for instruction in 0..8 {
            plane[instruction * 4] = 0x10; // NOP: first 32 bytes
        }
        plane[32..36].copy_from_slice(&[0xD8, 0, 0, 5]); // backdrop = 5

        let mut normal = Mcd212::new(true);
        normal.process_dca(0, &plane);
        assert_eq!(
            normal.backdrop_color, 0,
            "byte 32 is beyond the CF=0 budget"
        );
        assert_eq!(normal.dca[0], 64, "DCA storage always has a 64-byte stride");

        let mut double_clock = Mcd212::new(true);
        double_clock.dcr[0] |= DCR_CF;
        double_clock.process_dca(0, &plane);
        assert_eq!(double_clock.backdrop_color, 5);
        assert_eq!(double_clock.dca[0], 64);
    }

    #[test]
    fn dca_requires_display_ic_and_dc_control_bits() {
        let mut m = Mcd212::new(true);
        let mut plane = vec![0u8; 0x80000];
        plane[..4].copy_from_slice(&[0xD8, 0, 0, 5]); // backdrop = 5

        m.dcr[0] = DCR_ICA | DCR_DCA;
        m.tick(cycles_per_line(true) * 32, &plane, &plane);
        assert_eq!(m.backdrop_color, 0);

        m.dcr[0] |= DCR_DE;
        m.tick(cycles_per_line(true), &plane, &plane);
        assert_eq!(m.backdrop_color, 5);
    }

    #[test]
    fn first_dca_slot_runs_after_ica_and_slot_count_matches_visible_lines() {
        let mut m = Mcd212::new(true);
        let mut plane = vec![0u8; 0x80000];
        plane[..8].copy_from_slice(&[0xD8, 0, 0, 5, 0, 0, 0, 0]);
        m.dcr[0] = DCR_DE | DCR_ICA | DCR_DCA;

        // Reaching the next frame start executes ICA and its first linked DCA
        // slot before any active line is rendered.
        m.tick(cycles_per_line(true) * 312, &plane, &plane);
        assert_eq!(m.backdrop_color, 5);
        assert_eq!(m.dca[0], 64);

        // PAL has 280 visible lines. The prefetch plus 279 post-line fetches
        // consume exactly 280 64-byte slots, not an extra slot at frame end.
        m.tick(cycles_per_line(true) * 311, &plane, &plane);
        assert_eq!(m.dca[0], 280 * 64);
    }

    #[test]
    fn da_bit_toggles_over_frame() {
        let mut m = Mcd212::new(true);
        let plane = vec![0u8; 0x80000];
        // Run half a frame: DA must be set during active lines.
        let per_line = cycles_per_line(true);
        m.tick(per_line * 100, &plane, &plane);
        assert_ne!(m.csrr[0] & CSR1R_DA, 0, "DA set in active region");
        // Cross the frame boundary: DA clears at frame start.
        let mut cleared = false;
        for _ in 0..320 {
            m.tick(per_line, &plane, &plane);
            if m.csrr[0] & CSR1R_DA == 0 {
                cleared = true;
                break;
            }
        }
        assert!(cleared, "DA must clear during vertical blanking");
    }

    #[test]
    fn parity_toggles_each_frame() {
        let mut m = Mcd212::new(true);
        let plane = vec![0u8; 0x80000];
        let pa0 = m.csrr[0] & CSR1R_PA;
        m.tick(cycles_per_line(true) * 312, &plane, &plane);
        assert_ne!(m.csrr[0] & CSR1R_PA, pa0);
    }

    #[test]
    fn ica_interrupt_and_csr2_clear() {
        let mut m = Mcd212::new(true);
        let mut plane = vec![0u8; 0x80000];
        // ICA start alternates between words 0x200/0x202 with field parity;
        // cover both: INTERRUPT, INTERRUPT, STOP, STOP.
        plane[0x400..0x410]
            .copy_from_slice(&[0x60, 0, 0, 0, 0x60, 0, 0, 0, 0x00, 0, 0, 0, 0x00, 0, 0, 0]);
        m.dcr[0] |= DCR_DE | DCR_ICA;
        // Advance a full frame so line 0 processing happens.
        m.tick(cycles_per_line(true) * 313, &plane, &plane);
        assert!(m.int_line(), "ICA INTERRUPT must assert INT");
        assert_ne!(m.csrr[1] & CSR2R_IT1, 0);
        // Reading CSR2 clears the flags and drops the line.
        let v = m.read8(0x01);
        assert_ne!(v & CSR2R_IT1, 0);
        assert!(!m.int_line());
        assert_eq!(m.csrr[1] & (CSR2R_IT1 | CSR2R_IT2), 0);
    }

    #[test]
    fn disable_interrupt_bits_gate_the_pin_but_not_status() {
        let mut m = Mcd212::new(true);

        m.write8(0x10, 0x80); // DI1
        m.raise_it(0);
        assert_ne!(m.csrr[1] & CSR2R_IT1, 0);
        assert!(!m.int_line(), "DI1 must suppress IT1 propagation");

        // INT is a combinational function of pending status and the disable
        // bits, so clearing DI1 exposes the still-pending IT1 condition.
        m.write8(0x10, 0x00);
        assert!(m.int_line());
        m.write8(0x10, 0x80);
        assert!(!m.int_line());

        m.raise_it(1);
        assert_ne!(m.csrr[1] & CSR2R_IT2, 0);
        assert!(m.int_line(), "DI1 must not suppress channel 2");
        m.write8(0x00, 0x80); // DI2
        assert!(!m.int_line());

        let status = m.read8(0x01);
        assert_eq!(status & (CSR2R_IT1 | CSR2R_IT2), CSR2R_IT1 | CSR2R_IT2);
        assert_eq!(m.csrr[1] & (CSR2R_IT1 | CSR2R_IT2), 0);
    }

    #[test]
    fn csr2_read_clears_bus_error_status() {
        let mut m = Mcd212::new(true);
        m.csrr[1] |= CSR2R_BE;
        assert_ne!(m.read8(0x01) & CSR2R_BE, 0);
        assert_eq!(m.csrr[1] & CSR2R_BE, 0);
    }

    #[test]
    fn ica_reload_display_parameters() {
        let mut m = Mcd212::new(true);
        let mut plane = vec![0u8; 0x80000];
        // RELOAD DISPLAY PARAMETERS (0x78) value 0x1F at both field-parity
        // start addresses, then STOPs.
        plane[0x400..0x410].copy_from_slice(&[
            0x78, 0, 0, 0x1F, 0x78, 0, 0, 0x1F, 0x00, 0, 0, 0, 0x00, 0, 0, 0,
        ]);
        m.dcr[0] |= DCR_DE | DCR_ICA;
        m.tick(cycles_per_line(true) * 313, &plane, &plane);
        assert_eq!(m.ddr[0] & 0x0F00, 0x0F00);
        assert_ne!(m.dcr[0] & DCR_CM, 0);
    }
}
