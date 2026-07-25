// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only, bounded machine diagnostics.

use crate::cdic::CdicDiagnosticSnapshot;
use crate::dvc::{DvcRegisterSnapshot, DvcStats};
use crate::mcd212::DisplayGeometry;
use crate::slave::SlaveDiagnosticSnapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct CpuDiagnosticSnapshot {
    pub d: [u32; 8],
    pub a: [u32; 8],
    pub pc: u32,
    pub sr: u16,
    pub stopped: bool,
    pub pending_ipl: u8,
    pub cycles: u64,
    pub exceptions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct InterruptDiagnosticSnapshot {
    pub pending_ipl: u8,
    pub slave_in2: bool,
    pub cdic_in4: bool,
    pub dvc_in5: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct DmaChannelDiagnosticSnapshot {
    pub status: u8,
    pub channel_control: u8,
    pub memory_address: u32,
    pub transfer_count: u16,
    pub operation_control: u8,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct DmaDiagnosticSnapshot {
    pub cdic_channel: DmaChannelDiagnosticSnapshot,
    pub dvc_channel: DmaChannelDiagnosticSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct Mcd212DiagnosticSnapshot {
    pub geometry: DisplayGeometry,
    pub csrw: [u16; 2],
    pub csrr: [u8; 2],
    pub dcr: [u16; 2],
    pub vsr: [u16; 2],
    pub ddr: [u16; 2],
    pub dcp: [u16; 2],
    pub dca: [u32; 2],
    pub image_coding_method: u32,
    pub transparency_control: u32,
    pub plane_order: u32,
    pub dyuv_absolute_start: [u32; 2],
    pub cursor_position: u32,
    pub cursor_control: u32,
    pub frame_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct DisplayProvenanceSnapshot {
    /// CDIC buffer RAM after sector delivery.
    pub cdic_buffer_hash: u64,
    /// Drawmap/plane RAM consumed by each MCD212 path.
    pub plane_a_hash: u64,
    pub plane_b_hash: u64,
    /// Composed hardware raster before frontend presentation.
    pub raster_hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct MachineDiagnosticSnapshot {
    pub cpu: CpuDiagnosticSnapshot,
    pub interrupts: InterruptDiagnosticSnapshot,
    pub dma: DmaDiagnosticSnapshot,
    pub cdic: CdicDiagnosticSnapshot,
    pub slave: SlaveDiagnosticSnapshot,
    pub mcd212: Mcd212DiagnosticSnapshot,
    pub display_provenance: DisplayProvenanceSnapshot,
    pub dvc: Option<DvcStats>,
    pub dvc_registers: Option<DvcRegisterSnapshot>,
    pub disc_inserted: bool,
}

/// Significant transitions sampled only while diagnostics are enabled.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "savestate", serde(tag = "kind", rename_all = "kebab-case"))]
pub enum MachineDiagnosticEvent {
    Frame {
        cycle: u64,
        frame: u64,
        geometry: DisplayGeometry,
        plane_a_hash: u64,
        plane_b_hash: u64,
        raster_hash: u64,
    },
    DiscPosition {
        cycle: u64,
        mode: u8,
        lba: u32,
    },
    CdicState {
        cycle: u64,
        command: u16,
        audio_buffer: u16,
        x_buffer: u16,
        z_buffer: u16,
        data_buffer: u16,
        interrupt_asserted: bool,
    },
    DvcCounters {
        cycle: u64,
        demux_errors: u64,
        video_errors: u64,
        audio_errors: u64,
        video_underflows: u64,
        audio_underflows: u64,
        stream_errors: u64,
    },
    HostReset {
        cycle: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DiagnosticProbe {
    pub frame: u64,
    pub cdic_mode: u8,
    pub cdic_lba: u32,
    pub cdic_state: [u16; 5],
    pub cdic_interrupt: bool,
    pub dvc_errors: [u64; 6],
}
