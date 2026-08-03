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
    pub dvc_in4: bool,
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
    /// Fields that sampled more than one MCD251 picture generation.
    pub mixed_external_generation_fields: u64,
}

/// Green Book VII.2 Play Control List state decoded from guest RAM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct PclDiagnosticSnapshot {
    pub address: u32,
    pub control: u8,
    pub submode: u8,
    pub coding: u8,
    pub signal: u16,
    pub next: u32,
    pub buffer: u32,
    /// Green Book VII.2 defines this value in sectors, not bytes.
    pub buffer_size: u32,
    pub error_buffer: u32,
    pub count: u32,
}

/// Green Book VII.2 Play Control Block state decoded from guest RAM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
pub struct PcbDiagnosticSnapshot {
    pub address: u32,
    pub status: u16,
    pub signal: u16,
    /// Real-time records left to play, where EOR marks a record boundary.
    pub records_remaining: u32,
    pub channel_mask: u32,
    pub audio_channel_mask: u16,
    pub video_cil: u32,
    pub audio_cil: u32,
    pub data_cil: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "savestate", serde(rename_all = "kebab-case"))]
pub enum PcbTransition {
    Discovered,
    RecordsDecremented,
    RecordsReachedZero,
    RecordsIncreased,
    Reconfigured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "savestate", serde(rename_all = "kebab-case"))]
pub enum PclDataKind {
    Unknown,
    Video,
    Audio,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "savestate", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "savestate", serde(rename_all = "kebab-case"))]
pub enum PclTransition {
    Discovered,
    ProducerAdvanced,
    BufferFull,
    ConsumerReleased,
    Reconfigured,
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
        #[cfg_attr(feature = "savestate", serde(default))]
        selected_file: u16,
        #[cfg_attr(feature = "savestate", serde(default))]
        selected_channels: u32,
        #[cfg_attr(feature = "savestate", serde(default))]
        audio_channel: u16,
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
    /// A low-frequency VMPEG play/pause/end milestone with cumulative
    /// counters. Long title runs can derive per-play deltas without recording
    /// MPEG payloads or sampling free-running state every instruction.
    DvcMilestone {
        cycle: u64,
        dclk: u32,
        stats: Box<DvcStats>,
        raster_hash: u64,
    },
    /// A guest-visible VMPEG register or transport-state transition.
    ///
    /// Free-running DCLK/timer values do not trigger these events, but their
    /// values are included when another state change is observed. This keeps
    /// long diagnostic runs bounded while preserving the clock relationship
    /// at each native-driver interaction.
    DvcState {
        cycle: u64,
        registers: DvcRegisterSnapshot,
    },
    /// One byte written by the native driver to a VMPEG control register.
    ///
    /// Recording writes separately from sampled status makes it possible to
    /// compare guest command sequences across CPU timing models without
    /// treating periodic VSYNC/timer acknowledges or MPEG payload writes as
    /// an immediate divergence.
    DvcRegisterWrite {
        cycle: u64,
        address: u32,
        value: u8,
    },
    /// One byte written by guest software to an SCC68070 DMA register.
    ///
    /// The register offset is relative to the on-chip block at `$80000000`.
    /// Retaining the CPU PC lets timing comparisons distinguish a changed
    /// native-driver decision from a device-side transfer discrepancy.
    DmaRegisterWrite {
        cycle: u64,
        cpu_pc: u32,
        channel: u8,
        register_offset: u32,
        value: u8,
    },
    DmaTransfer {
        cycle: u64,
        /// Zero is CDIC/main-memory DMA; one is main-memory/VMPEG DMA.
        channel: u8,
        memory_address: u32,
        bytes: u32,
        /// CDIC buffer offset for channel zero; VMPEG target for channel one
        /// (one video, two audio).
        device_address_or_target: u32,
        to_memory: bool,
        completed: bool,
        payload_hash: u64,
        /// Hash of the bytes submitted to VMPEG. For a 2,324-byte video
        /// sector this excludes the 12-byte pack prefix; otherwise it is the
        /// full transfer hash.
        transport_payload_hash: u64,
        pcl_addresses: Vec<u32>,
    },
    /// Guest CPU writes to a main-memory range most recently filled by CDIC
    /// DMA. This is diagnostic provenance only: it records hashes and bounds,
    /// never the commercial payload itself.
    GuestMemoryWrite {
        cycle: u64,
        memory_address: u32,
        bytes: u32,
        changed_bytes: u32,
        before_hash: u64,
        after_hash: u64,
        source_dma_address: u32,
        source_dma_bytes: u32,
        pcl_addresses: Vec<u32>,
    },
    /// A bounded test harness deliberately changed guest RAM.
    ///
    /// This is never produced by emulated software. It makes synthetic
    /// native-driver experiments auditable without storing guest payloads in
    /// the diagnostic JSON.
    DiagnosticRamPatch {
        cycle: u64,
        memory_address: u32,
        bytes: u32,
        changed_bytes: u32,
        before_hash: u64,
        after_hash: u64,
    },
    PclState {
        cycle: u64,
        /// CPU program counter immediately after the instruction that made
        /// this transition observable.
        cpu_pc: u32,
        transition: PclTransition,
        data_kind: PclDataKind,
        channel: Option<u8>,
        pcb_address: Option<u32>,
        cil_address: Option<u32>,
        pcl: PclDiagnosticSnapshot,
    },
    /// Guest CDFM play-control state associated with a discovered PCL chain.
    ///
    /// Green Book R2 VII.2 defines the layout. The worked real-time-file
    /// example in the Philips master Disc Building Utility, pp. 3-64--3-65,
    /// confirms that `PCB_Rec` counts EOR-delimited records rather than
    /// sectors. These events are observational and do not implement CDFM.
    PcbState {
        cycle: u64,
        /// CPU program counter immediately after the instruction that made
        /// this transition observable.
        cpu_pc: u32,
        transition: PcbTransition,
        pcb: PcbDiagnosticSnapshot,
    },
    /// Guest software stored the address of a discovered PCL in RAM.
    ///
    /// This exposes producer/consumer cursor movement without depending on
    /// title-specific symbols or firmware addresses.
    PclPointerWrite {
        cycle: u64,
        cpu_pc: u32,
        memory_address: u32,
        pcl_address: u32,
        changed: bool,
    },
    PclOverwriteRisk {
        cycle: u64,
        pcl_address: u32,
        buffer: u32,
        buffer_size: u32,
        count: u32,
        dma_address: u32,
        dma_bytes: u32,
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
    pub cdic_selected_file: u16,
    pub cdic_selected_channels: u32,
    pub cdic_audio_channel: u16,
    pub cdic_interrupt: bool,
    pub dvc_errors: [u64; 6],
    pub dvc_milestones: [u64; 12],
    pub dvc_state: Option<DvcRegisterSnapshot>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RamDiagnosticRegion<'a> {
    pub base: u32,
    pub bytes: &'a [u8],
}

#[derive(Debug, Clone, Copy)]
struct PclContext {
    data_kind: PclDataKind,
    channel: Option<u8>,
    pcb_address: Option<u32>,
    cil_address: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
struct WatchedPcl {
    snapshot: PclDiagnosticSnapshot,
    context: PclContext,
    context_retry_done: bool,
}

#[derive(Debug, Clone, Copy)]
struct WatchedPcb {
    snapshot: PcbDiagnosticSnapshot,
}

/// Diagnostic-only ownership tracker anchored to actual DMA buffer ranges.
#[derive(Debug, Default)]
pub(crate) struct PclOwnershipTracker {
    watched: Vec<WatchedPcl>,
    watched_pcbs: Vec<WatchedPcb>,
}

impl PclOwnershipTracker {
    pub fn observe_cdic_dma(
        &mut self,
        cycle: u64,
        regions: &[RamDiagnosticRegion<'_>],
        address: u32,
        bytes: u32,
    ) -> (Vec<u32>, Vec<MachineDiagnosticEvent>) {
        let mut events = Vec::new();
        let mut matches = self.matching_buffers(address, bytes);
        for pcl_address in matches.iter().copied() {
            let watch = self
                .watched
                .iter()
                .find(|watch| watch.snapshot.address == pcl_address)
                .expect("matching PCL must be watched");
            if watch.snapshot.control & 1 != 0 {
                events.push(overwrite_event(cycle, watch.snapshot, address, bytes));
            }
        }

        if matches.is_empty() {
            for snapshot in discover_pcls(regions, address, bytes) {
                if self
                    .watched
                    .iter()
                    .any(|watch| watch.snapshot.address == snapshot.address)
                {
                    continue;
                }
                let context = find_pcl_context(regions, snapshot);
                self.watch_chain(regions, snapshot, context, &mut events, cycle);
            }
        }

        matches = self.matching_buffers(address, bytes);
        for pcl_address in matches.iter().copied() {
            let watch = self
                .watched
                .iter()
                .find(|watch| watch.snapshot.address == pcl_address)
                .expect("matching PCL must be watched");
            if watch.snapshot.control & 1 != 0
                && !events.iter().any(|event| {
                    matches!(
                        event,
                        MachineDiagnosticEvent::PclOverwriteRisk {
                            pcl_address: existing,
                            ..
                        } if *existing == pcl_address
                    )
                })
            {
                events.push(overwrite_event(cycle, watch.snapshot, address, bytes));
            }
        }
        (matches, events)
    }

    pub fn matching_buffers(&self, address: u32, bytes: u32) -> Vec<u32> {
        self.watched
            .iter()
            .filter(|watch| range_contains(watch.snapshot, address, bytes))
            .map(|watch| watch.snapshot.address)
            .collect()
    }

    pub fn known_pcl_addresses(&self) -> Vec<u32> {
        self.watched
            .iter()
            .map(|watch| watch.snapshot.address)
            .collect()
    }

    pub fn sample(
        &mut self,
        cycle: u64,
        regions: &[RamDiagnosticRegion<'_>],
    ) -> Vec<MachineDiagnosticEvent> {
        let mut events = Vec::new();
        let mut discovered_pcbs = Vec::new();
        for watch in &mut self.watched {
            let Some(current) = parse_pcl(regions, watch.snapshot.address) else {
                continue;
            };
            if current != watch.snapshot
                && watch.context.pcb_address.is_none()
                && !watch.context_retry_done
                && current.submode & 0x0E != 0
            {
                let context = find_pcl_context(regions, current);
                if let Some(pcb_address) = context.pcb_address {
                    watch.context = context;
                    discovered_pcbs.push(pcb_address);
                }
                watch.context_retry_done = true;
            }
            if current == watch.snapshot {
                continue;
            }
            let previous = watch.snapshot;
            let transition = if previous.control & 1 != 0
                && current.control & 1 == 0
                && current.count <= previous.count
            {
                PclTransition::ConsumerReleased
            } else if previous.control & 1 == 0 && current.control & 1 != 0 {
                PclTransition::BufferFull
            } else if current.count > previous.count {
                PclTransition::ProducerAdvanced
            } else {
                PclTransition::Reconfigured
            };
            watch.snapshot = current;
            events.push(pcl_state_event(cycle, transition, current, watch.context));
        }
        for pcb_address in discovered_pcbs {
            self.watch_pcb(regions, pcb_address, cycle, &mut events);
        }
        for watch in &mut self.watched_pcbs {
            let Some(current) = parse_pcb(regions, watch.snapshot.address) else {
                continue;
            };
            if current == watch.snapshot {
                continue;
            }
            let previous = watch.snapshot;
            let transition = if previous.records_remaining != 0 && current.records_remaining == 0 {
                PcbTransition::RecordsReachedZero
            } else if current.records_remaining < previous.records_remaining {
                PcbTransition::RecordsDecremented
            } else if current.records_remaining > previous.records_remaining {
                PcbTransition::RecordsIncreased
            } else {
                PcbTransition::Reconfigured
            };
            watch.snapshot = current;
            events.push(pcb_state_event(cycle, transition, current));
        }
        events
    }

    fn watch_chain(
        &mut self,
        regions: &[RamDiagnosticRegion<'_>],
        first: PclDiagnosticSnapshot,
        context: PclContext,
        events: &mut Vec<MachineDiagnosticEvent>,
        cycle: u64,
    ) {
        let mut current = Some(first);
        for _ in 0..512 {
            let Some(snapshot) = current else {
                break;
            };
            if self
                .watched
                .iter()
                .any(|watch| watch.snapshot.address == snapshot.address)
            {
                break;
            }
            self.watched.push(WatchedPcl {
                snapshot,
                context,
                context_retry_done: false,
            });
            if let Some(pcb_address) = context.pcb_address {
                self.watch_pcb(regions, pcb_address, cycle, events);
            }
            events.push(pcl_state_event(
                cycle,
                PclTransition::Discovered,
                snapshot,
                context,
            ));
            current = if snapshot.next == 0 {
                None
            } else {
                parse_plausible_pcl(regions, snapshot.next)
            };
        }
    }

    fn watch_pcb(
        &mut self,
        regions: &[RamDiagnosticRegion<'_>],
        pcb_address: u32,
        cycle: u64,
        events: &mut Vec<MachineDiagnosticEvent>,
    ) {
        if self
            .watched_pcbs
            .iter()
            .any(|watch| watch.snapshot.address == pcb_address)
        {
            return;
        }
        let Some(snapshot) = parse_pcb(regions, pcb_address) else {
            return;
        };
        self.watched_pcbs.push(WatchedPcb { snapshot });
        events.push(pcb_state_event(cycle, PcbTransition::Discovered, snapshot));
    }
}

fn overwrite_event(
    cycle: u64,
    pcl: PclDiagnosticSnapshot,
    dma_address: u32,
    dma_bytes: u32,
) -> MachineDiagnosticEvent {
    MachineDiagnosticEvent::PclOverwriteRisk {
        cycle,
        pcl_address: pcl.address,
        buffer: pcl.buffer,
        buffer_size: pcl.buffer_size,
        count: pcl.count,
        dma_address,
        dma_bytes,
    }
}

fn pcl_state_event(
    cycle: u64,
    transition: PclTransition,
    pcl: PclDiagnosticSnapshot,
    context: PclContext,
) -> MachineDiagnosticEvent {
    MachineDiagnosticEvent::PclState {
        cycle,
        cpu_pc: 0,
        transition,
        data_kind: context.data_kind,
        channel: context.channel,
        pcb_address: context.pcb_address,
        cil_address: context.cil_address,
        pcl,
    }
}

fn pcb_state_event(
    cycle: u64,
    transition: PcbTransition,
    pcb: PcbDiagnosticSnapshot,
) -> MachineDiagnosticEvent {
    MachineDiagnosticEvent::PcbState {
        cycle,
        cpu_pc: 0,
        transition,
        pcb,
    }
}

fn discover_pcls(
    regions: &[RamDiagnosticRegion<'_>],
    dma_address: u32,
    dma_bytes: u32,
) -> Vec<PclDiagnosticSnapshot> {
    let mut found = Vec::new();
    for region in regions {
        if region.bytes.len() < 28 {
            continue;
        }
        for offset in (0..=region.bytes.len() - 28).step_by(2) {
            let bytes = &region.bytes[offset..offset + 28];
            if bytes[1] != 0 || bytes[22] != 0 || bytes[23] != 0 || bytes[0] & 0x7E != 0 {
                continue;
            }
            let buffer = read_u32(bytes, 10);
            let buffer_size = read_u32(bytes, 14);
            let count = read_u32(bytes, 24);
            let next = read_u32(bytes, 6);
            let dma_at_count = buffer.checked_add(count);
            let dma_end_at_count = dma_address
                .checked_add(dma_bytes)
                .is_some_and(|end| Some(end) == dma_at_count);
            if dma_at_count != Some(dma_address)
                && !dma_end_at_count
                && !(bytes[0] & 1 != 0 && buffer == dma_address)
            {
                continue;
            }
            if next == 0 || buffer_size == 0 || buffer_size > 4096 {
                continue;
            }
            let address = region.base.wrapping_add(offset as u32);
            let Some(pcl) = parse_plausible_pcl(regions, address) else {
                continue;
            };
            if range_contains(pcl, dma_address, dma_bytes)
                && (dma_address == pcl.buffer.wrapping_add(pcl.count)
                    || dma_address.wrapping_add(dma_bytes) == pcl.buffer.wrapping_add(pcl.count)
                    || (pcl.control & 1 != 0 && dma_address == pcl.buffer))
            {
                found.push(pcl);
            }
        }
        if !found.is_empty() {
            break;
        }
    }
    found
}

fn parse_plausible_pcl(
    regions: &[RamDiagnosticRegion<'_>],
    address: u32,
) -> Option<PclDiagnosticSnapshot> {
    let pcl = parse_pcl(regions, address)?;
    let bytes = read_memory(regions, address, 28)?;
    if bytes[1] != 0
        || bytes[22] != 0
        || bytes[23] != 0
        || pcl.control & 0x7E != 0
        || pcl.buffer_size == 0
        || pcl.buffer_size > 4096
        || pcl.count > pcl_buffer_capacity(pcl)
        || read_memory(regions, pcl.buffer, pcl_buffer_capacity(pcl) as usize).is_none()
        || (pcl.next != 0 && read_memory(regions, pcl.next, 28).is_none())
    {
        return None;
    }
    Some(pcl)
}

fn parse_pcl(regions: &[RamDiagnosticRegion<'_>], address: u32) -> Option<PclDiagnosticSnapshot> {
    let bytes = read_memory(regions, address, 28)?;
    Some(PclDiagnosticSnapshot {
        address,
        control: bytes[0],
        submode: bytes[2],
        coding: bytes[3],
        signal: u16::from_be_bytes([bytes[4], bytes[5]]),
        next: read_u32(bytes, 6),
        buffer: read_u32(bytes, 10),
        buffer_size: read_u32(bytes, 14),
        error_buffer: read_u32(bytes, 18),
        count: read_u32(bytes, 24),
    })
}

fn range_contains(pcl: PclDiagnosticSnapshot, address: u32, bytes: u32) -> bool {
    let Some(dma_end) = address.checked_add(bytes) else {
        return false;
    };
    let Some(buffer_end) = pcl.buffer.checked_add(pcl_buffer_capacity(pcl)) else {
        return false;
    };
    address >= pcl.buffer && dma_end <= buffer_end
}

fn pcl_buffer_capacity(pcl: PclDiagnosticSnapshot) -> u32 {
    let bytes_per_sector = if pcl.submode & 0x20 == 0 {
        2048
    } else if pcl.submode & 0x04 != 0 {
        2304
    } else {
        2324
    };
    pcl.buffer_size.saturating_mul(bytes_per_sector)
}

fn find_pcl_context(regions: &[RamDiagnosticRegion<'_>], pcl: PclDiagnosticSnapshot) -> PclContext {
    for pointer_address in pointer_occurrences(regions, pcl.address) {
        for (data_kind, pcb_cil_offset, channel_count) in [
            (PclDataKind::Video, 14u32, 32u8),
            (PclDataKind::Audio, 18u32, 16u8),
            (PclDataKind::Data, 22u32, 32u8),
        ] {
            for channel in 0..channel_count {
                let channel_offset = u32::from(channel) * 4;
                let Some(cil_address) = pointer_address.checked_sub(channel_offset) else {
                    continue;
                };
                if read_be_u32(regions, cil_address.wrapping_add(channel_offset))
                    != Some(pcl.address)
                {
                    continue;
                }
                for cil_pointer in pointer_occurrences(regions, cil_address) {
                    let Some(pcb_address) = cil_pointer.checked_sub(pcb_cil_offset) else {
                        continue;
                    };
                    let Some(pcb) = read_memory(regions, pcb_address, 26) else {
                        continue;
                    };
                    if read_u32(pcb, pcb_cil_offset as usize) != cil_address {
                        continue;
                    }
                    // PCB_Chan selects sectors for processing regardless of
                    // data kind. PCB_AChan separately routes selected audio
                    // directly to the audio processor instead of RAM.
                    let selected = read_u32(pcb, 8) & (1u32 << channel) != 0;
                    if !selected {
                        continue;
                    }
                    return PclContext {
                        data_kind,
                        channel: Some(channel),
                        pcb_address: Some(pcb_address),
                        cil_address: Some(cil_address),
                    };
                }
            }
        }
    }

    PclContext {
        data_kind: if pcl.submode & 0x02 != 0 {
            PclDataKind::Video
        } else if pcl.submode & 0x04 != 0 {
            PclDataKind::Audio
        } else if pcl.submode & 0x08 != 0 {
            PclDataKind::Data
        } else {
            PclDataKind::Unknown
        },
        channel: None,
        pcb_address: None,
        cil_address: None,
    }
}

fn parse_pcb(regions: &[RamDiagnosticRegion<'_>], address: u32) -> Option<PcbDiagnosticSnapshot> {
    let bytes = read_memory(regions, address, 26)?;
    Some(PcbDiagnosticSnapshot {
        address,
        status: u16::from_be_bytes([bytes[0], bytes[1]]),
        signal: u16::from_be_bytes([bytes[2], bytes[3]]),
        records_remaining: read_u32(bytes, 4),
        channel_mask: read_u32(bytes, 8),
        audio_channel_mask: u16::from_be_bytes([bytes[12], bytes[13]]),
        video_cil: read_u32(bytes, 14),
        audio_cil: read_u32(bytes, 18),
        data_cil: read_u32(bytes, 22),
    })
}

fn pointer_occurrences(regions: &[RamDiagnosticRegion<'_>], value: u32) -> Vec<u32> {
    let needle = value.to_be_bytes();
    let mut found = Vec::new();
    for region in regions {
        if region.bytes.len() < 4 {
            continue;
        }
        for offset in (0..=region.bytes.len() - 4).step_by(2) {
            if region.bytes[offset..offset + 4] == needle {
                found.push(region.base.wrapping_add(offset as u32));
            }
        }
    }
    found
}

fn read_be_u32(regions: &[RamDiagnosticRegion<'_>], address: u32) -> Option<u32> {
    let bytes = read_memory(regions, address, 4)?;
    Some(read_u32(bytes, 0))
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

fn read_memory<'a>(
    regions: &'a [RamDiagnosticRegion<'a>],
    address: u32,
    len: usize,
) -> Option<&'a [u8]> {
    regions.iter().find_map(|region| {
        let offset = address.checked_sub(region.base)? as usize;
        let end = offset.checked_add(len)?;
        region.bytes.get(offset..end)
    })
}

#[cfg(test)]
mod pcl_tests {
    use super::*;

    fn put_u16(memory: &mut [u8], address: usize, value: u16) {
        memory[address..address + 2].copy_from_slice(&value.to_be_bytes());
    }

    fn put_u32(memory: &mut [u8], address: usize, value: u32) {
        memory[address..address + 4].copy_from_slice(&value.to_be_bytes());
    }

    fn put_pcl(memory: &mut [u8], address: usize, next: u32, buffer: u32) {
        memory[address + 2] = 0x22;
        memory[address + 3] = 0x0F;
        put_u16(memory, address + 4, 7);
        put_u32(memory, address + 6, next);
        put_u32(memory, address + 10, buffer);
        put_u32(memory, address + 14, 1);
        put_u32(memory, address + 18, 0);
        put_u32(memory, address + 24, 0);
    }

    fn put_pcb(
        memory: &mut [u8],
        address: usize,
        records_remaining: u32,
        channel_mask: u32,
        audio_channel_mask: u16,
    ) {
        put_u16(memory, address, 1);
        put_u16(memory, address + 2, 0x1234);
        put_u32(memory, address + 4, records_remaining);
        put_u32(memory, address + 8, channel_mask);
        put_u16(memory, address + 12, audio_channel_mask);
    }

    #[test]
    fn circular_pcl_trace_detects_reuse_before_release() {
        let mut memory = vec![0u8; 0x3000];
        let pcb = 0x100;
        let cil = 0x200;
        let pcl0 = 0x300;
        let pcl1 = 0x340;
        let buffer0 = 0x1000;
        let buffer1 = 0x2000;

        put_pcb(&mut memory, pcb, 1, 1, 0);
        put_u32(&mut memory, pcb + 14, cil as u32);
        put_u32(&mut memory, cil, pcl0 as u32);
        put_pcl(&mut memory, pcl0, pcl1 as u32, buffer0 as u32);
        put_pcl(&mut memory, pcl1, pcl0 as u32, buffer1 as u32);

        let mut tracker = PclOwnershipTracker::default();
        let regions = [RamDiagnosticRegion {
            base: 0,
            bytes: &memory,
        }];
        let (matches, events) = tracker.observe_cdic_dma(10, &regions, buffer0 as u32, 2324);
        assert_eq!(matches, vec![pcl0 as u32]);
        assert!(events.iter().any(|event| matches!(
            event,
            MachineDiagnosticEvent::PclState {
                transition: PclTransition::Discovered,
                channel: Some(0),
                pcb_address: Some(0x100),
                cil_address: Some(0x200),
                ..
            }
        )));

        memory[pcl0] = 1;
        put_u32(&mut memory, pcl0 + 24, 2324);
        let regions = [RamDiagnosticRegion {
            base: 0,
            bytes: &memory,
        }];
        let events = tracker.sample(20, &regions);
        assert!(events.iter().any(|event| matches!(
            event,
            MachineDiagnosticEvent::PclState {
                transition: PclTransition::BufferFull,
                ..
            }
        )));

        let (_, events) = tracker.observe_cdic_dma(30, &regions, buffer0 as u32, 2324);
        assert!(events.iter().any(|event| matches!(
            event,
            MachineDiagnosticEvent::PclOverwriteRisk {
                pcl_address: 0x300,
                buffer_size: 1,
                ..
            }
        )));

        memory[pcl0] = 0;
        put_u32(&mut memory, pcl0 + 24, 0);
        let regions = [RamDiagnosticRegion {
            base: 0,
            bytes: &memory,
        }];
        let events = tracker.sample(40, &regions);
        assert!(events.iter().any(|event| matches!(
            event,
            MachineDiagnosticEvent::PclState {
                transition: PclTransition::ConsumerReleased,
                ..
            }
        )));
        let (_, events) = tracker.observe_cdic_dma(50, &regions, buffer0 as u32, 2324);
        assert!(!events
            .iter()
            .any(|event| matches!(event, MachineDiagnosticEvent::PclOverwriteRisk { .. })));
    }

    #[test]
    fn pcb_trace_reports_record_count_and_routing_changes() {
        let mut memory = vec![0u8; 0x2000];
        let pcb = 0x100;
        let cil = 0x200;
        let pcl = 0x300;
        let buffer = 0x1000;

        put_pcb(&mut memory, pcb, 2, 0x0000_0003, 0x0001);
        put_u32(&mut memory, pcb + 14, cil as u32);
        put_u32(&mut memory, cil + 4, pcl as u32);
        put_pcl(&mut memory, pcl, pcl as u32, buffer as u32);

        let mut tracker = PclOwnershipTracker::default();
        let regions = [RamDiagnosticRegion {
            base: 0,
            bytes: &memory,
        }];
        let (_, events) = tracker.observe_cdic_dma(10, &regions, buffer as u32, 2324);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, MachineDiagnosticEvent::PcbState { .. }))
                .count(),
            1
        );
        assert!(events.iter().any(|event| matches!(
            event,
            MachineDiagnosticEvent::PcbState {
                transition: PcbTransition::Discovered,
                pcb: snapshot,
                ..
            } if snapshot.address == pcb as u32
                && snapshot.status == 1
                && snapshot.signal == 0x1234
                && snapshot.records_remaining == 2
                && snapshot.channel_mask == 3
                && snapshot.audio_channel_mask == 1
                && snapshot.video_cil == cil as u32
        )));

        put_u32(&mut memory, pcb + 4, 1);
        let regions = [RamDiagnosticRegion {
            base: 0,
            bytes: &memory,
        }];
        assert!(tracker.sample(20, &regions).iter().any(|event| matches!(
            event,
            MachineDiagnosticEvent::PcbState {
                transition: PcbTransition::RecordsDecremented,
                pcb: snapshot,
                ..
            } if snapshot.records_remaining == 1
        )));

        put_u32(&mut memory, pcb + 4, 0);
        let regions = [RamDiagnosticRegion {
            base: 0,
            bytes: &memory,
        }];
        assert!(tracker.sample(30, &regions).iter().any(|event| matches!(
            event,
            MachineDiagnosticEvent::PcbState {
                transition: PcbTransition::RecordsReachedZero,
                pcb: snapshot,
                ..
            } if snapshot.records_remaining == 0
        )));

        put_u32(&mut memory, pcb + 4, 3);
        let regions = [RamDiagnosticRegion {
            base: 0,
            bytes: &memory,
        }];
        assert!(tracker.sample(40, &regions).iter().any(|event| matches!(
            event,
            MachineDiagnosticEvent::PcbState {
                transition: PcbTransition::RecordsIncreased,
                pcb: snapshot,
                ..
            } if snapshot.records_remaining == 3
        )));

        put_u32(&mut memory, pcb + 8, 2);
        let regions = [RamDiagnosticRegion {
            base: 0,
            bytes: &memory,
        }];
        assert!(tracker.sample(50, &regions).iter().any(|event| matches!(
            event,
            MachineDiagnosticEvent::PcbState {
                transition: PcbTransition::Reconfigured,
                pcb: snapshot,
                ..
            } if snapshot.channel_mask == 2
        )));
    }

    #[test]
    fn audio_pcl_context_uses_channel_mask_not_direct_audio_mask() {
        let mut memory = vec![0u8; 0x2000];
        let pcb = 0x100;
        let cil = 0x200;
        let pcl = 0x300;
        let buffer = 0x1000;

        put_pcb(&mut memory, pcb, 1, 1 << 2, 0);
        put_u32(&mut memory, pcb + 18, cil as u32);
        put_u32(&mut memory, cil + 8, pcl as u32);
        put_pcl(&mut memory, pcl, pcl as u32, buffer as u32);
        memory[pcl + 2] = 0x24;

        let mut tracker = PclOwnershipTracker::default();
        let regions = [RamDiagnosticRegion {
            base: 0,
            bytes: &memory,
        }];
        let (_, events) = tracker.observe_cdic_dma(10, &regions, buffer as u32, 2304);
        assert!(events.iter().any(|event| matches!(
            event,
            MachineDiagnosticEvent::PclState {
                transition: PclTransition::Discovered,
                data_kind: PclDataKind::Audio,
                channel: Some(2),
                pcb_address: Some(0x100),
                cil_address: Some(0x200),
                ..
            }
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            MachineDiagnosticEvent::PcbState {
                pcb: snapshot,
                ..
            } if snapshot.channel_mask == 1 << 2 && snapshot.audio_channel_mask == 0
        )));
    }
}
