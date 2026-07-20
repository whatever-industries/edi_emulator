// SPDX-License-Identifier: MIT
//! Incremental, safe-Rust MPEG-1 video decoder.
//!
//! This is an attributed translation of the MIT-licensed video decoder in
//! `gen2brain/mpeg` revision `27c6f084c6ca342380c99a59a6a130b3f716e9d7`.
//! Its unsafe Go pixel-slice convenience path is intentionally omitted. All
//! motion compensation and YCbCr conversion use bounds-checked Rust.

#[path = "mpeg1_tables.rs"]
mod tables;

use tables::*;

const PICTURE_TYPE_INTRA: u8 = 1;
const PICTURE_TYPE_PREDICTIVE: u8 = 2;
const PICTURE_TYPE_B: u8 = 3;

const START_PICTURE: u8 = 0x00;
const START_SLICE_FIRST: u8 = 0x01;
const START_SLICE_LAST: u8 = 0xAF;
const START_USER_DATA: u8 = 0xB2;
const START_SEQUENCE: u8 = 0xB3;
const START_EXTENSION: u8 = 0xB5;
const START_SEQUENCE_END: u8 = 0xB7;
const START_GROUP: u8 = 0xB8;

#[derive(Debug, Clone, Copy)]
struct Vlc {
    index: i16,
    value: i16,
}

impl Vlc {
    const fn new(index: i16, value: i16) -> Self {
        Self { index, value }
    }
}

#[derive(Debug, Clone, Copy)]
struct VlcUint {
    index: i16,
    value: u16,
}

impl VlcUint {
    const fn new(index: i16, value: u16) -> Self {
        Self { index, value }
    }
}

#[derive(Debug, Default)]
struct BitBuffer {
    bytes: Vec<u8>,
    bit_index: usize,
}

impl BitBuffer {
    fn write(&mut self, bytes: &[u8]) {
        self.discard_read_bytes();
        self.bytes.extend_from_slice(bytes);
    }

    fn remaining_bits(&self) -> usize {
        self.bytes
            .len()
            .saturating_mul(8)
            .saturating_sub(self.bit_index)
    }

    fn has(&self, count: usize) -> bool {
        self.remaining_bits() >= count
    }

    fn read(&mut self, mut count: usize) -> Option<u32> {
        if count > 32 || !self.has(count) {
            return None;
        }
        let mut value = 0u32;
        while count != 0 {
            let current = u32::from(self.bytes[self.bit_index >> 3]);
            let remaining = 8 - (self.bit_index & 7);
            let take = remaining.min(count);
            let shift = remaining - take;
            let mask = 0xFFu32 >> (8 - take);
            value = (value << take) | ((current >> shift) & mask);
            self.bit_index += take;
            count -= take;
        }
        Some(value)
    }

    fn read1(&mut self) -> Option<u8> {
        self.read(1).map(|value| value as u8)
    }

    fn skip(&mut self, count: usize) -> bool {
        if !self.has(count) {
            return false;
        }
        self.bit_index += count;
        true
    }

    fn align(&mut self) {
        self.bit_index = (self.bit_index + 7) & !7;
    }

    fn next_start_code(&mut self) -> Option<u8> {
        self.align();
        let mut at = self.bit_index >> 3;
        while at + 3 < self.bytes.len() {
            if self.bytes[at..at + 3] == [0, 0, 1] {
                self.bit_index = (at + 4) << 3;
                return Some(self.bytes[at + 3]);
            }
            at += 1;
        }
        // Keep a possible split 00 00 01 prefix for the next feed.
        self.bit_index = self.bytes.len().saturating_sub(3) << 3;
        None
    }

    fn find_start_code(&mut self, wanted: u8) -> Option<u8> {
        loop {
            let code = self.next_start_code()?;
            if code == wanted {
                return Some(code);
            }
        }
    }

    fn has_start_code(&mut self, wanted: u8) -> bool {
        let saved = self.bit_index;
        let found = self.find_start_code(wanted).is_some();
        self.bit_index = saved;
        found
    }

    fn peek_non_zero(&mut self, count: usize) -> bool {
        let saved = self.bit_index;
        let value = self.read(count).unwrap_or(0);
        self.bit_index = saved;
        value != 0
    }

    fn read_vlc(&mut self, table: &[Vlc]) -> Option<i32> {
        let mut state = Vlc::new(0, 0);
        for _ in 0..64 {
            let index = i32::from(state.index) + i32::from(self.read1()?);
            if index < 0 {
                return None;
            }
            state = *table.get(index as usize)?;
            if state.index <= 0 {
                return (state.index == 0).then_some(i32::from(state.value));
            }
        }
        None
    }

    fn read_vlc_uint(&mut self, table: &[VlcUint]) -> Option<u16> {
        let mut state = VlcUint::new(0, 0);
        for _ in 0..64 {
            let index = i32::from(state.index) + i32::from(self.read1()?);
            if index < 0 {
                return None;
            }
            state = *table.get(index as usize)?;
            if state.index <= 0 {
                return (state.index == 0).then_some(state.value);
            }
        }
        None
    }

    fn discard_read_bytes(&mut self) {
        let byte_pos = self.bit_index >> 3;
        if byte_pos == 0 {
            return;
        }
        self.bytes.drain(..byte_pos.min(self.bytes.len()));
        self.bit_index = self.bit_index.saturating_sub(byte_pos << 3);
    }
}

#[derive(Debug, Default)]
struct Plane {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

#[derive(Debug, Default)]
struct Frame {
    width: usize,
    height: usize,
    first_in_sequence: bool,
    first_in_group: bool,
    y: Plane,
    cb: Plane,
    cr: Plane,
}

impl Frame {
    fn new(width: usize, height: usize, luma_width: usize, luma_height: usize) -> Self {
        let chroma_width = luma_width / 2;
        let chroma_height = luma_height / 2;
        Self {
            width,
            height,
            first_in_sequence: false,
            first_in_group: false,
            y: Plane {
                width: luma_width,
                height: luma_height,
                data: vec![0; luma_width * luma_height],
            },
            cb: Plane {
                width: chroma_width,
                height: chroma_height,
                data: vec![128; chroma_width * chroma_height],
            },
            cr: Plane {
                width: chroma_width,
                height: chroma_height,
                data: vec![128; chroma_width * chroma_height],
            },
        }
    }

    fn to_rgb(&self) -> DecodedVideoFrame {
        let mut pixels = vec![0; self.width * self.height];
        for y in 0..self.height {
            for x in 0..self.width {
                let yy = i32::from(self.y.data[y * self.y.width + x]);
                let cb = i32::from(self.cb.data[(y / 2) * self.cb.width + x / 2]) - 128;
                let cr = i32::from(self.cr.data[(y / 2) * self.cr.width + x / 2]) - 128;
                // The VMPEG decoder feeds the MCD212's internal CCIR-601 RGB
                // domain, where neutral black is 16 and white is 235. Do not
                // expand to desktop/full-range RGB here: the external video
                // still has to pass through the hardware mixer and mattes.
                let r = (yy + (351 * cr) / 256).clamp(0, 255) as u32;
                let g = (yy - (86 * cb + 179 * cr) / 256).clamp(0, 255) as u32;
                let b = (yy + (444 * cb) / 256).clamp(0, 255) as u32;
                pixels[y * self.width + x] = (r << 16) | (g << 8) | b;
            }
        }
        DecodedVideoFrame {
            width: self.width,
            height: self.height,
            pixels,
            first_in_sequence: self.first_in_sequence,
            first_in_group: self.first_in_group,
            last_in_sequence: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecodedVideoFrame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
    pub first_in_sequence: bool,
    pub first_in_group: bool,
    pub last_in_sequence: bool,
}

enum PictureResult {
    Frame(DecodedVideoFrame),
    /// A valid first reference picture has been retained until display order
    /// can be established from the following coded picture.
    Deferred,
}

#[derive(Debug, Default, Clone, Copy)]
struct Motion {
    full_px: bool,
    r_size: u8,
    h: i32,
    v: i32,
    is_set: bool,
}

#[derive(Debug)]
pub(crate) struct Mpeg1VideoDecoder {
    buffer: BitBuffer,
    pending_start_code: Option<u8>,
    frame_rate: f64,
    sequence_prpa: u8,
    width: usize,
    height: usize,
    mb_width: usize,
    mb_height: usize,
    mb_size: usize,
    luma_width: usize,
    luma_height: usize,
    picture_type: u8,
    motion_forward: Motion,
    motion_backward: Motion,
    has_sequence_header: bool,
    has_reference_frame: bool,
    sequence_intra_pending: bool,
    group_intra_pending: bool,
    quantizer_scale: i32,
    slice_begin: bool,
    macroblock_address: i32,
    mb_row: usize,
    mb_col: usize,
    macroblock_type: i32,
    macroblock_intra: bool,
    dc_predictor: [i32; 3],
    frame_current: Frame,
    frame_forward: Frame,
    frame_backward: Frame,
    block_data: [i32; 64],
    intra_quant_matrix: [u8; 64],
    non_intra_quant_matrix: [u8; 64],
    pub errors: u64,
    pub sequence_headers: u64,
    pub group_headers: u64,
    pub sequence_ends: u64,
}

impl Default for Mpeg1VideoDecoder {
    fn default() -> Self {
        Self {
            buffer: BitBuffer::default(),
            pending_start_code: None,
            frame_rate: 0.0,
            sequence_prpa: 0,
            width: 0,
            height: 0,
            mb_width: 0,
            mb_height: 0,
            mb_size: 0,
            luma_width: 0,
            luma_height: 0,
            picture_type: 0,
            motion_forward: Motion::default(),
            motion_backward: Motion::default(),
            has_sequence_header: false,
            has_reference_frame: false,
            sequence_intra_pending: false,
            group_intra_pending: false,
            quantizer_scale: 0,
            slice_begin: false,
            macroblock_address: 0,
            mb_row: 0,
            mb_col: 0,
            macroblock_type: 0,
            macroblock_intra: false,
            dc_predictor: [0; 3],
            frame_current: Frame::default(),
            frame_forward: Frame::default(),
            frame_backward: Frame::default(),
            block_data: [0; 64],
            intra_quant_matrix: VIDEO_INTRA_QUANT_MATRIX,
            non_intra_quant_matrix: VIDEO_NON_INTRA_QUANT_MATRIX,
            errors: 0,
            sequence_headers: 0,
            group_headers: 0,
            sequence_ends: 0,
        }
    }
}

impl Mpeg1VideoDecoder {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.buffer.write(bytes);
    }

    pub fn frame_rate(&self) -> f64 {
        self.frame_rate
    }

    /// Raw MPEG sequence-header aspect-ratio/frame-rate byte exposed by the
    /// MCD251 temporal picture-rate/aspect register.
    pub fn sequence_prpa(&self) -> u8 {
        self.sequence_prpa
    }

    pub fn decode_available(&mut self, limit: usize) -> Vec<DecodedVideoFrame> {
        let mut frames = Vec::new();
        while frames.len() < limit {
            match self.decode_next() {
                Some(frame) => frames.push(frame),
                None => break,
            }
        }
        frames
    }

    fn decode_next(&mut self) -> Option<DecodedVideoFrame> {
        loop {
            let code = match self.pending_start_code.take() {
                Some(code) => code,
                None => self.buffer.next_start_code()?,
            };
            match code {
                START_SEQUENCE => {
                    let saved = self.buffer.bit_index;
                    if self.decode_sequence_header().is_err() {
                        self.buffer.bit_index = saved;
                        self.pending_start_code = Some(code);
                        return None;
                    }
                }
                START_GROUP => {
                    self.group_headers += 1;
                    self.group_intra_pending = true;
                }
                START_PICTURE if self.has_sequence_header => {
                    if !self.buffer.has_start_code(START_PICTURE)
                        && !self.buffer.has_start_code(START_SEQUENCE_END)
                    {
                        self.pending_start_code = Some(code);
                        return None;
                    }
                    match self.decode_picture() {
                        Some(PictureResult::Frame(frame)) => {
                            self.buffer.discard_read_bytes();
                            return Some(frame);
                        }
                        Some(PictureResult::Deferred) => {
                            self.buffer.discard_read_bytes();
                        }
                        None => {
                            self.errors += 1;
                            log::debug!(
                                "mpeg1: rejected picture type={} macroblock={}/{} ({},{}), sequence={} group={}, buffered_bits={}",
                                self.picture_type,
                                self.macroblock_address,
                                self.mb_size,
                                self.mb_col,
                                self.mb_row,
                                self.sequence_headers,
                                self.group_headers,
                                self.buffer.remaining_bits(),
                            );
                        }
                    }
                }
                START_SEQUENCE_END => {
                    self.sequence_ends += 1;
                    if self.has_reference_frame {
                        self.has_reference_frame = false;
                        let mut frame = self.frame_backward.to_rgb();
                        frame.last_in_sequence = true;
                        return Some(frame);
                    }
                }
                _ => {}
            }
        }
    }

    fn decode_sequence_header(&mut self) -> Result<(), ()> {
        let width = self.buffer.read(12).ok_or(())? as usize;
        let height = self.buffer.read(12).ok_or(())? as usize;
        if width == 0 || height == 0 || width > 4095 || height > 4095 {
            return Err(());
        }
        let aspect_code = self.buffer.read(4).ok_or(())? as u8;
        let _aspect_ratio = VIDEO_ASPECT_RATIO[usize::from(aspect_code)];
        let frame_rate_code = self.buffer.read(4).ok_or(())? as u8;
        let frame_rate = VIDEO_PICTURE_RATE[usize::from(frame_rate_code)];
        if frame_rate == 0.0 {
            return Err(());
        }
        let _bit_rate = self.buffer.read(18).ok_or(())?;
        if !self.buffer.skip(12) {
            return Err(());
        }

        let mut intra = VIDEO_INTRA_QUANT_MATRIX;
        if self.buffer.read1().ok_or(())? != 0 {
            for &index in &VIDEO_ZIG_ZAG {
                intra[index as usize] = self.buffer.read(8).ok_or(())? as u8;
            }
        }
        let mut non_intra = VIDEO_NON_INTRA_QUANT_MATRIX;
        if self.buffer.read1().ok_or(())? != 0 {
            for &index in &VIDEO_ZIG_ZAG {
                non_intra[index as usize] = self.buffer.read(8).ok_or(())? as u8;
            }
        }

        let mb_width = (width + 15) >> 4;
        let mb_height = (height + 15) >> 4;
        let luma_width = mb_width << 4;
        let luma_height = mb_height << 4;
        let geometry_changed = !self.has_sequence_header
            || self.width != width
            || self.height != height
            || self.luma_width != luma_width
            || self.luma_height != luma_height;

        self.width = width;
        self.height = height;
        self.frame_rate = frame_rate;
        self.sequence_prpa = (aspect_code << 4) | frame_rate_code;
        self.mb_width = mb_width;
        self.mb_height = mb_height;
        self.mb_size = mb_width * mb_height;
        self.luma_width = luma_width;
        self.luma_height = luma_height;
        self.intra_quant_matrix = intra;
        self.non_intra_quant_matrix = non_intra;
        // MPEG-1 streams commonly repeat an unchanged sequence header before
        // a GOP. The delayed reference picture still belongs to the display
        // sequence and the following B pictures may depend on the retained
        // reference pair. Reallocating here used to discard exactly one frame
        // per repeated header and briefly decode against empty reference
        // planes, producing visible macroblock corruption.
        if geometry_changed {
            self.frame_current = Frame::new(width, height, luma_width, luma_height);
            self.frame_forward = Frame::new(width, height, luma_width, luma_height);
            self.frame_backward = Frame::new(width, height, luma_width, luma_height);
            self.has_reference_frame = false;
        }
        self.has_sequence_header = true;
        self.sequence_intra_pending = true;
        self.sequence_headers += 1;
        Ok(())
    }

    fn decode_picture(&mut self) -> Option<PictureResult> {
        self.buffer.skip(10).then_some(())?;
        self.picture_type = self.buffer.read(3)? as u8;
        self.buffer.skip(16).then_some(())?;
        if !(PICTURE_TYPE_INTRA..=PICTURE_TYPE_B).contains(&self.picture_type) {
            return None;
        }

        if matches!(self.picture_type, PICTURE_TYPE_PREDICTIVE | PICTURE_TYPE_B) {
            self.motion_forward.full_px = self.buffer.read1()? != 0;
            let f_code = self.buffer.read(3)? as u8;
            if f_code == 0 {
                return None;
            }
            self.motion_forward.r_size = f_code - 1;
        }
        if self.picture_type == PICTURE_TYPE_B {
            self.motion_backward.full_px = self.buffer.read1()? != 0;
            let f_code = self.buffer.read(3)? as u8;
            if f_code == 0 {
                return None;
            }
            self.motion_backward.r_size = f_code - 1;
        }

        let reference = matches!(
            self.picture_type,
            PICTURE_TYPE_INTRA | PICTURE_TYPE_PREDICTIVE
        );
        // Reference pictures rotate three preallocated buffers. Swapping keeps all
        // three valid even when a malformed or truncated picture aborts decoding;
        // `mem::take` here used to leave an empty reference plane behind.
        if reference {
            std::mem::swap(&mut self.frame_forward, &mut self.frame_backward);
        }

        let decoded = (|| {
            let mut code = self.buffer.next_start_code()?;
            while matches!(code, START_EXTENSION | START_USER_DATA) {
                code = self.buffer.next_start_code()?;
            }
            while (START_SLICE_FIRST..=START_SLICE_LAST).contains(&code) {
                self.decode_slice(code)?;
                if self.macroblock_address >= self.mb_size as i32 - 2 {
                    code = self.buffer.next_start_code()?;
                    break;
                }
                code = self.buffer.next_start_code()?;
            }
            Some(code)
        })();
        let code = match decoded {
            Some(code) => code,
            None => {
                if reference {
                    std::mem::swap(&mut self.frame_forward, &mut self.frame_backward);
                }
                return None;
            }
        };
        self.pending_start_code = Some(code);

        let intra = self.picture_type == PICTURE_TYPE_INTRA;
        self.frame_current.first_in_sequence = intra && self.sequence_intra_pending;
        self.frame_current.first_in_group = intra && self.group_intra_pending;
        if intra {
            self.sequence_intra_pending = false;
            self.group_intra_pending = false;
        }

        if reference {
            std::mem::swap(&mut self.frame_backward, &mut self.frame_current);
        }

        if self.picture_type == PICTURE_TYPE_B {
            Some(PictureResult::Frame(self.frame_current.to_rgb()))
        } else if self.has_reference_frame {
            Some(PictureResult::Frame(self.frame_forward.to_rgb()))
        } else {
            self.has_reference_frame = true;
            Some(PictureResult::Deferred)
        }
    }

    fn decode_slice(&mut self, slice: u8) -> Option<()> {
        self.slice_begin = true;
        self.macroblock_address = (i32::from(slice) - 1) * self.mb_width as i32 - 1;
        self.motion_backward.h = 0;
        self.motion_backward.v = 0;
        self.motion_forward.h = 0;
        self.motion_forward.v = 0;
        self.dc_predictor = [128; 3];
        self.quantizer_scale = self.buffer.read(5)? as i32;
        while self.buffer.read1()? != 0 {
            self.buffer.skip(8).then_some(())?;
        }
        loop {
            self.decode_macroblock()?;
            if self.macroblock_address >= self.mb_size as i32 - 1 || !self.buffer.peek_non_zero(23)
            {
                break;
            }
        }
        Some(())
    }

    fn decode_macroblock(&mut self) -> Option<()> {
        let mut increment = 0i32;
        let mut value = self.buffer.read_vlc(VIDEO_MACROBLOCK_ADDRESS_INCREMENT)?;
        while value == 34 {
            value = self.buffer.read_vlc(VIDEO_MACROBLOCK_ADDRESS_INCREMENT)?;
        }
        while value == 35 {
            increment += 33;
            value = self.buffer.read_vlc(VIDEO_MACROBLOCK_ADDRESS_INCREMENT)?;
        }
        increment += value;

        if self.slice_begin {
            self.slice_begin = false;
            self.macroblock_address += increment;
        } else {
            if self.macroblock_address + increment >= self.mb_size as i32 {
                return None;
            }
            if increment > 1 {
                self.dc_predictor = [128; 3];
                if self.picture_type == PICTURE_TYPE_PREDICTIVE {
                    self.motion_forward.h = 0;
                    self.motion_forward.v = 0;
                }
            }
            while increment > 1 {
                self.macroblock_address += 1;
                self.update_macroblock_position()?;
                self.predict_macroblock();
                increment -= 1;
            }
            self.macroblock_address += 1;
        }

        self.update_macroblock_position()?;
        let table = match self.picture_type {
            PICTURE_TYPE_INTRA => VIDEO_MACROBLOCK_TYPE_INTRA,
            PICTURE_TYPE_PREDICTIVE => VIDEO_MACROBLOCK_TYPE_PREDICTIVE,
            PICTURE_TYPE_B => VIDEO_MACROBLOCK_TYPE_B,
            _ => return None,
        };
        self.macroblock_type = self.buffer.read_vlc(table)?;
        self.macroblock_intra = self.macroblock_type & 0x01 != 0;
        self.motion_forward.is_set = self.macroblock_type & 0x08 != 0;
        self.motion_backward.is_set = self.macroblock_type & 0x04 != 0;

        if self.macroblock_type & 0x10 != 0 {
            self.quantizer_scale = self.buffer.read(5)? as i32;
        }
        if self.macroblock_intra {
            self.motion_backward.h = 0;
            self.motion_backward.v = 0;
            self.motion_forward.h = 0;
            self.motion_forward.v = 0;
        } else {
            self.dc_predictor = [128; 3];
            self.decode_motion_vectors()?;
            self.predict_macroblock();
        }

        let cbp = if self.macroblock_type & 0x02 != 0 {
            self.buffer.read_vlc(VIDEO_CODE_BLOCK_PATTERN)?
        } else if self.macroblock_intra {
            0x3F
        } else {
            0
        };
        let mut mask = 0x20;
        for block in 0..6 {
            if cbp & mask != 0 {
                self.decode_block(block)?;
            }
            mask >>= 1;
        }
        Some(())
    }

    fn update_macroblock_position(&mut self) -> Option<()> {
        if self.macroblock_address < 0 {
            return None;
        }
        self.mb_row = self.macroblock_address as usize / self.mb_width;
        self.mb_col = self.macroblock_address as usize % self.mb_width;
        (self.mb_col < self.mb_width && self.mb_row < self.mb_height).then_some(())
    }

    fn decode_motion_vectors(&mut self) -> Option<()> {
        if self.motion_forward.is_set {
            self.motion_forward.h =
                self.decode_motion_vector(self.motion_forward.r_size, self.motion_forward.h)?;
            self.motion_forward.v =
                self.decode_motion_vector(self.motion_forward.r_size, self.motion_forward.v)?;
        } else if self.picture_type == PICTURE_TYPE_PREDICTIVE {
            self.motion_forward.h = 0;
            self.motion_forward.v = 0;
        }
        if self.motion_backward.is_set {
            self.motion_backward.h =
                self.decode_motion_vector(self.motion_backward.r_size, self.motion_backward.h)?;
            self.motion_backward.v =
                self.decode_motion_vector(self.motion_backward.r_size, self.motion_backward.v)?;
        }
        Some(())
    }

    fn decode_motion_vector(&mut self, r_size: u8, mut motion: i32) -> Option<i32> {
        let fscale = 1i32 << r_size;
        let motion_code = self.buffer.read_vlc(VIDEO_MOTION)?;
        let delta = if motion_code != 0 && fscale != 1 {
            let residual = self.buffer.read(r_size as usize)? as i32;
            let magnitude = ((motion_code.abs() - 1) << r_size) + residual + 1;
            if motion_code < 0 {
                -magnitude
            } else {
                magnitude
            }
        } else {
            motion_code
        };
        motion += delta;
        if motion > (fscale << 4) - 1 {
            motion -= fscale << 5;
        } else if motion < -(fscale << 4) {
            motion += fscale << 5;
        }
        Some(motion)
    }

    fn predict_macroblock(&mut self) {
        let mut forward_h = self.motion_forward.h;
        let mut forward_v = self.motion_forward.v;
        if self.motion_forward.full_px {
            forward_h <<= 1;
            forward_v <<= 1;
        }

        if self.picture_type == PICTURE_TYPE_B {
            let mut backward_h = self.motion_backward.h;
            let mut backward_v = self.motion_backward.v;
            if self.motion_backward.full_px {
                backward_h <<= 1;
                backward_v <<= 1;
            }
            if self.motion_forward.is_set {
                copy_macroblock(
                    forward_h,
                    forward_v,
                    self.mb_row,
                    self.mb_col,
                    &self.frame_forward,
                    &mut self.frame_current,
                    false,
                );
                if self.motion_backward.is_set {
                    copy_macroblock(
                        backward_h,
                        backward_v,
                        self.mb_row,
                        self.mb_col,
                        &self.frame_backward,
                        &mut self.frame_current,
                        true,
                    );
                }
            } else {
                copy_macroblock(
                    backward_h,
                    backward_v,
                    self.mb_row,
                    self.mb_col,
                    &self.frame_backward,
                    &mut self.frame_current,
                    false,
                );
            }
        } else {
            copy_macroblock(
                forward_h,
                forward_v,
                self.mb_row,
                self.mb_col,
                &self.frame_forward,
                &mut self.frame_current,
                false,
            );
        }
    }

    fn decode_block(&mut self, block: usize) -> Option<()> {
        self.block_data.fill(0);
        let mut n = 0usize;
        let quant_matrix;
        if self.macroblock_intra {
            let plane_index = block.saturating_sub(3);
            let predictor = self.dc_predictor[plane_index];
            let dct_table = if plane_index == 0 {
                VIDEO_DCT_SIZE_LUMINANCE
            } else {
                VIDEO_DCT_SIZE_CHROMINANCE
            };
            let dct_size = self.buffer.read_vlc(dct_table)? as usize;
            let coefficient = if dct_size != 0 {
                let differential = self.buffer.read(dct_size)? as i32;
                if differential & (1 << (dct_size - 1)) != 0 {
                    predictor + differential
                } else {
                    predictor + ((-1i32 << dct_size) | (differential + 1))
                }
            } else {
                predictor
            };
            self.block_data[0] = coefficient;
            self.dc_predictor[plane_index] = coefficient;
            self.block_data[0] <<= 8;
            quant_matrix = self.intra_quant_matrix;
            n = 1;
        } else {
            quant_matrix = self.non_intra_quant_matrix;
        }

        loop {
            let coefficient = self.buffer.read_vlc_uint(VIDEO_DCT_COEFF)?;
            if coefficient == 0x0001 && n > 0 && self.buffer.read1()? == 0 {
                break;
            }
            let (run, mut level) = if coefficient == 0xFFFF {
                let run = self.buffer.read(6)? as usize;
                let byte = self.buffer.read(8)? as i32;
                let level = match byte {
                    0 => self.buffer.read(8)? as i32,
                    128 => self.buffer.read(8)? as i32 - 256,
                    129..=255 => byte - 256,
                    _ => byte,
                };
                (run, level)
            } else {
                let run = usize::from(coefficient >> 8);
                let level =
                    i32::from(coefficient & 0xFF) * if self.buffer.read1()? != 0 { -1 } else { 1 };
                (run, level)
            };
            n = n.checked_add(run)?;
            if n >= 64 {
                return None;
            }
            let dezigzagged = usize::from(VIDEO_ZIG_ZAG[n] & 63);
            n += 1;

            level <<= 1;
            if !self.macroblock_intra {
                level += if level < 0 { -1 } else { 1 };
            }
            level = (level * self.quantizer_scale * i32::from(quant_matrix[dezigzagged])) >> 4;
            if level & 1 == 0 {
                level += if level > 0 { -1 } else { 1 };
            }
            level = level.clamp(-2048, 2047);
            self.block_data[dezigzagged] =
                level * i32::from(VIDEO_PREMULTIPLIER_MATRIX[dezigzagged]);
        }

        let (plane, index, scan) = if block < 4 {
            let mut index = (self.mb_row * self.luma_width + self.mb_col) << 4;
            if block & 1 != 0 {
                index += 8;
            }
            if block & 2 != 0 {
                index += self.luma_width << 3;
            }
            (&mut self.frame_current.y.data, index, self.luma_width - 8)
        } else if block == 4 {
            (
                &mut self.frame_current.cb.data,
                ((self.mb_row * self.luma_width) << 2) + (self.mb_col << 3),
                (self.luma_width >> 1) - 8,
            )
        } else {
            (
                &mut self.frame_current.cr.data,
                ((self.mb_row * self.luma_width) << 2) + (self.mb_col << 3),
                (self.luma_width >> 1) - 8,
            )
        };

        if self.macroblock_intra {
            if n == 1 {
                copy_value_to_dest((self.block_data[0] + 128) >> 8, plane, index, scan)?;
            } else {
                idct(&mut self.block_data, n);
                copy_block_to_dest(&self.block_data, plane, index, scan)?;
            }
        } else if n == 1 {
            add_value_to_dest((self.block_data[0] + 128) >> 8, plane, index, scan)?;
        } else {
            idct(&mut self.block_data, n);
            add_block_to_dest(&self.block_data, plane, index, scan)?;
        }
        self.block_data.fill(0);
        Some(())
    }
}

#[allow(clippy::too_many_arguments)]
fn copy_macroblock(
    motion_h: i32,
    motion_v: i32,
    mb_row: usize,
    mb_col: usize,
    source: &Frame,
    dest: &mut Frame,
    average: bool,
) {
    copy_motion_block(
        &source.y,
        &mut dest.y,
        mb_col * 16,
        mb_row * 16,
        16,
        motion_h,
        motion_v,
        average,
    );
    let chroma_h = motion_h / 2;
    let chroma_v = motion_v / 2;
    copy_motion_block(
        &source.cb,
        &mut dest.cb,
        mb_col * 8,
        mb_row * 8,
        8,
        chroma_h,
        chroma_v,
        average,
    );
    copy_motion_block(
        &source.cr,
        &mut dest.cr,
        mb_col * 8,
        mb_row * 8,
        8,
        chroma_h,
        chroma_v,
        average,
    );
}

#[allow(clippy::too_many_arguments)]
fn copy_motion_block(
    source: &Plane,
    dest: &mut Plane,
    dest_x: usize,
    dest_y: usize,
    size: usize,
    motion_h: i32,
    motion_v: i32,
    average: bool,
) {
    if source.width == 0 || source.height == 0 || source.data.is_empty() {
        let neutral = if source.width == dest.width { 0 } else { 128 };
        for row in 0..size {
            for col in 0..size {
                if dest_x + col < dest.width && dest_y + row < dest.height {
                    dest.data[(dest_y + row) * dest.width + dest_x + col] = neutral;
                }
            }
        }
        return;
    }
    let source_x = dest_x as i32 + (motion_h >> 1);
    let source_y = dest_y as i32 + (motion_v >> 1);
    let half_x = motion_h & 1 != 0;
    let half_y = motion_v & 1 != 0;
    let sample = |x: i32, y: i32| -> u8 {
        let x = x.clamp(0, source.width.saturating_sub(1) as i32) as usize;
        let y = y.clamp(0, source.height.saturating_sub(1) as i32) as usize;
        source.data[y * source.width + x]
    };
    for row in 0..size {
        for col in 0..size {
            if dest_x + col >= dest.width || dest_y + row >= dest.height {
                continue;
            }
            let x = source_x + col as i32;
            let y = source_y + row as i32;
            let a = u16::from(sample(x, y));
            let prediction = match (half_x, half_y) {
                (false, false) => a,
                (true, false) => (a + u16::from(sample(x + 1, y)) + 1) >> 1,
                (false, true) => (a + u16::from(sample(x, y + 1)) + 1) >> 1,
                (true, true) => {
                    (a + u16::from(sample(x + 1, y))
                        + u16::from(sample(x, y + 1))
                        + u16::from(sample(x + 1, y + 1))
                        + 2)
                        >> 2
                }
            } as u8;
            let index = (dest_y + row) * dest.width + dest_x + col;
            dest.data[index] = if average {
                ((u16::from(dest.data[index]) + u16::from(prediction) + 1) >> 1) as u8
            } else {
                prediction
            };
        }
    }
}

fn copy_block_to_dest(
    block: &[i32; 64],
    dest: &mut [u8],
    mut index: usize,
    scan: usize,
) -> Option<()> {
    for row in block.chunks_exact(8) {
        let output = dest.get_mut(index..index + 8)?;
        for (target, value) in output.iter_mut().zip(row) {
            *target = (*value).clamp(0, 255) as u8;
        }
        index += scan + 8;
    }
    Some(())
}

fn add_block_to_dest(
    block: &[i32; 64],
    dest: &mut [u8],
    mut index: usize,
    scan: usize,
) -> Option<()> {
    for row in block.chunks_exact(8) {
        let output = dest.get_mut(index..index + 8)?;
        for (target, value) in output.iter_mut().zip(row) {
            *target = (i32::from(*target) + value).clamp(0, 255) as u8;
        }
        index += scan + 8;
    }
    Some(())
}

fn copy_value_to_dest(value: i32, dest: &mut [u8], mut index: usize, scan: usize) -> Option<()> {
    let value = value.clamp(0, 255) as u8;
    for _ in 0..8 {
        dest.get_mut(index..index + 8)?.fill(value);
        index += scan + 8;
    }
    Some(())
}

fn add_value_to_dest(value: i32, dest: &mut [u8], mut index: usize, scan: usize) -> Option<()> {
    for _ in 0..8 {
        for target in dest.get_mut(index..index + 8)? {
            *target = (i32::from(*target) + value).clamp(0, 255) as u8;
        }
        index += scan + 8;
    }
    Some(())
}

fn idct(block: &mut [i32; 64], _max_index: usize) {
    // Integer two-pass IDCT from the attributed decoder. The constants are
    // fixed-point approximations of the MPEG-1 inverse cosine transform.
    for column in 0..8 {
        let b1 = block[32 + column];
        let b3 = block[16 + column] + block[48 + column];
        let b4 = block[40 + column] - block[24 + column];
        let tmp1 = block[8 + column] + block[56 + column];
        let tmp2 = block[24 + column] + block[40 + column];
        let b6 = block[8 + column] - block[56 + column];
        let b7 = tmp1 + tmp2;
        let m0 = block[column];
        let x4 = ((b6 * 473 - b4 * 196 + 128) >> 8) - b7;
        let x0 = x4 - (((tmp1 - tmp2) * 362 + 128) >> 8);
        let x1 = m0 - b1;
        let x2 = (((block[16 + column] - block[48 + column]) * 362 + 128) >> 8) - b3;
        let x3 = m0 + b1;
        let y3 = x1 + x2;
        let y4 = x3 + b3;
        let y5 = x1 - x2;
        let y6 = x3 - b3;
        let y7 = -x0 - ((b4 * 473 + b6 * 196 + 128) >> 8);
        block[column] = b7 + y4;
        block[8 + column] = x4 + y3;
        block[16 + column] = y5 - x0;
        block[24 + column] = y6 - y7;
        block[32 + column] = y6 + y7;
        block[40 + column] = x0 + y5;
        block[48 + column] = y3 - x4;
        block[56 + column] = y4 - b7;
    }
    for start in (0..64).step_by(8) {
        let b1 = block[start + 4];
        let b3 = block[start + 2] + block[start + 6];
        let b4 = block[start + 5] - block[start + 3];
        let tmp1 = block[start + 1] + block[start + 7];
        let tmp2 = block[start + 3] + block[start + 5];
        let b6 = block[start + 1] - block[start + 7];
        let b7 = tmp1 + tmp2;
        let m0 = block[start];
        let x4 = ((b6 * 473 - b4 * 196 + 128) >> 8) - b7;
        let x0 = x4 - (((tmp1 - tmp2) * 362 + 128) >> 8);
        let x1 = m0 - b1;
        let x2 = (((block[start + 2] - block[start + 6]) * 362 + 128) >> 8) - b3;
        let x3 = m0 + b1;
        let y3 = x1 + x2;
        let y4 = x3 + b3;
        let y5 = x1 - x2;
        let y6 = x3 - b3;
        let y7 = -x0 - ((b4 * 473 + b6 * 196 + 128) >> 8);
        block[start] = (b7 + y4 + 128) >> 8;
        block[start + 1] = (x4 + y3 + 128) >> 8;
        block[start + 2] = (y5 - x0 + 128) >> 8;
        block[start + 3] = (y6 - y7 + 128) >> 8;
        block[start + 4] = (y6 + y7 + 128) >> 8;
        block[start + 5] = (x0 + y5 + 128) >> 8;
        block[start + 6] = (y3 - x4 + 128) >> 8;
        block[start + 7] = (y4 - b7 + 128) >> 8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequence_header(width: u16, height: u16) -> Vec<u8> {
        let fields = [
            (u32::from(width), 12),
            (u32::from(height), 12),
            (1, 4), // square-pixel aspect code
            (3, 4), // 25 fps
            (1, 18),
            (1, 1), // marker
            (0, 10),
            (0, 1), // constrained-parameters flag
            (0, 1), // default intra matrix
            (0, 1), // default non-intra matrix
        ];
        let mut bits = Vec::new();
        for (value, width) in fields {
            for shift in (0..width).rev() {
                bits.push(((value >> shift) & 1) as u8);
            }
        }
        bits.chunks(8)
            .map(|chunk| chunk.iter().fold(0u8, |byte, bit| (byte << 1) | bit))
            .collect()
    }

    #[test]
    fn split_start_codes_survive_incremental_feeds() {
        let mut buffer = BitBuffer::default();
        buffer.write(&[0, 0]);
        assert_eq!(buffer.next_start_code(), None);
        buffer.write(&[1, START_SEQUENCE, 0x12]);
        assert_eq!(buffer.next_start_code(), Some(START_SEQUENCE));
    }

    #[test]
    fn ycbcr_neutral_chroma_converts_to_gray() {
        let mut frame = Frame::new(1, 1, 16, 16);
        frame.y.data[0] = 126;
        let rgb = frame.to_rgb().pixels[0];
        assert_eq!((rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF);
        assert_eq!((rgb >> 8) & 0xFF, rgb & 0xFF);
    }

    #[test]
    fn ycbcr_conversion_retains_cdi_studio_range() {
        let mut frame = Frame::new(1, 1, 16, 16);
        frame.y.data[0] = 16;
        assert_eq!(frame.to_rgb().pixels[0], 0x0010_1010);
        frame.y.data[0] = 235;
        assert_eq!(frame.to_rgb().pixels[0], 0x00EB_EBEB);
    }

    #[test]
    fn motion_compensation_clamps_edge_reads() {
        let mut source = Frame::new(16, 16, 16, 16);
        source.y.data.fill(42);
        let mut dest = Frame::new(16, 16, 16, 16);
        copy_macroblock(-31, -31, 0, 0, &source, &mut dest, false);
        assert_eq!(dest.y.data[0], 42);
    }

    #[test]
    fn repeated_sequence_header_preserves_reference_frames() {
        let mut decoder = Mpeg1VideoDecoder::default();
        let header = sequence_header(368, 176);
        decoder.feed(&header);
        decoder.decode_sequence_header().unwrap();
        decoder.has_reference_frame = true;
        decoder.frame_forward.y.data[0] = 17;
        decoder.frame_backward.y.data[0] = 29;

        decoder.buffer = BitBuffer::default();
        decoder.feed(&header);
        decoder.decode_sequence_header().unwrap();

        assert!(decoder.has_reference_frame);
        assert_eq!(decoder.frame_forward.y.data[0], 17);
        assert_eq!(decoder.frame_backward.y.data[0], 29);
        assert_eq!(decoder.sequence_headers, 2);
    }

    #[test]
    fn changed_sequence_geometry_resets_reference_frames() {
        let mut decoder = Mpeg1VideoDecoder::default();
        decoder.feed(&sequence_header(352, 240));
        decoder.decode_sequence_header().unwrap();
        decoder.has_reference_frame = true;

        decoder.buffer = BitBuffer::default();
        decoder.feed(&sequence_header(368, 176));
        decoder.decode_sequence_header().unwrap();

        assert!(!decoder.has_reference_frame);
        assert_eq!((decoder.width, decoder.height), (368, 176));
        assert_eq!(decoder.frame_forward.y.data[0], 0);
    }

    #[test]
    #[ignore = "requires CDI_MPEG1_FIXTURE pointing to a local elementary stream"]
    fn local_reference_stream_decodes() {
        let path = std::env::var("CDI_MPEG1_FIXTURE").expect("CDI_MPEG1_FIXTURE");
        let bytes = std::fs::read(path).expect("read MPEG-1 fixture");
        let mut decoder = Mpeg1VideoDecoder::default();
        let mut frames = Vec::new();
        for chunk in bytes.chunks(997) {
            decoder.feed(chunk);
            frames.extend(decoder.decode_available(10_000));
        }
        frames.extend(decoder.decode_available(10_000));
        assert!(!frames.is_empty(), "decoder errors={}", decoder.errors);
        assert!(frames
            .iter()
            .all(|frame| frame.width != 0 && frame.height != 0));
        eprintln!(
            "decoded {} frames, {} errors, {} sequence headers, {} sequence ends",
            frames.len(),
            decoder.errors,
            decoder.sequence_headers,
            decoder.sequence_ends
        );
        if let Some(path) = std::env::var_os("CDI_MPEG1_DUMP_RGB24") {
            let mut rgb = Vec::new();
            for frame in &frames {
                rgb.reserve(frame.pixels.len() * 3);
                for &pixel in &frame.pixels {
                    rgb.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
                }
            }
            std::fs::write(path, rgb).expect("write RGB24 frame dump");
        }
    }
}
