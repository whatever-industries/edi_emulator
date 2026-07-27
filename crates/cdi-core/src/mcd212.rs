// SPDX-License-Identifier: GPL-3.0-or-later
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

use crate::dvc::ExternalVideo;

/// CSR1R status bits.
pub const CSR1R_DA: u8 = 0x80; // Display Active
pub const CSR1R_PA: u8 = 0x20; // Parity (odd/even field)

/// CSR2R status bits.
pub const CSR2R_IT1: u8 = 0x04;
pub const CSR2R_IT2: u8 = 0x02;
pub const CSR2R_BE: u8 = 0x01;

const CSR1W_DI1: u16 = 1 << 15;
const CSR1W_ST: u16 = 1 << 1;
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

    /// Output framebuffer, `FB_WIDTH` × `FB_HEIGHT` 0RGB pixels. The hardware
    /// cursor is composited here after both display fields have been woven so
    /// an animated cursor does not contain pixels from two different moments.
    #[cfg_attr(feature = "savestate", serde(skip, default = "default_fb"))]
    framebuffer: Vec<u32>,
    /// Woven base-video fields before the hardware cursor is overlaid.
    #[cfg_attr(feature = "savestate", serde(skip, default = "default_fb"))]
    field_framebuffer: Vec<u32>,
    /// Optional woven output of each decoded plane before transparency,
    /// mattes, plane ordering, and cursor composition. This is allocated only
    /// for a requested diagnostic capture so normal emulation pays no large
    /// memory cost.
    #[cfg_attr(feature = "savestate", serde(skip))]
    diagnostic_plane_framebuffer: [Option<Vec<u32>>; 2],
}

pub const FB_WIDTH: usize = 768;
pub const FB_HEIGHT: usize = 560;

/// Hardware-defined picture aperture inside the fixed host framebuffer.
///
/// The dimensions follow MCD212 tables 5-1 through 5-7 and section 5.8:
/// `CF`/`ST` select the 720/768-pixel active line, while 625-line
/// Compatibility Mode centers 240 source lines inside the 280-line raster.
/// Scan mode and field parity do not change the aperture, but are included so
/// diagnostics can describe the complete live display state without
/// reinterpreting register bits outside the core. Pixel aspect is expressed
/// as height/width from Philips TSA-003 (TN 093): 1.025 on Philips 625-line
/// players and 1.225 on Philips 525-line players.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct DisplayGeometry {
    pub raster_width: usize,
    pub raster_height: usize,
    pub active_x: usize,
    pub active_y: usize,
    pub active_width: usize,
    pub active_height: usize,
    pub compatibility_mode: bool,
    pub interlaced: bool,
    pub odd_field: bool,
    pub frame_duration_60hz: bool,
    pub pixel_aspect_num: u16,
    pub pixel_aspect_den: u16,
}

/// Expand one CD-i/CCIR-601 internal RGB pixel for presentation on a desktop
/// display. The MCD212 performs its pixel pipeline with nominal black at 16
/// and white at 235; keep that range internally so mattes and external-video
/// mixing remain hardware-faithful, and expand only at the output boundary.
pub fn presentation_rgb(pixel: u32) -> u32 {
    fn component(value: u32) -> u32 {
        let studio = value.clamp(16, 235) - 16;
        (studio * 255 + 109) / 219
    }

    (component((pixel >> 16) & 0xFF) << 16)
        | (component((pixel >> 8) & 0xFF) << 8)
        | component(pixel & 0xFF)
}

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
const CURSOR_BLINK_FIELDS_PER_UNIT: u32 = 12;

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
            field_framebuffer: vec![0; FB_WIDTH * FB_HEIGHT],
            diagnostic_plane_framebuffer: [None, None],
        }
    }

    /// The rendered frame (768×560 0RGB; NTSC uses the top 480 lines).
    pub fn framebuffer(&self) -> &[u32] {
        &self.framebuffer
    }

    /// Base raster after both fields are woven but before the live hardware
    /// cursor is overlaid.
    pub fn diagnostic_base_framebuffer(&self) -> &[u32] {
        &self.field_framebuffer
    }

    /// Enable or disable decoded-plane capture. Enabling allocates two full
    /// diagnostic rasters; disabling releases them immediately.
    pub fn set_diagnostic_plane_capture(&mut self, enabled: bool) {
        if enabled {
            for plane in &mut self.diagnostic_plane_framebuffer {
                if plane.is_none() {
                    *plane = Some(vec![0; FB_WIDTH * FB_HEIGHT]);
                }
            }
        } else {
            self.diagnostic_plane_framebuffer = [None, None];
        }
    }

    /// Return a decoded plane before MCD212 composition when capture is
    /// enabled.
    pub fn diagnostic_plane_framebuffer(&self, path: usize) -> Option<&[u32]> {
        self.diagnostic_plane_framebuffer
            .get(path)
            .and_then(Option::as_deref)
    }

    /// Visible output size for the current video standard.
    pub fn visible_size(&self) -> (usize, usize) {
        let geometry = self.display_geometry();
        (geometry.raster_width, geometry.raster_height)
    }

    /// Resolve the live MCD212 aperture from hardware state only.
    ///
    /// No title, disc-profile, or framebuffer-content information enters
    /// this decision. Green Book V.4.8 defines Compatibility Mode offsets in
    /// normal-resolution pixels; this framebuffer is double resolution in
    /// both axes, hence the 24/40 host-pixel offsets.
    pub fn display_geometry(&self) -> DisplayGeometry {
        let compatibility_mode = self.csrw[0] & CSR1W_ST != 0;
        let frame_duration_60hz = self.dcr[0] & DCR_FD != 0;
        let active_width = if self.dcr[0] & DCR_CF != 0 && !compatibility_mode {
            768
        } else {
            720
        };
        let raster_height = if self.pal { 560 } else { 480 };
        let vertical_compatibility = self.pal && compatibility_mode && !frame_duration_60hz;
        let active_height = if vertical_compatibility {
            480
        } else {
            raster_height
        };
        DisplayGeometry {
            raster_width: FB_WIDTH,
            raster_height,
            active_x: (FB_WIDTH - active_width) / 2,
            active_y: (raster_height - active_height) / 2,
            active_width,
            active_height,
            compatibility_mode,
            interlaced: self.dcr[0] & DCR_SM != 0,
            odd_field: self.csrr[0] & CSR1R_PA != 0,
            frame_duration_60hz,
            // Philips TSA-003 measured 384 samples in 51.2 us for PAL and
            // 50.84 us for NTSC. Its resulting pixel-height/width ratios are
            // 1.025 and 1.225 respectively.
            pixel_aspect_num: if self.pal { 41 } else { 49 },
            pixel_aspect_den: 40,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.pal);
    }

    /// INT1 line state.
    pub fn int_line(&self) -> bool {
        self.int_asserted
    }

    pub fn get_dcp(&self, path: usize) -> u32 {
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

    fn set_display_parameters(&mut self, path: usize, value: u32) {
        // Green Book V.4.6.1 Figure V.49 and Philips' cp_dprm macro place
        // BP at bits 9:8, PRF at 3:2, and RMS at 1:0.  The MCD212 exposes
        // the base-case BP_DOUBLE selection as DCR.CM; BP_HIGH is an
        // extended-case 8-bit mode which this Mono-I device does not provide.
        self.ddr[path] = (self.ddr[path] & 0xF0FF) | ((value as u16 & 0x0F) << 8);
        let color_mode = if (value >> 8) & 0x03 == 1 { DCR_CM } else { 0 };
        self.dcr[path] = (self.dcr[path] & !DCR_CM) | color_mode;
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
        self.display_geometry().active_width
    }

    /// Side border width inside the framebuffer (Standard/720 mode).
    pub fn border_width(&self) -> usize {
        self.display_geometry().active_x
    }

    /// Top border inside the progressive host framebuffer. The MCD212's
    /// 50 Hz Standard mode exposes 240 active lines in the 280-line PAL
    /// timing aperture (datasheet tables 5-6 and 5-7). Each source line is
    /// represented by two host rows, hence a 40-row margin at either end.
    pub fn top_border(&self) -> usize {
        self.display_geometry().active_y
    }

    /// Active picture height inside the framebuffer, excluding the timing
    /// aperture that a television does not present as picture area.
    pub fn screen_height(&self) -> usize {
        self.display_geometry().active_height
    }

    fn icm_for(&self, path: usize) -> u8 {
        ((self.image_coding_method >> (path * 8)) & 0xF) as u8
    }

    fn backdrop(&self, external: Option<u32>) -> u32 {
        const ICM_EV: u32 = 0x04_0000;
        if self.image_coding_method & ICM_EV != 0 {
            external.unwrap_or(0)
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
        active_line: usize,
        external: Option<ExternalVideo<'_>>,
    ) {
        const ICM_EV: u32 = 0x04_0000;
        let width = self.screen_width();
        let border = self.border_width();
        let external_selected = self.image_coding_method & ICM_EV != 0;
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
            let external_color = external.map(|video| video.pixel(x + border, active_line));
            let backdrop = self.backdrop(external_color);
            if transparent_a[x] && transparent_b[x] {
                out_line[x] = backdrop;
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

            let mut weight_a = self.weight_factor[0][x];
            let mut weight_b = self.weight_factor[1][x];
            if transparent_a[x] {
                a = 0;
                weight_a = 0;
            } else if order_ab && self.transparency_control & TCR_DISABLE_MX != 0 {
                b = 0;
                weight_b = 0;
            }
            if transparent_b[x] {
                b = 0;
                weight_b = 0;
            } else if !order_ab && self.transparency_control & TCR_DISABLE_MX != 0 {
                a = 0;
                weight_a = 0;
            }

            let weigh = |c: u32, w: u8| -> (i32, i32, i32) {
                let f = |v: i32| ((v - 16).clamp(0, 255) * i32::from(w)) >> 6;
                (
                    f((c >> 16) as i32 & 0xFF).clamp(0, 255),
                    f((c >> 8) as i32 & 0xFF).clamp(0, 255),
                    f(c as i32 & 0xFF).clamp(0, 255),
                )
            };
            let (ar, ag, ab) = weigh(a, weight_a);
            let (br, bg, bb) = weigh(b, weight_b);
            let external_weight = if external_selected {
                64u8.saturating_sub(weight_a.saturating_add(weight_b))
            } else {
                0
            };
            let (er, eg, eb) = weigh(backdrop, external_weight);
            let r = (ar + br + er + 16).clamp(0, 255) as u32;
            let g = (ag + bg + eg + 16).clamp(0, 255) as u32;
            let bl = (ab + bb + eb + 16).clamp(0, 255) as u32;
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

    /// Publish the completed field pair and draw the current hardware cursor
    /// over both host rows. Base video remains field-woven, but the cursor is
    /// a live MCD212 overlay; leaving it baked into alternating fields makes
    /// animated cursor patterns visibly comb between their old and new shapes.
    fn compose_framebuffer(&mut self) {
        self.framebuffer.copy_from_slice(&self.field_framebuffer);
        if self.cursor_control & CURCNT_EN == 0 {
            return;
        }

        let (ica_height, total_height) = geometry(self.pal);
        let cursor_y = ((self.cursor_position >> 12) & 0x3FF) + ica_height;
        let border = self.border_width();
        let mut line = [0u32; FB_WIDTH];
        for pattern_y in 0..16u32 {
            let scanline = cursor_y + pattern_y;
            if !(ica_height..total_height).contains(&scanline) {
                continue;
            }
            let first_row = ((scanline - ica_height) as usize) * 2;
            for row in [first_row, first_row + 1] {
                let start = row * FB_WIDTH;
                line.copy_from_slice(&self.framebuffer[start..start + FB_WIDTH]);
                self.draw_cursor(&mut line[border..], scanline);
                self.framebuffer[start..start + FB_WIDTH].copy_from_slice(&line);
            }
        }
    }

    /// Return the destination row(s) for one active source line. In
    /// non-interlaced mode one field is a complete picture and is doubled for
    /// the progressive host framebuffer. In interlaced mode consecutive
    /// fields supply alternating rows and the other field must be retained.
    fn output_rows(&self, active_line: usize) -> (usize, Option<usize>) {
        let first = active_line * 2;
        if self.dcr[0] & DCR_SM != 0 {
            // PA=1 is the odd field (lines 1,3,5...), which maps to zero-based
            // even rows. PA=0 is the even field (lines 2,4,6...).
            (first + usize::from(self.csrr[0] & CSR1R_PA == 0), None)
        } else {
            (first, Some(first + 1))
        }
    }

    fn store_diagnostic_plane_lines(
        &mut self,
        row: usize,
        duplicate_row: Option<usize>,
        plane_a: &[u32; FB_WIDTH],
        plane_b: &[u32; FB_WIDTH],
    ) {
        for (framebuffer, pixels) in self
            .diagnostic_plane_framebuffer
            .iter_mut()
            .zip([plane_a.as_slice(), plane_b.as_slice()])
        {
            let Some(framebuffer) = framebuffer else {
                continue;
            };
            let start = row * FB_WIDTH;
            framebuffer[start..start + FB_WIDTH].copy_from_slice(pixels);
            if let Some(other) = duplicate_row {
                let start = other * FB_WIDTH;
                framebuffer[start..start + FB_WIDTH].copy_from_slice(pixels);
            }
        }
    }

    fn clear_diagnostic_plane_lines(&mut self, row: usize, duplicate_row: Option<usize>) {
        for framebuffer in self.diagnostic_plane_framebuffer.iter_mut().flatten() {
            let start = row * FB_WIDTH;
            framebuffer[start..start + FB_WIDTH].fill(0);
            if let Some(other) = duplicate_row {
                let start = other * FB_WIDTH;
                framebuffer[start..start + FB_WIDTH].fill(0);
            }
        }
    }

    /// Whether this physical active line consumes bitmap data and a DCA
    /// control slot. PAL Compatibility Mode masks 20 lines at both ends of
    /// the 280-line raster, leaving a 240-line display file (MCD212 tables
    /// 5-6 and 5-7).
    fn display_file_line(&self, scanline: u32) -> bool {
        let (ica_height, total_height) = geometry(self.pal);
        if !(ica_height..total_height).contains(&scanline) {
            return false;
        }
        !(self.display_geometry().active_y != 0
            && (scanline - ica_height < 20 || scanline >= total_height - 20))
    }

    /// Render the current field line into the progressive host framebuffer.
    fn render_line(&mut self, planea: &[u8], planeb: &[u8], external: Option<ExternalVideo<'_>>) {
        let (ica_height, _) = geometry(self.pal);
        let scanline = self.line;
        let active_line = (scanline - ica_height) as usize;
        let (row, duplicate_row) = self.output_rows(active_line);
        if row >= FB_HEIGHT {
            return;
        }

        // PAL Compatibility Mode: 20-line top/bottom masks in both scan
        // modes (MCD212 tables 5-6/5-7 and Green Book V.4.8).
        if !self.display_file_line(scanline) {
            let start = row * FB_WIDTH;
            self.field_framebuffer[start..start + FB_WIDTH].fill(COLOR_4BPP[0]);
            if let Some(other) = duplicate_row {
                let start = other * FB_WIDTH;
                self.field_framebuffer[start..start + FB_WIDTH].fill(COLOR_4BPP[0]);
            }
            self.clear_diagnostic_plane_lines(row, duplicate_row);
            return;
        }

        let mut plane_a = [0u32; FB_WIDTH];
        let mut plane_b = [0u32; FB_WIDTH];
        let mut ta = [false; FB_WIDTH];
        let mut tb = [false; FB_WIDTH];
        self.process_vsr(0, planea, planeb, &mut plane_a, &mut ta);
        self.process_vsr(1, planeb, planea, &mut plane_b, &mut tb);

        let mut line = [0u32; FB_WIDTH];
        self.store_diagnostic_plane_lines(row, duplicate_row, &plane_a, &plane_b);
        self.mix_line(
            &plane_a,
            &ta,
            &plane_b,
            &tb,
            &mut line,
            active_line,
            external,
        );
        let start = row * FB_WIDTH;
        self.field_framebuffer[start..start + FB_WIDTH].copy_from_slice(&line);
        if let Some(other) = duplicate_row {
            let start = other * FB_WIDTH;
            self.field_framebuffer[start..start + FB_WIDTH].copy_from_slice(&line);
        }
    }

    fn plane_word(plane: &[u8], word_addr: u32) -> u32 {
        let i = ((word_addr as usize) * 2) % plane.len().max(2);
        (u32::from(plane[i]) << 8) | u32::from(plane[i + 1])
    }

    /// Run the Image Control Area program for `path`.
    fn process_ica(&mut self, path: usize, plane: &[u8]) {
        let (ica_height, _) = geometry(self.pal);
        let max = ica_height * 120;
        // MCD212 table 5-8: non-interlaced fields always start at byte
        // address $400. In interlace mode the odd field (PA=1) starts at
        // $400 and the even field (PA=0) starts at $404. `addr` is a
        // word address, hence $200/$202 here.
        let interlaced_even_field = self.dcr[0] & DCR_SM != 0 && self.csrr[0] & CSR1R_PA == 0;
        let mut addr: u32 = if interlaced_even_field { 0x202 } else { 0x200 };
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
                0x78..=0x7F => self.set_display_parameters(path, cmd),
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
                0x78..=0x7F => self.set_display_parameters(path, cmd),
                reg => self.set_register(path, reg as u8, cmd & 0x00FF_FFFF),
            }
        }
        addr += (64 - count) / 2;
        self.dca[path] = (addr * 2) & 0x0007_FFFC;
    }

    /// Advance by `cycles` CPU cycles; runs per-line and per-frame work.
    pub fn tick(&mut self, cycles: u64, planea: &[u8], planeb: &[u8]) {
        self.tick_with_external(cycles, planea, planeb, None);
    }

    /// Advance the display while sampling the optional Digital Video
    /// Cartridge plane behind the two base-case graphics planes.
    pub(crate) fn tick_with_external(
        &mut self,
        cycles: u64,
        planea: &[u8],
        planeb: &[u8],
        external: Option<ExternalVideo<'_>>,
    ) {
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
                // Motorola MCD212 Technical Reference Manual, rev. 0, §7.6
                // (Cursor Control Register): each CON/COF unit is twelve
                // television fields, independent of the 50/60 Hz standard.
                self.blink_time += 1;
                let on_time = (self.cursor_control >> CURCNT_CON_SHIFT) & 7;
                let off_time = (self.cursor_control >> CURCNT_COF_SHIFT) & 7;
                if !self.blink_active && self.blink_time >= on_time * CURSOR_BLINK_FIELDS_PER_UNIT {
                    self.blink_active = true;
                    self.blink_time = 0;
                }
                if self.blink_active && self.blink_time >= off_time * CURSOR_BLINK_FIELDS_PER_UNIT {
                    self.blink_active = false;
                    self.blink_time = 0;
                }
            } else if self.line >= ica_height {
                // Active display region.
                self.csrr[0] |= CSR1R_DA;
                if self.dcr[0] & DCR_DE != 0 {
                    self.render_line(planea, planeb, external);
                }
                // The first DCA slot was fetched after ICA. PAL
                // Compatibility Mode's masked top/bottom lines do not consume
                // display-file data or DCA slots; hold the first slot through
                // the top mask and fetch successors only between its 240
                // content lines.
                let advance_dca =
                    self.display_file_line(self.line) && self.display_file_line(self.line + 1);
                if advance_dca && self.dca_enabled(0) {
                    self.process_dca(0, planea);
                }
                if advance_dca && self.dca_enabled(1) {
                    self.process_dca(1, planeb);
                }
            }

            if self.line == total_height - 1 {
                self.compose_framebuffer();
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

    fn advance_fields(mcd212: &mut Mcd212, fields: u32) {
        let plane = vec![0; 0x80000];
        let field_cycles = cycles_per_line(mcd212.pal) * u64::from(geometry(mcd212.pal).1);
        for _ in 0..fields {
            mcd212.tick(field_cycles, &plane, &plane);
        }
    }

    #[test]
    fn cursor_blink_units_are_twelve_fields_in_pal_and_ntsc() {
        for pal in [true, false] {
            let mut mcd212 = Mcd212::new(pal);
            if !pal {
                mcd212.write8(0x12, (DCR_FD >> 8) as u8);
            }
            mcd212.set_register(
                0,
                0xCE,
                CURCNT_EN | (1 << CURCNT_CON_SHIFT) | (1 << CURCNT_COF_SHIFT),
            );

            advance_fields(&mut mcd212, 11);
            assert!(
                !mcd212.blink_active,
                "{} cursor switched off before twelve fields",
                if pal { "PAL" } else { "NTSC" }
            );
            advance_fields(&mut mcd212, 1);
            assert!(
                mcd212.blink_active,
                "{} cursor did not switch off on the twelfth field",
                if pal { "PAL" } else { "NTSC" }
            );

            advance_fields(&mut mcd212, 11);
            assert!(
                mcd212.blink_active,
                "{} cursor switched on before twelve further fields",
                if pal { "PAL" } else { "NTSC" }
            );
            advance_fields(&mut mcd212, 1);
            assert!(
                !mcd212.blink_active,
                "{} cursor did not switch on on the twelfth further field",
                if pal { "PAL" } else { "NTSC" }
            );
        }
    }

    #[test]
    fn presentation_expands_ccir_levels_once() {
        assert_eq!(presentation_rgb(0x0010_1010), 0x0000_0000);
        assert_eq!(presentation_rgb(0x00EB_EBEB), 0x00FF_FFFF);
        assert_eq!(presentation_rgb(0x0000_10FF), 0x0000_00FF);
        assert_eq!(presentation_rgb(0x007E_7E7E), 0x0080_8080);
    }

    #[test]
    fn output_rows_duplicate_progressive_and_weave_interlace() {
        let mut m = Mcd212::new(true);
        assert_eq!(m.output_rows(7), (14, Some(15)));

        m.dcr[0] |= DCR_SM;
        m.csrr[0] |= CSR1R_PA;
        assert_eq!(m.output_rows(7), (14, None), "odd field supplies odd lines");
        m.csrr[0] &= !CSR1R_PA;
        assert_eq!(
            m.output_rows(7),
            (15, None),
            "even field supplies even lines"
        );
    }

    #[test]
    fn decoded_plane_diagnostics_are_opt_in_and_follow_output_rows() {
        let mut m = Mcd212::new(true);
        assert!(m.diagnostic_plane_framebuffer(0).is_none());
        assert!(m.diagnostic_plane_framebuffer(1).is_none());

        m.set_diagnostic_plane_capture(true);
        let plane_a = [0x0011_2233; FB_WIDTH];
        let plane_b = [0x0044_5566; FB_WIDTH];
        m.store_diagnostic_plane_lines(14, Some(15), &plane_a, &plane_b);

        for row in [14, 15] {
            let start = row * FB_WIDTH;
            assert_eq!(
                &m.diagnostic_plane_framebuffer(0).unwrap()[start..start + FB_WIDTH],
                &plane_a
            );
            assert_eq!(
                &m.diagnostic_plane_framebuffer(1).unwrap()[start..start + FB_WIDTH],
                &plane_b
            );
        }
        assert_eq!(m.diagnostic_base_framebuffer().len(), FB_WIDTH * FB_HEIGHT);

        m.set_diagnostic_plane_capture(false);
        assert!(m.diagnostic_plane_framebuffer(0).is_none());
        assert!(m.diagnostic_plane_framebuffer(1).is_none());
    }

    #[test]
    fn display_geometry_follows_the_specification_matrix() {
        for pal in [false, true] {
            for cf in [false, true] {
                for compatibility in [false, true] {
                    for frame_duration_60hz in [false, true] {
                        for interlaced in [false, true] {
                            for odd_field in [false, true] {
                                let mut m = Mcd212::new(pal);
                                if cf {
                                    m.dcr[0] |= DCR_CF;
                                }
                                if frame_duration_60hz {
                                    m.dcr[0] |= DCR_FD;
                                }
                                if interlaced {
                                    m.dcr[0] |= DCR_SM;
                                }
                                if compatibility {
                                    m.csrw[0] |= CSR1W_ST;
                                }
                                if odd_field {
                                    m.csrr[0] |= CSR1R_PA;
                                }

                                let geometry = m.display_geometry();
                                let raster_height = if pal { 560 } else { 480 };
                                let active_width = if cf && !compatibility { 768 } else { 720 };
                                let vertical_compatibility =
                                    pal && compatibility && !frame_duration_60hz;
                                let active_height = if vertical_compatibility {
                                    480
                                } else {
                                    raster_height
                                };
                                assert_eq!(
                                    geometry,
                                    DisplayGeometry {
                                        raster_width: 768,
                                        raster_height,
                                        active_x: (768 - active_width) / 2,
                                        active_y: (raster_height - active_height) / 2,
                                        active_width,
                                        active_height,
                                        compatibility_mode: compatibility,
                                        interlaced,
                                        odd_field,
                                        frame_duration_60hz,
                                        pixel_aspect_num: if pal { 41 } else { 49 },
                                        pixel_aspect_den: 40,
                                    }
                                );
                                assert_eq!(m.visible_size(), (768, raster_height));
                                assert_eq!(m.border_width(), geometry.active_x);
                                assert_eq!(m.screen_width(), geometry.active_width);
                                assert_eq!(m.top_border(), geometry.active_y);
                                assert_eq!(m.screen_height(), geometry.active_height);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn animated_hardware_cursor_is_overlaid_after_field_weave() {
        let mut m = Mcd212::new(true);
        let background = 0x0012_3456;
        m.field_framebuffer.fill(background);
        m.cursor_position = 5 << 12;
        m.cursor_control = CURCNT_EN | 15;
        m.cursor_pattern[0] = 0x8000;

        m.compose_framebuffer();
        let border = m.border_width();
        let first_row = 5 * 2;
        assert_eq!(m.framebuffer[first_row * FB_WIDTH + border], COLOR_4BPP[15]);
        assert_eq!(
            m.framebuffer[(first_row + 1) * FB_WIDTH + border],
            COLOR_4BPP[15],
            "the current cursor shape is applied to both field rows"
        );

        m.cursor_position = (5 << 12) | 4;
        m.compose_framebuffer();
        assert_eq!(
            m.framebuffer[first_row * FB_WIDTH + border],
            background,
            "the previous field must not retain the old cursor shape"
        );
        assert_eq!(
            m.framebuffer[first_row * FB_WIDTH + border + 4],
            COLOR_4BPP[15]
        );
    }

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
    fn pal_compatibility_masks_do_not_consume_dca_slots_in_either_scan_mode() {
        for interlaced in [false, true] {
            let mut m = Mcd212::new(true);
            let mut plane = vec![0u8; 0x80000];
            const DCP: usize = 0x1000;

            // Both field entry points link the same DCA table. Noninterlace
            // enters at $400; this test starts interlace on the even-field
            // $404 entry.
            plane[0x400..0x404].copy_from_slice(&[0x30, 0x00, 0x10, 0x00]);
            plane[0x404..0x408].copy_from_slice(&[0x30, 0x00, 0x10, 0x00]);
            // Exactly 240 valid line-control slots, all NOPs.
            for slot in 0..240 {
                for command in 0..16 {
                    plane[DCP + slot * 64 + command * 4] = 0x10;
                }
            }
            // If either 20-line mask consumes slots, this bitmap data would
            // be misexecuted as a control command and alter the backdrop.
            plane[DCP + 240 * 64..DCP + 240 * 64 + 4].copy_from_slice(&[0xD8, 0, 0, 5]);

            m.dcr[0] = DCR_DE | DCR_CF | DCR_ICA | DCR_DCA | if interlaced { DCR_SM } else { 0 };
            m.csrw[0] = CSR1W_ST;
            m.line = geometry(true).1 - 1;
            m.tick(cycles_per_line(true), &plane, &plane); // ICA + first DCA slot.
            m.tick(
                cycles_per_line(true) * u64::from(geometry(true).1 - 1),
                &plane,
                &plane,
            );

            assert_eq!(m.dca[0], (DCP + 240 * 64) as u32, "interlaced={interlaced}");
            assert_eq!(m.backdrop_color, 0);
        }
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
    fn ica_entry_address_follows_scan_mode_and_field_parity() {
        let mut plane = vec![0u8; 0x80000];
        // Keep the $400 and $404 entry points independent by jumping each to
        // a small program that selects a distinct backdrop color.
        plane[0x400..0x404].copy_from_slice(&[0x40, 0x00, 0x10, 0x00]);
        plane[0x404..0x408].copy_from_slice(&[0x40, 0x00, 0x20, 0x00]);
        plane[0x1000..0x1008].copy_from_slice(&[0xD8, 0, 0, 5, 0, 0, 0, 0]);
        plane[0x2000..0x2008].copy_from_slice(&[0xD8, 0, 0, 6, 0, 0, 0, 0]);

        // Table 5-8 fixes non-interlace at $400, regardless of the PA status
        // left over from the previous frame.
        for parity in [0, CSR1R_PA] {
            let mut m = Mcd212::new(true);
            m.csrr[0] = parity;
            m.process_ica(0, &plane);
            assert_eq!(m.backdrop_color, 5);
        }

        // Interlace uses $400 for the odd field and $404 for the even field.
        let mut odd = Mcd212::new(true);
        odd.dcr[0] = DCR_SM;
        odd.csrr[0] = CSR1R_PA;
        odd.process_ica(0, &plane);
        assert_eq!(odd.backdrop_color, 5);

        let mut even = Mcd212::new(true);
        even.dcr[0] = DCR_SM;
        even.csrr[0] = 0;
        even.process_ica(0, &plane);
        assert_eq!(even.backdrop_color, 6);
    }

    #[test]
    fn ica_interrupt_and_csr2_clear() {
        let mut m = Mcd212::new(true);
        let mut plane = vec![0u8; 0x80000];
        // Cover both interlaced entry points: INTERRUPT, INTERRUPT, STOP,
        // STOP.
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
        // Green Book RELOAD DISPLAY PARAMETERS with BP_DOUBLE, PRF_X16,
        // and RMS_MOSAIC at both field-parity start addresses, then STOPs.
        plane[0x400..0x410].copy_from_slice(&[
            0x78, 0, 0x05, 0x0F, 0x78, 0, 0x05, 0x0F, 0x00, 0, 0, 0, 0x00, 0, 0, 0,
        ]);
        m.dcr[0] |= DCR_DE | DCR_ICA;
        m.tick(cycles_per_line(true) * 313, &plane, &plane);
        assert_eq!(m.ddr[0] & 0x0F00, 0x0F00);
        assert_ne!(m.dcr[0] & DCR_CM, 0);
    }

    #[test]
    fn ica_uses_green_book_display_parameter_fields() {
        let mut m = Mcd212::new(true);
        let mut plane = vec![0u8; 0x80000];
        // Green Book V.4.6.1 Figure V.49 and Philips' cp_dprm macro:
        // fixed bit 10, BP_DOUBLE at bits 9:8, PRF at 3:2, RMS at 1:0.
        plane[0x400..0x410].copy_from_slice(&[
            0x78, 0x00, 0x05, 0x00, 0x78, 0x00, 0x05, 0x00, 0x00, 0, 0, 0, 0x00, 0, 0, 0,
        ]);
        m.dcr[0] |= DCR_DE | DCR_ICA;
        m.tick(cycles_per_line(true) * 313, &plane, &plane);
        assert_ne!(m.dcr[0] & DCR_CM, 0);
        assert_eq!(m.ddr[0] & 0x0F00, 0);
    }

    #[test]
    fn dca_uses_green_book_display_parameter_fields() {
        let mut m = Mcd212::new(true);
        let mut plane = vec![0u8; 0x80000];
        plane[0x100..0x108].copy_from_slice(&[
            0x78, 0x00, 0x05, 0x06, // BP_DOUBLE, PRF_X4, RMS_RUNLENGTH
            0x00, 0, 0, 0,
        ]);
        m.dca[0] = 0x100;
        m.process_dca(0, &plane);
        assert_ne!(m.dcr[0] & DCR_CM, 0);
        assert_eq!(m.ddr[0] & 0x0F00, 0x0600);

        plane[0x100..0x108].copy_from_slice(&[
            0x78, 0x00, 0x04, 0x00, // BP_NORMAL, PRF_X2, RMS_NORMAL
            0x00, 0, 0, 0,
        ]);
        m.dca[0] = 0x100;
        m.process_dca(0, &plane);
        assert_eq!(m.dcr[0] & DCR_CM, 0);
        assert_eq!(m.ddr[0] & 0x0F00, 0);
    }
}
