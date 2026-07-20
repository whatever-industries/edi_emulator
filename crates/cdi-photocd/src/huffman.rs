//! Canonical Huffman tables and bit-stream decoder for 4Base/16Base
//! residuals (spec Section IV.3 Figs IV.17–20).
//!
//! The four classes are fixed tables: each is a list of
//! `(code_length, signed_residual_value)` pairs in canonical order.
//! The Huffman class is selected from bits 6–5 of the IPA byte.
//! Values are signed residuals in range −15..+15 (range is per spec,
//! ignoring the "zero run" encoding used internally).
//!
//! Bit-stream layout is MSB-first. A code equal to all-ones at `max_len`
//! is the end-of-row marker: the rest of the row is padded with zeros.

/// (code_length_in_bits, residual_value)
const CLASS1: &[(u8, i16)] = &[
    (1, 0),
    (4, 1),
    (4, -1),
    (5, 2),
    (5, -2),
    (6, 3),
    (6, -3),
    (7, 4),
    (7, -4),
    (8, 5),
    (8, -5),
    (9, 6),
    (9, -6),
    (10, 7),
    (10, -7),
    (11, 8),
    (11, -8),
    (12, 9),
    (12, -9),
    (12, 10),
    (12, -10),
    (13, 11),
    (13, -11),
    (13, 12),
    (13, -12),
    (14, 13),
    (14, -13),
    (14, 14),
    (14, -14),
    (14, 15),
    (14, -15),
];

const CLASS2: &[(u8, i16)] = &[
    (2, 0),
    (3, 1),
    (3, -1),
    (4, 2),
    (4, -2),
    (5, 3),
    (5, -3),
    (6, 4),
    (6, -4),
    (7, 5),
    (7, -5),
    (8, 6),
    (8, -6),
    (9, 7),
    (9, -7),
    (10, 8),
    (10, -8),
    (11, 9),
    (11, -9),
    (12, 10),
    (12, -10),
    (13, 11),
    (13, -11),
    (14, 12),
    (14, -12),
    (14, 13),
    (14, -13),
    (14, 14),
    (14, -14),
    (14, 15),
    (14, -15),
];

const CLASS3: &[(u8, i16)] = &[
    (3, 0),
    (4, 1),
    (4, -1),
    (4, 2),
    (4, -2),
    (5, 3),
    (5, -3),
    (5, 4),
    (5, -4),
    (6, 5),
    (6, -5),
    (7, 6),
    (7, -6),
    (8, 7),
    (8, -7),
    (9, 8),
    (9, -8),
    (10, 9),
    (10, -9),
    (11, 10),
    (11, -10),
    (12, 11),
    (12, -11),
    (13, 12),
    (13, -12),
    (14, 13),
    (14, -13),
    (14, 14),
    (14, -14),
    (14, 15),
    (14, -15),
];

const CLASS4: &[(u8, i16)] = &[
    (4, 0),
    (4, 1),
    (4, -1),
    (5, 2),
    (5, -2),
    (5, 3),
    (5, -3),
    (6, 4),
    (6, -4),
    (6, 5),
    (6, -5),
    (7, 6),
    (7, -6),
    (8, 7),
    (8, -7),
    (9, 8),
    (9, -8),
    (10, 9),
    (10, -9),
    (11, 10),
    (11, -10),
    (12, 11),
    (12, -11),
    (13, 12),
    (13, -12),
    (13, 13),
    (13, -13),
    (14, 14),
    (14, -14),
    (14, 15),
    (14, -15),
];

const CLASSES: [&[(u8, i16)]; 4] = [CLASS1, CLASS2, CLASS3, CLASS4];

pub const MAX_CODE_LEN: u8 = 14;

/// Sentinel residual value meaning "end-of-row" (>= 256, never a real residual).
pub const EOL: i16 = i16::MIN;

/// A decoded Huffman table for one class. Lookup is indexed by the next
/// `MAX_CODE_LEN` bits of the stream; each entry is `(value, nbits_used)`.
/// The lookup table has 2^MAX_CODE_LEN = 16384 entries (32 KB of i32) — fine.
pub struct HuffmanTable {
    lut: Vec<(i16, u8)>, // index: 14-bit window; (value, code_length)
}

impl HuffmanTable {
    pub fn for_class(class: u8) -> Self {
        assert!((1..=4).contains(&class));
        build_table(CLASSES[class as usize - 1])
    }

    /// Peek `MAX_CODE_LEN` bits at the current bit offset of `stream` and
    /// return `(value, nbits)`. `nbits` is how many bits the code
    /// consumed — caller advances the bit cursor.
    #[inline]
    pub fn lookup(&self, window: u16) -> (i16, u8) {
        self.lut[window as usize]
    }
}

fn build_table(codes: &[(u8, i16)]) -> HuffmanTable {
    let max_len = MAX_CODE_LEN;
    let width = 1usize << max_len;
    let mut lut = vec![(0i16, 0u8); width];

    let mut code: u32 = 0;
    let mut prev_len: u8 = 0;

    for &(n_bits, value) in codes {
        if n_bits > prev_len {
            code <<= n_bits - prev_len;
        }
        // Fill every LUT entry whose high bits match this code.
        let shift = max_len - n_bits;
        let base = (code as usize) << shift;
        let span = 1usize << shift;
        for j in 0..span {
            lut[base + j] = (value, n_bits);
        }
        code += 1;
        prev_len = n_bits;
    }

    // End-of-row marker: all-ones at max_len. Fill just that single entry
    // (since it already has n_bits=max_len, no span).
    let eol_idx = (1usize << max_len) - 1;
    lut[eol_idx] = (EOL, max_len);

    HuffmanTable { lut }
}

/// MSB-first bit reader over a byte slice.
pub struct BitStream<'a> {
    data: &'a [u8],
    pos_bits: usize,
    total_bits: usize,
}

impl<'a> BitStream<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos_bits: 0,
            total_bits: data.len() * 8,
        }
    }

    /// Peek up to 14 bits at current position, right-aligned (MSB first).
    /// Returns 0-padded on EOF.
    #[inline]
    pub fn peek14(&self) -> u16 {
        let mut acc: u32 = 0;
        let mut filled = 0u32;
        let mut bit = self.pos_bits;
        while filled < 14 {
            if bit >= self.total_bits {
                acc <<= 14 - filled;
                return acc as u16;
            }
            let byte = self.data[bit >> 3] as u32;
            let bit_off = 7 - (bit & 7) as u32;
            acc = (acc << 1) | ((byte >> bit_off) & 1);
            bit += 1;
            filled += 1;
        }
        acc as u16
    }

    #[inline]
    pub fn skip(&mut self, n: u8) {
        self.pos_bits = (self.pos_bits + n as usize).min(self.total_bits);
    }

    #[inline]
    pub fn at_end(&self) -> bool {
        self.pos_bits >= self.total_bits
    }
}

/// Decode exactly `row_width` signed residuals from `bs` using `table`.
/// Rows end early on the EOL marker (remaining values are 0).
pub fn decode_row(bs: &mut BitStream<'_>, table: &HuffmanTable, row_width: usize, out: &mut [i16]) {
    debug_assert!(out.len() >= row_width);
    // Zero-initialize requested slice.
    for v in out.iter_mut().take(row_width) {
        *v = 0;
    }
    let mut i = 0;
    while i < row_width {
        if bs.at_end() {
            return;
        }
        let window = bs.peek14();
        let (val, nbits) = table.lookup(window);
        if nbits == 0 {
            // No code matched — defensive (shouldn't happen with a complete
            // canonical table). Skip one bit and continue.
            bs.skip(1);
            continue;
        }
        bs.skip(nbits);
        if val == EOL {
            return;
        }
        out[i] = val;
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class1_shortest_code_is_zero() {
        // Class 1 has (1, 0) as the shortest code; a stream of 0-bits
        // should decode to all zeros until EOF.
        let t = HuffmanTable::for_class(1);
        let data = vec![0u8; 16];
        let mut bs = BitStream::new(&data);
        let mut out = [99i16; 32];
        decode_row(&mut bs, &t, 32, &mut out);
        assert!(out.iter().all(|&v| v == 0));
    }

    #[test]
    fn class2_all_ones_is_eol() {
        let t = HuffmanTable::for_class(2);
        // All 1 bits → first 14-bit window = EOL → row ends immediately.
        let data = vec![0xFFu8; 16];
        let mut bs = BitStream::new(&data);
        let mut out = [99i16; 8];
        decode_row(&mut bs, &t, 8, &mut out);
        // Row was zero-initialized then EOL → all zeros.
        assert!(out.iter().all(|&v| v == 0));
    }

    #[test]
    fn bitstream_reads_msb_first() {
        // 0b10110010, 0b11000000
        let data = [0b1011_0010u8, 0b1100_0000];
        let mut bs = BitStream::new(&data);
        // Peek 14: 10110010_110000 = 0b10110010110000 = 11568
        assert_eq!(bs.peek14(), 0b10110010110000);
        bs.skip(3); // consume 101 → 10110
                    // now pos=3, window = 10010_110000_00 = 0b10010110000000 = 9600
        assert_eq!(bs.peek14(), 0b10010110000000);
    }
}
