// SPDX-License-Identifier: GPL-3.0-or-later
//! CD-i machine core: boards, bus, scheduler, and devices.
//!
//! This crate is deterministic by construction: no wall-clock time, no
//! randomness, no UI or audio dependencies. Frontends feed timestamped input
//! and consume framebuffers/audio buffers.

pub mod board;
pub mod boards;
pub mod cdic;
pub mod diagnostics;
pub mod dvc;
pub mod machine;
pub mod mcd212;
mod mpeg1_video;
pub mod sched;
pub mod slave;

pub use board::{BoardDef, DeviceKind, ModelDef, VideoStandard};
pub use diagnostics::{
    CpuDiagnosticSnapshot, DisplayProvenanceSnapshot, DmaChannelDiagnosticSnapshot,
    DmaDiagnosticSnapshot, InterruptDiagnosticSnapshot, MachineDiagnosticEvent,
    MachineDiagnosticSnapshot, Mcd212DiagnosticSnapshot,
};
pub use dvc::{DvcConfig, DvcKind, DvcStats, Vmpeg};
pub use machine::{Machine, MachineBus};
pub use sched::{EventId, Scheduler, Ticks};
