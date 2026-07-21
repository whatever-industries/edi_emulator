// SPDX-License-Identifier: GPL-3.0-or-later
//! ECMA-130 sector scrambling.
//!
//! Data sectors are scrambled on disc with a 15-bit LFSR (x^15 + x + 1,
//! initial state 1, LSB-first per byte) over bytes 12..2352; the 12-byte
//! sync pattern is never scrambled and the LFSR restarts every sector.
//! Drives descramble data sectors transparently, so BIN rips of data tracks
//! are already descrambled — but audio-track rips are bit-exact, so CD-i
//! Ready data hidden in a track-1 pregap arrives still scrambled.

use std::sync::OnceLock;

const SCRAMBLED_LEN: usize = 2352 - 12;

pub fn scramble_table() -> &'static [u8; SCRAMBLED_LEN] {
    static TABLE: OnceLock<[u8; SCRAMBLED_LEN]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut state: u16 = 1;
        let mut table = [0u8; SCRAMBLED_LEN];
        for byte in table.iter_mut() {
            let mut val = 0u8;
            for bit in 0..8 {
                let out = (state & 1) as u8;
                val |= out << bit;
                let feedback = (state & 1) ^ ((state >> 1) & 1);
                state = ((state >> 1) | (feedback << 14)) & 0x7FFF;
            }
            *byte = val;
        }
        table
    })
}

/// XOR bytes 12..2352 with the scramble sequence (an involution: applying
/// twice restores the original).
pub fn descramble_in_place(sector: &mut [u8]) {
    let table = scramble_table();
    for (byte, key) in sector[12..].iter_mut().zip(table.iter()) {
        *byte ^= key;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_starts_with_known_sequence() {
        // First bytes of the ECMA-130 scramble sequence.
        let t = scramble_table();
        assert_eq!(&t[..4], &[0x01, 0x80, 0x00, 0x60]);
    }

    #[test]
    fn descramble_is_involution() {
        let mut sector = [0xA5u8; 2352];
        let original = sector;
        descramble_in_place(&mut sector);
        assert_ne!(sector[100], original[100]);
        // Sync bytes untouched.
        assert_eq!(&sector[..12], &original[..12]);
        descramble_in_place(&mut sector);
        assert_eq!(sector[..], original[..]);
    }
}
