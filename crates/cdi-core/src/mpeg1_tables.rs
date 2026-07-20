// SPDX-License-Identifier: MIT
//
// Mechanically translated VLC and quantization tables from gen2brain/mpeg
// revision 27c6f084c6ca342380c99a59a6a130b3f716e9d7. See NOTICE.md.

use super::{Vlc, VlcUint};

pub(super) const VIDEO_PICTURE_RATE: [f64; 16] = [
    0.000, 23.976, 24.000, 25.000, 29.970, 30.000, 50.000, 59.940, 60.000, 0.000, 0.000, 0.000,
    0.000, 0.000, 0.000, 0.000,
];

pub(super) const VIDEO_ASPECT_RATIO: [f64; 16] = [
    0.0000, 1.0000, 0.6735, 0.7031, 0.7615, 0.8055, 0.8437, 0.8935, 0.9375, 0.9815, 1.0255, 1.0695,
    1.1250, 1.1575, 1.2015, 0.0000,
];

pub(super) const VIDEO_ZIG_ZAG: [u8; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

pub(super) const VIDEO_INTRA_QUANT_MATRIX: [u8; 64] = [
    8, 16, 19, 22, 26, 27, 29, 34, 16, 16, 22, 24, 27, 29, 34, 37, 19, 22, 26, 27, 29, 34, 34, 38,
    22, 22, 26, 27, 29, 34, 37, 40, 22, 26, 27, 29, 32, 35, 40, 48, 26, 27, 29, 32, 35, 40, 48, 58,
    26, 27, 29, 34, 38, 46, 56, 69, 27, 29, 35, 38, 46, 56, 69, 83,
];

pub(super) const VIDEO_NON_INTRA_QUANT_MATRIX: [u8; 64] = [
    16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16,
    16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16,
    16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16, 16,
];

pub(super) const VIDEO_PREMULTIPLIER_MATRIX: [u8; 64] = [
    32, 44, 42, 38, 32, 25, 17, 9, 44, 62, 58, 52, 44, 35, 24, 12, 42, 58, 55, 49, 42, 33, 23, 12,
    38, 52, 49, 44, 38, 30, 20, 10, 32, 44, 42, 38, 32, 25, 17, 9, 25, 35, 33, 30, 25, 20, 14, 7,
    17, 24, 23, 20, 17, 14, 9, 5, 9, 12, 12, 10, 9, 7, 5, 2,
];

pub(super) const VIDEO_MACROBLOCK_ADDRESS_INCREMENT: &[Vlc] = &[
    Vlc::new(1 << 1, 0),
    Vlc::new(0, 1), //   0: x
    Vlc::new(2 << 1, 0),
    Vlc::new(3 << 1, 0), //   1: 0x
    Vlc::new(4 << 1, 0),
    Vlc::new(5 << 1, 0), //   2: 00x
    Vlc::new(0, 3),
    Vlc::new(0, 2), //   3: 01x
    Vlc::new(6 << 1, 0),
    Vlc::new(7 << 1, 0), //   4: 000x
    Vlc::new(0, 5),
    Vlc::new(0, 4), //   5: 001x
    Vlc::new(8 << 1, 0),
    Vlc::new(9 << 1, 0), //   6: 0000x
    Vlc::new(0, 7),
    Vlc::new(0, 6), //   7: 0001x
    Vlc::new(10 << 1, 0),
    Vlc::new(11 << 1, 0), //   8: 0000 0x
    Vlc::new(12 << 1, 0),
    Vlc::new(13 << 1, 0), //   9: 0000 1x
    Vlc::new(14 << 1, 0),
    Vlc::new(15 << 1, 0), //  10: 0000 00x
    Vlc::new(16 << 1, 0),
    Vlc::new(17 << 1, 0), //  11: 0000 01x
    Vlc::new(18 << 1, 0),
    Vlc::new(19 << 1, 0), //  12: 0000 10x
    Vlc::new(0, 9),
    Vlc::new(0, 8), //  13: 0000 11x
    Vlc::new(-1, 0),
    Vlc::new(20 << 1, 0), //  14: 0000 000x
    Vlc::new(-1, 0),
    Vlc::new(21 << 1, 0), //  15: 0000 001x
    Vlc::new(22 << 1, 0),
    Vlc::new(23 << 1, 0), //  16: 0000 010x
    Vlc::new(0, 15),
    Vlc::new(0, 14), //  17: 0000 011x
    Vlc::new(0, 13),
    Vlc::new(0, 12), //  18: 0000 100x
    Vlc::new(0, 11),
    Vlc::new(0, 10), //  19: 0000 101x
    Vlc::new(24 << 1, 0),
    Vlc::new(25 << 1, 0), //  20: 0000 0001x
    Vlc::new(26 << 1, 0),
    Vlc::new(27 << 1, 0), //  21: 0000 0011x
    Vlc::new(28 << 1, 0),
    Vlc::new(29 << 1, 0), //  22: 0000 0100x
    Vlc::new(30 << 1, 0),
    Vlc::new(31 << 1, 0), //  23: 0000 0101x
    Vlc::new(32 << 1, 0),
    Vlc::new(-1, 0), //  24: 0000 0001 0x
    Vlc::new(-1, 0),
    Vlc::new(33 << 1, 0), //  25: 0000 0001 1x
    Vlc::new(34 << 1, 0),
    Vlc::new(35 << 1, 0), //  26: 0000 0011 0x
    Vlc::new(36 << 1, 0),
    Vlc::new(37 << 1, 0), //  27: 0000 0011 1x
    Vlc::new(38 << 1, 0),
    Vlc::new(39 << 1, 0), //  28: 0000 0100 0x
    Vlc::new(0, 21),
    Vlc::new(0, 20), //  29: 0000 0100 1x
    Vlc::new(0, 19),
    Vlc::new(0, 18), //  30: 0000 0101 0x
    Vlc::new(0, 17),
    Vlc::new(0, 16), //  31: 0000 0101 1x
    Vlc::new(0, 35),
    Vlc::new(-1, 0), //  32: 0000 0001 00x
    Vlc::new(-1, 0),
    Vlc::new(0, 34), //  33: 0000 0001 11x
    Vlc::new(0, 33),
    Vlc::new(0, 32), //  34: 0000 0011 00x
    Vlc::new(0, 31),
    Vlc::new(0, 30), //  35: 0000 0011 01x
    Vlc::new(0, 29),
    Vlc::new(0, 28), //  36: 0000 0011 10x
    Vlc::new(0, 27),
    Vlc::new(0, 26), //  37: 0000 0011 11x
    Vlc::new(0, 25),
    Vlc::new(0, 24), //  38: 0000 0100 00x
    Vlc::new(0, 23),
    Vlc::new(0, 22), //  39: 0000 0100 01x
];

pub(super) const VIDEO_MACROBLOCK_TYPE_INTRA: &[Vlc] = &[
    Vlc::new(1 << 1, 0),
    Vlc::new(0, 0x01), //   0: x
    Vlc::new(-1, 0),
    Vlc::new(0, 0x11), //   1: 0x
];

pub(super) const VIDEO_MACROBLOCK_TYPE_PREDICTIVE: &[Vlc] = &[
    Vlc::new(1 << 1, 0),
    Vlc::new(0, 0x0a), //   0: x
    Vlc::new(2 << 1, 0),
    Vlc::new(0, 0x02), //   1: 0x
    Vlc::new(3 << 1, 0),
    Vlc::new(0, 0x08), //   2: 00x
    Vlc::new(4 << 1, 0),
    Vlc::new(5 << 1, 0), //   3: 000x
    Vlc::new(6 << 1, 0),
    Vlc::new(0, 0x12), //   4: 0000x
    Vlc::new(0, 0x1a),
    Vlc::new(0, 0x01), //   5: 0001x
    Vlc::new(-1, 0),
    Vlc::new(0, 0x11), //   6: 0000 0x
];

pub(super) const VIDEO_MACROBLOCK_TYPE_B: &[Vlc] = &[
    Vlc::new(1 << 1, 0),
    Vlc::new(2 << 1, 0), //   0: x
    Vlc::new(3 << 1, 0),
    Vlc::new(4 << 1, 0), //   1: 0x
    Vlc::new(0, 0x0c),
    Vlc::new(0, 0x0e), //   2: 1x
    Vlc::new(5 << 1, 0),
    Vlc::new(6 << 1, 0), //   3: 00x
    Vlc::new(0, 0x04),
    Vlc::new(0, 0x06), //   4: 01x
    Vlc::new(7 << 1, 0),
    Vlc::new(8 << 1, 0), //   5: 000x
    Vlc::new(0, 0x08),
    Vlc::new(0, 0x0a), //   6: 001x
    Vlc::new(9 << 1, 0),
    Vlc::new(10 << 1, 0), //   7: 0000x
    Vlc::new(0, 0x1e),
    Vlc::new(0, 0x01), //   8: 0001x
    Vlc::new(-1, 0),
    Vlc::new(0, 0x11), //   9: 0000 0x
    Vlc::new(0, 0x16),
    Vlc::new(0, 0x1a), //  10: 0000 1x
];

pub(super) const VIDEO_CODE_BLOCK_PATTERN: &[Vlc] = &[
    Vlc::new(1 << 1, 0),
    Vlc::new(2 << 1, 0), //   0: x
    Vlc::new(3 << 1, 0),
    Vlc::new(4 << 1, 0), //   1: 0x
    Vlc::new(5 << 1, 0),
    Vlc::new(6 << 1, 0), //   2: 1x
    Vlc::new(7 << 1, 0),
    Vlc::new(8 << 1, 0), //   3: 00x
    Vlc::new(9 << 1, 0),
    Vlc::new(10 << 1, 0), //   4: 01x
    Vlc::new(11 << 1, 0),
    Vlc::new(12 << 1, 0), //   5: 10x
    Vlc::new(13 << 1, 0),
    Vlc::new(0, 60), //   6: 11x
    Vlc::new(14 << 1, 0),
    Vlc::new(15 << 1, 0), //   7: 000x
    Vlc::new(16 << 1, 0),
    Vlc::new(17 << 1, 0), //   8: 001x
    Vlc::new(18 << 1, 0),
    Vlc::new(19 << 1, 0), //   9: 010x
    Vlc::new(20 << 1, 0),
    Vlc::new(21 << 1, 0), //  10: 011x
    Vlc::new(22 << 1, 0),
    Vlc::new(23 << 1, 0), //  11: 100x
    Vlc::new(0, 32),
    Vlc::new(0, 16), //  12: 101x
    Vlc::new(0, 8),
    Vlc::new(0, 4), //  13: 110x
    Vlc::new(24 << 1, 0),
    Vlc::new(25 << 1, 0), //  14: 0000x
    Vlc::new(26 << 1, 0),
    Vlc::new(27 << 1, 0), //  15: 0001x
    Vlc::new(28 << 1, 0),
    Vlc::new(29 << 1, 0), //  16: 0010x
    Vlc::new(30 << 1, 0),
    Vlc::new(31 << 1, 0), //  17: 0011x
    Vlc::new(0, 62),
    Vlc::new(0, 2), //  18: 0100x
    Vlc::new(0, 61),
    Vlc::new(0, 1), //  19: 0101x
    Vlc::new(0, 56),
    Vlc::new(0, 52), //  20: 0110x
    Vlc::new(0, 44),
    Vlc::new(0, 28), //  21: 0111x
    Vlc::new(0, 40),
    Vlc::new(0, 20), //  22: 1000x
    Vlc::new(0, 48),
    Vlc::new(0, 12), //  23: 1001x
    Vlc::new(32 << 1, 0),
    Vlc::new(33 << 1, 0), //  24: 0000 0x
    Vlc::new(34 << 1, 0),
    Vlc::new(35 << 1, 0), //  25: 0000 1x
    Vlc::new(36 << 1, 0),
    Vlc::new(37 << 1, 0), //  26: 0001 0x
    Vlc::new(38 << 1, 0),
    Vlc::new(39 << 1, 0), //  27: 0001 1x
    Vlc::new(40 << 1, 0),
    Vlc::new(41 << 1, 0), //  28: 0010 0x
    Vlc::new(42 << 1, 0),
    Vlc::new(43 << 1, 0), //  29: 0010 1x
    Vlc::new(0, 63),
    Vlc::new(0, 3), //  30: 0011 0x
    Vlc::new(0, 36),
    Vlc::new(0, 24), //  31: 0011 1x
    Vlc::new(44 << 1, 0),
    Vlc::new(45 << 1, 0), //  32: 0000 00x
    Vlc::new(46 << 1, 0),
    Vlc::new(47 << 1, 0), //  33: 0000 01x
    Vlc::new(48 << 1, 0),
    Vlc::new(49 << 1, 0), //  34: 0000 10x
    Vlc::new(50 << 1, 0),
    Vlc::new(51 << 1, 0), //  35: 0000 11x
    Vlc::new(52 << 1, 0),
    Vlc::new(53 << 1, 0), //  36: 0001 00x
    Vlc::new(54 << 1, 0),
    Vlc::new(55 << 1, 0), //  37: 0001 01x
    Vlc::new(56 << 1, 0),
    Vlc::new(57 << 1, 0), //  38: 0001 10x
    Vlc::new(58 << 1, 0),
    Vlc::new(59 << 1, 0), //  39: 0001 11x
    Vlc::new(0, 34),
    Vlc::new(0, 18), //  40: 0010 00x
    Vlc::new(0, 10),
    Vlc::new(0, 6), //  41: 0010 01x
    Vlc::new(0, 33),
    Vlc::new(0, 17), //  42: 0010 10x
    Vlc::new(0, 9),
    Vlc::new(0, 5), //  43: 0010 11x
    Vlc::new(-1, 0),
    Vlc::new(60 << 1, 0), //  44: 0000 000x
    Vlc::new(61 << 1, 0),
    Vlc::new(62 << 1, 0), //  45: 0000 001x
    Vlc::new(0, 58),
    Vlc::new(0, 54), //  46: 0000 010x
    Vlc::new(0, 46),
    Vlc::new(0, 30), //  47: 0000 011x
    Vlc::new(0, 57),
    Vlc::new(0, 53), //  48: 0000 100x
    Vlc::new(0, 45),
    Vlc::new(0, 29), //  49: 0000 101x
    Vlc::new(0, 38),
    Vlc::new(0, 26), //  50: 0000 110x
    Vlc::new(0, 37),
    Vlc::new(0, 25), //  51: 0000 111x
    Vlc::new(0, 43),
    Vlc::new(0, 23), //  52: 0001 000x
    Vlc::new(0, 51),
    Vlc::new(0, 15), //  53: 0001 001x
    Vlc::new(0, 42),
    Vlc::new(0, 22), //  54: 0001 010x
    Vlc::new(0, 50),
    Vlc::new(0, 14), //  55: 0001 011x
    Vlc::new(0, 41),
    Vlc::new(0, 21), //  56: 0001 100x
    Vlc::new(0, 49),
    Vlc::new(0, 13), //  57: 0001 101x
    Vlc::new(0, 35),
    Vlc::new(0, 19), //  58: 0001 110x
    Vlc::new(0, 11),
    Vlc::new(0, 7), //  59: 0001 111x
    Vlc::new(0, 39),
    Vlc::new(0, 27), //  60: 0000 0001x
    Vlc::new(0, 59),
    Vlc::new(0, 55), //  61: 0000 0010x
    Vlc::new(0, 47),
    Vlc::new(0, 31), //  62: 0000 0011x
];

pub(super) const VIDEO_MOTION: &[Vlc] = &[
    Vlc::new(1 << 1, 0),
    Vlc::new(0, 0), //   0: x
    Vlc::new(2 << 1, 0),
    Vlc::new(3 << 1, 0), //   1: 0x
    Vlc::new(4 << 1, 0),
    Vlc::new(5 << 1, 0), //   2: 00x
    Vlc::new(0, 1),
    Vlc::new(0, -1), //   3: 01x
    Vlc::new(6 << 1, 0),
    Vlc::new(7 << 1, 0), //   4: 000x
    Vlc::new(0, 2),
    Vlc::new(0, -2), //   5: 001x
    Vlc::new(8 << 1, 0),
    Vlc::new(9 << 1, 0), //   6: 0000x
    Vlc::new(0, 3),
    Vlc::new(0, -3), //   7: 0001x
    Vlc::new(10 << 1, 0),
    Vlc::new(11 << 1, 0), //   8: 0000 0x
    Vlc::new(12 << 1, 0),
    Vlc::new(13 << 1, 0), //   9: 0000 1x
    Vlc::new(-1, 0),
    Vlc::new(14 << 1, 0), //  10: 0000 00x
    Vlc::new(15 << 1, 0),
    Vlc::new(16 << 1, 0), //  11: 0000 01x
    Vlc::new(17 << 1, 0),
    Vlc::new(18 << 1, 0), //  12: 0000 10x
    Vlc::new(0, 4),
    Vlc::new(0, -4), //  13: 0000 11x
    Vlc::new(-1, 0),
    Vlc::new(19 << 1, 0), //  14: 0000 001x
    Vlc::new(20 << 1, 0),
    Vlc::new(21 << 1, 0), //  15: 0000 010x
    Vlc::new(0, 7),
    Vlc::new(0, -7), //  16: 0000 011x
    Vlc::new(0, 6),
    Vlc::new(0, -6), //  17: 0000 100x
    Vlc::new(0, 5),
    Vlc::new(0, -5), //  18: 0000 101x
    Vlc::new(22 << 1, 0),
    Vlc::new(23 << 1, 0), //  19: 0000 0011x
    Vlc::new(24 << 1, 0),
    Vlc::new(25 << 1, 0), //  20: 0000 0100x
    Vlc::new(26 << 1, 0),
    Vlc::new(27 << 1, 0), //  21: 0000 0101x
    Vlc::new(28 << 1, 0),
    Vlc::new(29 << 1, 0), //  22: 0000 0011 0x
    Vlc::new(30 << 1, 0),
    Vlc::new(31 << 1, 0), //  23: 0000 0011 1x
    Vlc::new(32 << 1, 0),
    Vlc::new(33 << 1, 0), //  24: 0000 0100 0x
    Vlc::new(0, 10),
    Vlc::new(0, -10), //  25: 0000 0100 1x
    Vlc::new(0, 9),
    Vlc::new(0, -9), //  26: 0000 0101 0x
    Vlc::new(0, 8),
    Vlc::new(0, -8), //  27: 0000 0101 1x
    Vlc::new(0, 16),
    Vlc::new(0, -16), //  28: 0000 0011 00x
    Vlc::new(0, 15),
    Vlc::new(0, -15), //  29: 0000 0011 01x
    Vlc::new(0, 14),
    Vlc::new(0, -14), //  30: 0000 0011 10x
    Vlc::new(0, 13),
    Vlc::new(0, -13), //  31: 0000 0011 11x
    Vlc::new(0, 12),
    Vlc::new(0, -12), //  32: 0000 0100 00x
    Vlc::new(0, 11),
    Vlc::new(0, -11), //  33: 0000 0100 01x
];

pub(super) const VIDEO_DCT_SIZE_LUMINANCE: &[Vlc] = &[
    Vlc::new(1 << 1, 0),
    Vlc::new(2 << 1, 0), //   0: x
    Vlc::new(0, 1),
    Vlc::new(0, 2), //   1: 0x
    Vlc::new(3 << 1, 0),
    Vlc::new(4 << 1, 0), //   2: 1x
    Vlc::new(0, 0),
    Vlc::new(0, 3), //   3: 10x
    Vlc::new(0, 4),
    Vlc::new(5 << 1, 0), //   4: 11x
    Vlc::new(0, 5),
    Vlc::new(6 << 1, 0), //   5: 111x
    Vlc::new(0, 6),
    Vlc::new(7 << 1, 0), //   6: 1111x
    Vlc::new(0, 7),
    Vlc::new(8 << 1, 0), //   7: 1111 1x
    Vlc::new(0, 8),
    Vlc::new(-1, 0), //   8: 1111 11x
];

pub(super) const VIDEO_DCT_SIZE_CHROMINANCE: &[Vlc] = &[
    Vlc::new(1 << 1, 0),
    Vlc::new(2 << 1, 0), //   0: x
    Vlc::new(0, 0),
    Vlc::new(0, 1), //   1: 0x
    Vlc::new(0, 2),
    Vlc::new(3 << 1, 0), //   2: 1x
    Vlc::new(0, 3),
    Vlc::new(4 << 1, 0), //   3: 11x
    Vlc::new(0, 4),
    Vlc::new(5 << 1, 0), //   4: 111x
    Vlc::new(0, 5),
    Vlc::new(6 << 1, 0), //   5: 1111x
    Vlc::new(0, 6),
    Vlc::new(7 << 1, 0), //   6: 1111 1x
    Vlc::new(0, 7),
    Vlc::new(8 << 1, 0), //   7: 1111 11x
    Vlc::new(0, 8),
    Vlc::new(-1, 0), //   8: 1111 111x
];

pub(super) const VIDEO_DCT_COEFF: &[VlcUint] = &[
    VlcUint::new(1 << 1, 0),
    VlcUint::new(0, 0x0001), //   0: x
    VlcUint::new(2 << 1, 0),
    VlcUint::new(3 << 1, 0), //   1: 0x
    VlcUint::new(4 << 1, 0),
    VlcUint::new(5 << 1, 0), //   2: 00x
    VlcUint::new(6 << 1, 0),
    VlcUint::new(0, 0x0101), //   3: 01x
    VlcUint::new(7 << 1, 0),
    VlcUint::new(8 << 1, 0), //   4: 000x
    VlcUint::new(9 << 1, 0),
    VlcUint::new(10 << 1, 0), //   5: 001x
    VlcUint::new(0, 0x0002),
    VlcUint::new(0, 0x0201), //   6: 010x
    VlcUint::new(11 << 1, 0),
    VlcUint::new(12 << 1, 0), //   7: 0000x
    VlcUint::new(13 << 1, 0),
    VlcUint::new(14 << 1, 0), //   8: 0001x
    VlcUint::new(15 << 1, 0),
    VlcUint::new(0, 0x0003), //   9: 0010x
    VlcUint::new(0, 0x0401),
    VlcUint::new(0, 0x0301), //  10: 0011x
    VlcUint::new(16 << 1, 0),
    VlcUint::new(0, 0xffff), //  11: 0000 0x
    VlcUint::new(17 << 1, 0),
    VlcUint::new(18 << 1, 0), //  12: 0000 1x
    VlcUint::new(0, 0x0701),
    VlcUint::new(0, 0x0601), //  13: 0001 0x
    VlcUint::new(0, 0x0102),
    VlcUint::new(0, 0x0501), //  14: 0001 1x
    VlcUint::new(19 << 1, 0),
    VlcUint::new(20 << 1, 0), //  15: 0010 0x
    VlcUint::new(21 << 1, 0),
    VlcUint::new(22 << 1, 0), //  16: 0000 00x
    VlcUint::new(0, 0x0202),
    VlcUint::new(0, 0x0901), //  17: 0000 10x
    VlcUint::new(0, 0x0004),
    VlcUint::new(0, 0x0801), //  18: 0000 11x
    VlcUint::new(23 << 1, 0),
    VlcUint::new(24 << 1, 0), //  19: 0010 00x
    VlcUint::new(25 << 1, 0),
    VlcUint::new(26 << 1, 0), //  20: 0010 01x
    VlcUint::new(27 << 1, 0),
    VlcUint::new(28 << 1, 0), //  21: 0000 000x
    VlcUint::new(29 << 1, 0),
    VlcUint::new(30 << 1, 0), //  22: 0000 001x
    VlcUint::new(0, 0x0d01),
    VlcUint::new(0, 0x0006), //  23: 0010 000x
    VlcUint::new(0, 0x0c01),
    VlcUint::new(0, 0x0b01), //  24: 0010 001x
    VlcUint::new(0, 0x0302),
    VlcUint::new(0, 0x0103), //  25: 0010 010x
    VlcUint::new(0, 0x0005),
    VlcUint::new(0, 0x0a01), //  26: 0010 011x
    VlcUint::new(31 << 1, 0),
    VlcUint::new(32 << 1, 0), //  27: 0000 0000x
    VlcUint::new(33 << 1, 0),
    VlcUint::new(34 << 1, 0), //  28: 0000 0001x
    VlcUint::new(35 << 1, 0),
    VlcUint::new(36 << 1, 0), //  29: 0000 0010x
    VlcUint::new(37 << 1, 0),
    VlcUint::new(38 << 1, 0), //  30: 0000 0011x
    VlcUint::new(39 << 1, 0),
    VlcUint::new(40 << 1, 0), //  31: 0000 0000 0x
    VlcUint::new(41 << 1, 0),
    VlcUint::new(42 << 1, 0), //  32: 0000 0000 1x
    VlcUint::new(43 << 1, 0),
    VlcUint::new(44 << 1, 0), //  33: 0000 0001 0x
    VlcUint::new(45 << 1, 0),
    VlcUint::new(46 << 1, 0), //  34: 0000 0001 1x
    VlcUint::new(0, 0x1001),
    VlcUint::new(0, 0x0502), //  35: 0000 0010 0x
    VlcUint::new(0, 0x0007),
    VlcUint::new(0, 0x0203), //  36: 0000 0010 1x
    VlcUint::new(0, 0x0104),
    VlcUint::new(0, 0x0f01), //  37: 0000 0011 0x
    VlcUint::new(0, 0x0e01),
    VlcUint::new(0, 0x0402), //  38: 0000 0011 1x
    VlcUint::new(47 << 1, 0),
    VlcUint::new(48 << 1, 0), //  39: 0000 0000 00x
    VlcUint::new(49 << 1, 0),
    VlcUint::new(50 << 1, 0), //  40: 0000 0000 01x
    VlcUint::new(51 << 1, 0),
    VlcUint::new(52 << 1, 0), //  41: 0000 0000 10x
    VlcUint::new(53 << 1, 0),
    VlcUint::new(54 << 1, 0), //  42: 0000 0000 11x
    VlcUint::new(55 << 1, 0),
    VlcUint::new(56 << 1, 0), //  43: 0000 0001 00x
    VlcUint::new(57 << 1, 0),
    VlcUint::new(58 << 1, 0), //  44: 0000 0001 01x
    VlcUint::new(59 << 1, 0),
    VlcUint::new(60 << 1, 0), //  45: 0000 0001 10x
    VlcUint::new(61 << 1, 0),
    VlcUint::new(62 << 1, 0), //  46: 0000 0001 11x
    VlcUint::new(-1, 0),
    VlcUint::new(63 << 1, 0), //  47: 0000 0000 000x
    VlcUint::new(64 << 1, 0),
    VlcUint::new(65 << 1, 0), //  48: 0000 0000 001x
    VlcUint::new(66 << 1, 0),
    VlcUint::new(67 << 1, 0), //  49: 0000 0000 010x
    VlcUint::new(68 << 1, 0),
    VlcUint::new(69 << 1, 0), //  50: 0000 0000 011x
    VlcUint::new(70 << 1, 0),
    VlcUint::new(71 << 1, 0), //  51: 0000 0000 100x
    VlcUint::new(72 << 1, 0),
    VlcUint::new(73 << 1, 0), //  52: 0000 0000 101x
    VlcUint::new(74 << 1, 0),
    VlcUint::new(75 << 1, 0), //  53: 0000 0000 110x
    VlcUint::new(76 << 1, 0),
    VlcUint::new(77 << 1, 0), //  54: 0000 0000 111x
    VlcUint::new(0, 0x000b),
    VlcUint::new(0, 0x0802), //  55: 0000 0001 000x
    VlcUint::new(0, 0x0403),
    VlcUint::new(0, 0x000a), //  56: 0000 0001 001x
    VlcUint::new(0, 0x0204),
    VlcUint::new(0, 0x0702), //  57: 0000 0001 010x
    VlcUint::new(0, 0x1501),
    VlcUint::new(0, 0x1401), //  58: 0000 0001 011x
    VlcUint::new(0, 0x0009),
    VlcUint::new(0, 0x1301), //  59: 0000 0001 100x
    VlcUint::new(0, 0x1201),
    VlcUint::new(0, 0x0105), //  60: 0000 0001 101x
    VlcUint::new(0, 0x0303),
    VlcUint::new(0, 0x0008), //  61: 0000 0001 110x
    VlcUint::new(0, 0x0602),
    VlcUint::new(0, 0x1101), //  62: 0000 0001 111x
    VlcUint::new(78 << 1, 0),
    VlcUint::new(79 << 1, 0), //  63: 0000 0000 0001x
    VlcUint::new(80 << 1, 0),
    VlcUint::new(81 << 1, 0), //  64: 0000 0000 0010x
    VlcUint::new(82 << 1, 0),
    VlcUint::new(83 << 1, 0), //  65: 0000 0000 0011x
    VlcUint::new(84 << 1, 0),
    VlcUint::new(85 << 1, 0), //  66: 0000 0000 0100x
    VlcUint::new(86 << 1, 0),
    VlcUint::new(87 << 1, 0), //  67: 0000 0000 0101x
    VlcUint::new(88 << 1, 0),
    VlcUint::new(89 << 1, 0), //  68: 0000 0000 0110x
    VlcUint::new(90 << 1, 0),
    VlcUint::new(91 << 1, 0), //  69: 0000 0000 0111x
    VlcUint::new(0, 0x0a02),
    VlcUint::new(0, 0x0902), //  70: 0000 0000 1000x
    VlcUint::new(0, 0x0503),
    VlcUint::new(0, 0x0304), //  71: 0000 0000 1001x
    VlcUint::new(0, 0x0205),
    VlcUint::new(0, 0x0107), //  72: 0000 0000 1010x
    VlcUint::new(0, 0x0106),
    VlcUint::new(0, 0x000f), //  73: 0000 0000 1011x
    VlcUint::new(0, 0x000e),
    VlcUint::new(0, 0x000d), //  74: 0000 0000 1100x
    VlcUint::new(0, 0x000c),
    VlcUint::new(0, 0x1a01), //  75: 0000 0000 1101x
    VlcUint::new(0, 0x1901),
    VlcUint::new(0, 0x1801), //  76: 0000 0000 1110x
    VlcUint::new(0, 0x1701),
    VlcUint::new(0, 0x1601), //  77: 0000 0000 1111x
    VlcUint::new(92 << 1, 0),
    VlcUint::new(93 << 1, 0), //  78: 0000 0000 0001 0x
    VlcUint::new(94 << 1, 0),
    VlcUint::new(95 << 1, 0), //  79: 0000 0000 0001 1x
    VlcUint::new(96 << 1, 0),
    VlcUint::new(97 << 1, 0), //  80: 0000 0000 0010 0x
    VlcUint::new(98 << 1, 0),
    VlcUint::new(99 << 1, 0), //  81: 0000 0000 0010 1x
    VlcUint::new(100 << 1, 0),
    VlcUint::new(101 << 1, 0), //  82: 0000 0000 0011 0x
    VlcUint::new(102 << 1, 0),
    VlcUint::new(103 << 1, 0), //  83: 0000 0000 0011 1x
    VlcUint::new(0, 0x001f),
    VlcUint::new(0, 0x001e), //  84: 0000 0000 0100 0x
    VlcUint::new(0, 0x001d),
    VlcUint::new(0, 0x001c), //  85: 0000 0000 0100 1x
    VlcUint::new(0, 0x001b),
    VlcUint::new(0, 0x001a), //  86: 0000 0000 0101 0x
    VlcUint::new(0, 0x0019),
    VlcUint::new(0, 0x0018), //  87: 0000 0000 0101 1x
    VlcUint::new(0, 0x0017),
    VlcUint::new(0, 0x0016), //  88: 0000 0000 0110 0x
    VlcUint::new(0, 0x0015),
    VlcUint::new(0, 0x0014), //  89: 0000 0000 0110 1x
    VlcUint::new(0, 0x0013),
    VlcUint::new(0, 0x0012), //  90: 0000 0000 0111 0x
    VlcUint::new(0, 0x0011),
    VlcUint::new(0, 0x0010), //  91: 0000 0000 0111 1x
    VlcUint::new(104 << 1, 0),
    VlcUint::new(105 << 1, 0), //  92: 0000 0000 0001 00x
    VlcUint::new(106 << 1, 0),
    VlcUint::new(107 << 1, 0), //  93: 0000 0000 0001 01x
    VlcUint::new(108 << 1, 0),
    VlcUint::new(109 << 1, 0), //  94: 0000 0000 0001 10x
    VlcUint::new(110 << 1, 0),
    VlcUint::new(111 << 1, 0), //  95: 0000 0000 0001 11x
    VlcUint::new(0, 0x0028),
    VlcUint::new(0, 0x0027), //  96: 0000 0000 0010 00x
    VlcUint::new(0, 0x0026),
    VlcUint::new(0, 0x0025), //  97: 0000 0000 0010 01x
    VlcUint::new(0, 0x0024),
    VlcUint::new(0, 0x0023), //  98: 0000 0000 0010 10x
    VlcUint::new(0, 0x0022),
    VlcUint::new(0, 0x0021), //  99: 0000 0000 0010 11x
    VlcUint::new(0, 0x0020),
    VlcUint::new(0, 0x010e), // 100: 0000 0000 0011 00x
    VlcUint::new(0, 0x010d),
    VlcUint::new(0, 0x010c), // 101: 0000 0000 0011 01x
    VlcUint::new(0, 0x010b),
    VlcUint::new(0, 0x010a), // 102: 0000 0000 0011 10x
    VlcUint::new(0, 0x0109),
    VlcUint::new(0, 0x0108), // 103: 0000 0000 0011 11x
    VlcUint::new(0, 0x0112),
    VlcUint::new(0, 0x0111), // 104: 0000 0000 0001 000x
    VlcUint::new(0, 0x0110),
    VlcUint::new(0, 0x010f), // 105: 0000 0000 0001 001x
    VlcUint::new(0, 0x0603),
    VlcUint::new(0, 0x1002), // 106: 0000 0000 0001 010x
    VlcUint::new(0, 0x0f02),
    VlcUint::new(0, 0x0e02), // 107: 0000 0000 0001 011x
    VlcUint::new(0, 0x0d02),
    VlcUint::new(0, 0x0c02), // 108: 0000 0000 0001 100x
    VlcUint::new(0, 0x0b02),
    VlcUint::new(0, 0x1f01), // 109: 0000 0000 0001 101x
    VlcUint::new(0, 0x1e01),
    VlcUint::new(0, 0x1d01), // 110: 0000 0000 0001 110x
    VlcUint::new(0, 0x1c01),
    VlcUint::new(0, 0x1b01), // 111: 0000 0000 0001 111x
];
