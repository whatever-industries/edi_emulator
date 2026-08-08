// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only CD-RTOS/OS-9 runtime classification helpers.
//!
//! These helpers deliberately stop at structures whose layout is specified
//! independently of a particular kernel build.  Process descriptors and the
//! System Globals table are kernel-version-specific and must not be decoded
//! until a matching ROM layout has been validated.

use std::ops::Range;

/// Size returned by `I$GetStat SS_Opt` for the CD-i CDFM and UCM managers.
pub const SS_OPT_PACKET_LEN: usize = 128;

/// Device represented by a CD-i `SS_Opt` option packet.
///
/// TN 072 identifies inherited paths by these fields.  The byte offsets are
/// packet-relative: `PD_DTP` 0, `PD_CDFC` 1, and UCM `PD_FC` 28.  The same
/// layout is specified by Green Book R2 Figures VII.33 and VII.40.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdiPathKind {
    CdfmCd,
    CdfmAudio,
    Ucm {
        keyboard: bool,
        pointer: bool,
        video: bool,
    },
    Other {
        device_type: u8,
        function_class: u8,
        function_capabilities: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SsOptPacketTooShort {
    pub actual: usize,
}

/// Classify one complete `SS_Opt` packet without retaining guest bytes.
pub fn classify_ss_opt(packet: &[u8]) -> Result<CdiPathKind, SsOptPacketTooShort> {
    if packet.len() < SS_OPT_PACKET_LEN {
        return Err(SsOptPacketTooShort {
            actual: packet.len(),
        });
    }

    let device_type = packet[0];
    let function_class = packet[1];
    let function_capabilities = packet[28];
    Ok(match (device_type, function_class) {
        (5, 0) => CdiPathKind::CdfmCd,
        (5, 1) => CdiPathKind::CdfmAudio,
        (6, _) => CdiPathKind::Ucm {
            keyboard: function_capabilities & 1 != 0,
            pointer: function_capabilities & 2 != 0,
            video: function_capabilities & 4 != 0,
        },
        _ => CdiPathKind::Other {
            device_type,
            function_class,
            function_capabilities,
        },
    })
}

/// Path numbers inherited by TN 072's `os9forkc()`/`chainc()` argument.
///
/// The argument is a count: a child inherits path numbers `0` through `N-1`.
/// It is not an arbitrary list of path numbers.
pub fn inherited_path_numbers(count: u16) -> Range<u16> {
    0..count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_cdfm_cd_and_audio_packets() {
        let mut packet = [0; SS_OPT_PACKET_LEN];
        packet[0] = 5;
        assert_eq!(classify_ss_opt(&packet), Ok(CdiPathKind::CdfmCd));
        packet[1] = 1;
        assert_eq!(classify_ss_opt(&packet), Ok(CdiPathKind::CdfmAudio));
    }

    #[test]
    fn classifies_ucm_capability_bits() {
        let mut packet = [0; SS_OPT_PACKET_LEN];
        packet[0] = 6;
        packet[28] = 1 | 4;
        assert_eq!(
            classify_ss_opt(&packet),
            Ok(CdiPathKind::Ucm {
                keyboard: true,
                pointer: false,
                video: true,
            })
        );
    }

    #[test]
    fn rejects_partial_packets() {
        assert_eq!(
            classify_ss_opt(&[0; SS_OPT_PACKET_LEN - 1]),
            Err(SsOptPacketTooShort {
                actual: SS_OPT_PACKET_LEN - 1,
            })
        );
    }

    #[test]
    fn inherited_paths_are_a_contiguous_prefix() {
        assert_eq!(inherited_path_numbers(0).collect::<Vec<_>>(), vec![]);
        assert_eq!(
            inherited_path_numbers(4).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
    }
}
