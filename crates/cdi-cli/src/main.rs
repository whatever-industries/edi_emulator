// SPDX-License-Identifier: GPL-3.0-or-later
//! Headless harness for E-Di: Emulator Disc Interactive.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum VideoStandardArg {
    Pal,
    Ntsc,
}

impl From<VideoStandardArg> for cdi_core::VideoStandard {
    fn from(value: VideoStandardArg) -> Self {
        match value {
            VideoStandardArg::Pal => Self::Pal,
            VideoStandardArg::Ntsc => Self::Ntsc,
        }
    }
}

#[derive(Clone, Debug)]
struct ClickEvent {
    at: u64,
    x: i32,
    y: i32,
    buttons: u8,
    duration: u64,
}

fn parse_click_event(value: &str) -> Result<ClickEvent, String> {
    let (at, event) = value
        .split_once(':')
        .ok_or("expected INSTRUCTION:X,Y[,BUTTONS[,DURATION]]")?;
    let at = at
        .parse()
        .map_err(|_| "invalid instruction number".to_owned())?;
    let mut fields = event.split(',');
    let x = fields
        .next()
        .ok_or("missing X coordinate")?
        .parse()
        .map_err(|_| "invalid X coordinate".to_owned())?;
    let y = fields
        .next()
        .ok_or("missing Y coordinate")?
        .parse()
        .map_err(|_| "invalid Y coordinate".to_owned())?;
    let buttons = fields
        .next()
        .map_or(Ok(1), |field| field.parse())
        .map_err(|_| "invalid button mask".to_owned())?;
    let duration = fields
        .next()
        .map_or(Ok(2_000_000), |field| field.parse())
        .map_err(|_| "invalid duration")?;
    if fields.next().is_some() {
        return Err("too many click-event fields".to_owned());
    }
    Ok(ClickEvent {
        at,
        x,
        y,
        buttons,
        duration,
    })
}

#[derive(Parser)]
#[command(
    name = "cdi-cli",
    about = "Headless E-Di: Emulator Disc Interactive harness"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Command {
    /// Inspect a system ROM: list OS-9 modules and identify the player model.
    Info {
        /// Path to a system ROM image.
        rom: PathBuf,
    },
    /// Boot a system ROM headlessly, executing instructions from the reset
    /// vector.
    Boot {
        /// Path to a system ROM image.
        rom: PathBuf,
        /// Model id (default: auto-detect from the ROM).
        #[arg(long)]
        model: Option<String>,
        /// Player video standard (default: the model's configured standard).
        #[arg(long, value_enum)]
        video_standard: Option<VideoStandardArg>,
        /// Number of instructions to execute.
        #[arg(long, default_value_t = 100_000)]
        instructions: u64,
        /// Print an instruction trace (pc/sr/d0/a7 per step).
        #[arg(long)]
        trace: bool,
        /// CUE sheet of a disc image to insert.
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Optional VMPEG/IMPEG Digital Video Cartridge firmware ROM.
        #[arg(long)]
        dvc_rom: Option<PathBuf>,
        /// Click the pointer at "x,y" (device coords 0-767,0-559) at 60% of
        /// the run, e.g. --click 588,265 hits the shell's PLAY CD-I button.
        #[arg(long)]
        click: Option<String>,
        /// Instruction at which to press `--click`; useful for repeatable
        /// long-running headless tests. Defaults to 60% of the run.
        #[arg(long)]
        click_at: Option<u64>,
        /// Repeatable scripted input event as
        /// INSTRUCTION:X,Y[,BUTTONS[,DURATION]]. Button bits are 1=left,
        /// 2=right, and 4=both/third CD-i action.
        #[arg(long = "click-event", value_parser = parse_click_event)]
        click_events: Vec<ClickEvent>,
        /// Write the final framebuffer to a PNG file.
        #[arg(long)]
        screenshot: Option<PathBuf>,
        /// Write the current VMPEG play's MPEG-1 video elementary stream.
        #[arg(long)]
        dump_vmpeg_es: Option<PathBuf>,
        /// Write the final plane-A/plane-B video RAM to a diagnostic directory.
        #[arg(long)]
        dump_video_ram: Option<PathBuf>,
        /// Print the SHA-256 of the final framebuffer.
        #[arg(long)]
        hash: bool,
    },
    /// Inspect a CUE/BIN disc image: TOC, layout, and CD-i label detection.
    Disc {
        /// Path to a .cue file.
        cue: PathBuf,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    match Cli::parse().command {
        Command::Info { rom } => info(&rom),
        Command::Boot {
            rom,
            model,
            video_standard,
            instructions,
            trace,
            disc,
            dvc_rom,
            click,
            click_at,
            click_events,
            screenshot,
            dump_vmpeg_es,
            dump_video_ram,
            hash,
        } => boot(
            &rom,
            model.as_deref(),
            video_standard,
            instructions,
            trace,
            disc.as_deref(),
            dvc_rom.as_deref(),
            click.as_deref(),
            click_at,
            click_events,
            screenshot.as_deref(),
            dump_vmpeg_es.as_deref(),
            dump_video_ram.as_deref(),
            hash,
        ),
        Command::Disc { cue } => disc_info(&cue),
    }
}

fn disc_info(cue: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use cdi_disc::sector::{has_sync, Mode2Subheader, SectorHeader};

    let disc = cdi_disc::DiscImage::load(cue)?;
    println!(
        "{} track(s), lead-out at {} (absolute frame {})",
        disc.tracks().len(),
        disc.leadout_msf(),
        disc.leadout()
    );
    for t in disc.tracks() {
        println!(
            "  track {:02} {:>10}  region {:>7}..{:<7}  INDEX01 at {} (abs {})",
            t.number,
            format!("{:?}", t.mode),
            t.region_start,
            t.end,
            cdi_disc::Msf::from_frames(t.start),
            t.start,
        );
    }

    // Look for the CD-i disc label: absolute frame 166 (LBA 16) on normal
    // discs, or 16 frames into a CD-i Ready track-1 pregap.
    for &(what, abs) in &[("LBA 16 (abs 166)", 166u32), ("pregap+16 (abs 16)", 16u32)] {
        let Some(sector) = disc.read_sector_data(abs) else {
            continue;
        };
        if !has_sync(&sector) {
            continue;
        }
        let header = SectorHeader::parse(&sector);
        let label = &sector[25..30];
        if label == b"CD-I " {
            let sub = Mode2Subheader::parse(&sector).unwrap();
            let album_bytes = &sector[24 + 0x28..24 + 0x28 + 32];
            let album = String::from_utf8_lossy(album_bytes).trim_end().to_string();
            println!(
                "CD-i disc label found at {what}: header {header:?}, submode {:#04x}",
                sub.submode
            );
            println!("  album: {album:?}");
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn boot(
    path: &std::path::Path,
    model_id: Option<&str>,
    video_standard: Option<VideoStandardArg>,
    instructions: u64,
    trace: bool,
    disc: Option<&std::path::Path>,
    dvc_rom: Option<&std::path::Path>,
    click: Option<&str>,
    click_at: Option<u64>,
    mut click_events: Vec<ClickEvent>,
    screenshot: Option<&std::path::Path>,
    dump_vmpeg_es: Option<&std::path::Path>,
    dump_video_ram: Option<&std::path::Path>,
    hash: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let click_pos: Option<(i32, i32)> = click.map(|s| {
        let mut it = s.split(',');
        (
            it.next().and_then(|v| v.parse().ok()).unwrap_or(384),
            it.next().and_then(|v| v.parse().ok()).unwrap_or(280),
        )
    });
    let image = std::fs::read(path)?;
    let detected_model = match model_id {
        Some(id) => {
            cdi_core::boards::model_by_id(id).ok_or_else(|| format!("unknown model id {id:?}"))?
        }
        None => {
            let modules = cdi_os9::scan_modules(&image);
            let rom_type = cdi_os9::identify_rom(&modules);
            cdi_core::boards::model_by_id(rom_type.id).ok_or_else(|| {
                format!(
                    "ROM identified as {} ({}) which has no emulation model yet",
                    rom_type.id, rom_type.title
                )
            })?
        }
    };
    let mut model = detected_model.clone();
    if let Some(standard) = video_standard {
        model.video = standard.into();
    }
    println!(
        "Booting as {} ({}, {:?})",
        model.title, model.board.name, model.video
    );

    let dvc = dvc_rom
        .map(|path| {
            let firmware = std::fs::read(path)?;
            let config = cdi_core::DvcConfig::from_rom(firmware)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            println!("DVC firmware: {} ({})", config.kind.name(), path.display());
            Ok::<_, Box<dyn std::error::Error>>(config)
        })
        .transpose()?;
    let mut machine = cdi_core::Machine::with_dvc(&model, image, dvc)?;
    if dump_vmpeg_es.is_some() {
        if let Some(dvc) = &mut machine.bus.dvc {
            dvc.set_video_es_capture(true);
        }
    }
    if let Some(cue) = disc {
        let disc_image = cdi_disc::DiscImage::load(cue)?;
        println!(
            "Disc inserted: {} track(s), lead-out {}",
            disc_image.tracks().len(),
            disc_image.leadout_msf()
        );
        machine.set_disc(Some(disc_image));
    }
    println!(
        "Reset: ssp={:#010x} pc={:#010x}",
        machine.cpu.a[7], machine.cpu.pc
    );
    let mut uart_log: Vec<u8> = Vec::new();
    let mut audio_samples: u64 = 0;
    if let Some((x, y)) = click_pos {
        click_events.push(ClickEvent {
            at: click_at.unwrap_or(instructions.saturating_mul(3) / 5),
            x,
            y,
            buttons: 1,
            duration: 40_000_000,
        });
    }
    click_events.sort_by_key(|event| event.at);
    let mut click_event_index = 0usize;
    for i in 0..instructions {
        if trace {
            println!(
                "{i:>8} pc={:#010x} sr={:#06x} d0={:#010x} a7={:#010x}",
                machine.cpu.pc, machine.cpu.sr, machine.cpu.d[0], machine.cpu.a[7]
            );
        }
        if let Some(event) = click_events.get(click_event_index) {
            // Hover before the requested point, hold for a deterministic
            // interval, then release. Multiple events make title-flow tests
            // reproducible without a GUI automation layer.
            let hover_at = event.at.saturating_sub(20_000_000);
            let release_at = event.at.saturating_add(event.duration);
            if i == hover_at {
                machine.bus.slave.set_pointer_absolute(event.x, event.y, 0);
            } else if i >= hover_at {
                let buttons = if (event.at..release_at).contains(&i) {
                    event.buttons
                } else {
                    0
                };
                machine.bus.slave.set_pointer(event.x, event.y, buttons);
                if i >= release_at {
                    click_event_index += 1;
                }
            }
        }
        machine.step();
        audio_samples += machine.take_audio().len() as u64 / 2;
        let out = machine.take_uart_output();
        if !out.is_empty() {
            use std::io::Write;
            std::io::stdout().write_all(&out)?;
            std::io::stdout().flush()?;
            uart_log.extend_from_slice(&out);
        }
        if machine.cpu.stopped && machine.cpu.pending_ipl == 0 {
            // Fully idle would spin forever without devices; note and go on.
            log::trace!("CPU stopped at instruction {i}");
        }
    }
    if !uart_log.is_empty() {
        println!();
    }
    println!(
        "Done: {} instructions, {} cycles, pc={:#010x}, {} exceptions, {} UART bytes, {} frames, {} audio samples",
        instructions,
        machine.cpu.cycles,
        machine.cpu.pc,
        machine.cpu.exceptions_taken,
        uart_log.len(),
        machine.bus.mcd212.frame_count,
        audio_samples,
    );
    if let Some(stats) = machine.dvc_stats() {
        println!(
            "VMPEG: {} DMA words + {} direct words, {} packs, PES video/audio {}/{}, elementary bytes {}/{}, decoded frames video/audio {}/{}, errors demux/video/audio {}/{}/{}",
            stats.dma_words,
            stats.direct_words,
            stats.system_packs,
            stats.video_pes_packets,
            stats.audio_pes_packets,
            stats.video_bytes,
            stats.audio_bytes,
            stats.decoded_video_frames,
            stats.decoded_audio_frames,
            stats.demux_errors,
            stats.video_errors,
            stats.audio_errors,
        );
        println!(
            "VMPEG display: {} frames presented, {} queued, visible={}, playing={}; events play/continue/VCUP/pause {}/{}/{}/{}, {} queued audio samples",
            stats.presented_video_frames,
            stats.queued_video_frames,
            stats.video_visible,
            stats.playing,
            stats.play_events,
            stats.continue_events,
            stats.video_update_events,
            stats.pause_events,
            stats.queued_audio_samples,
        );
        println!(
            "VMPEG events: sequence/GOP/last-picture/sequence-end/program-end {}/{}/{}/{}/{}, underflow video/audio {}/{}, audio start/stop {}/{} ({} samples discarded), interrupt acks audio/video {}/{}",
            stats.sequence_events,
            stats.group_events,
            stats.end_of_data_events,
            stats.sequence_end_events,
            stats.program_end_events,
            stats.video_underflow_events,
            stats.audio_underflow_events,
            stats.audio_start_events,
            stats.audio_stop_events,
            stats.audio_samples_discarded,
            stats.fma_intacks,
            stats.fmv_intacks,
        );
        println!(
            "VMPEG end routing: program-end video/audio {}/{}",
            stats.video_program_end_events, stats.audio_program_end_events,
        );
    }

    log::info!(
        "video: dcr=[{:#06x},{:#06x}] ddr=[{:#06x},{:#06x}] icm={:#08x} tcr={:#08x} order={} vsr=[{:#08x},{:#08x}] dcp=[{:#08x},{:#08x}] dca=[{:#08x},{:#08x}] csrw=[{:#06x},{:#06x}] backdrop={} mosaic=[{:#08x},{:#08x}] weight={:?} cursor=({:#08x},{:#08x})",
        machine.bus.mcd212.dcr[0],
        machine.bus.mcd212.dcr[1],
        machine.bus.mcd212.ddr[0],
        machine.bus.mcd212.ddr[1],
        machine.bus.mcd212.image_coding_method,
        machine.bus.mcd212.transparency_control,
        machine.bus.mcd212.plane_order,
        machine.bus.mcd212.get_vsr(0),
        machine.bus.mcd212.get_vsr(1),
        machine.bus.mcd212.get_dcp(0),
        machine.bus.mcd212.get_dcp(1),
        machine.bus.mcd212.dca[0],
        machine.bus.mcd212.dca[1],
        machine.bus.mcd212.csrw[0],
        machine.bus.mcd212.csrw[1],
        machine.bus.mcd212.backdrop_color,
        machine.bus.mcd212.mosaic_hold[0],
        machine.bus.mcd212.mosaic_hold[1],
        machine.bus.mcd212.weight_factor_base,
        machine.bus.mcd212.cursor_position,
        machine.bus.mcd212.cursor_control,
    );

    let (width, height) = machine.bus.mcd212.visible_size();
    let fb = machine.bus.mcd212.framebuffer();
    if hash {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        for px in &fb[..width * height] {
            hasher.update(px.to_be_bytes());
        }
        println!("Framebuffer SHA-256: {:x}", hasher.finalize());
    }
    if let Some(out_path) = screenshot {
        let mut rgb = Vec::with_capacity(width * height * 3);
        for px in &fb[..width * height] {
            let px = cdi_core::mcd212::presentation_rgb(*px);
            rgb.extend_from_slice(&[(px >> 16) as u8, (px >> 8) as u8, px as u8]);
        }
        let file = std::fs::File::create(out_path)?;
        let mut encoder =
            png::Encoder::new(std::io::BufWriter::new(file), width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&rgb)?;
        println!("Screenshot written to {}", out_path.display());
    }
    if let Some(out_path) = dump_vmpeg_es {
        let bytes = machine
            .bus
            .dvc
            .as_ref()
            .and_then(cdi_core::Vmpeg::captured_video_es)
            .ok_or("--dump-vmpeg-es requires an attached VMPEG cartridge")?;
        std::fs::write(out_path, bytes)?;
        println!(
            "VMPEG elementary stream written to {} ({} bytes)",
            out_path.display(),
            bytes.len()
        );
    }
    if let Some(out_dir) = dump_video_ram {
        std::fs::create_dir_all(out_dir)?;
        let plane_a = machine
            .bus
            .ram
            .first()
            .ok_or("plane-A RAM is unavailable")?;
        let plane_b = machine.bus.ram.get(1).ok_or("plane-B RAM is unavailable")?;
        std::fs::write(out_dir.join("plane-a.bin"), plane_a)?;
        std::fs::write(out_dir.join("plane-b.bin"), plane_b)?;
        println!("Video RAM written to {}", out_dir.display());
    }
    Ok(())
}

fn info(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let image = std::fs::read(path)?;
    let modules = cdi_os9::scan_modules(&image);
    if modules.is_empty() {
        println!("No OS-9 modules found ({} bytes).", image.len());
        return Ok(());
    }

    println!(
        "{:<8} {:>8}  {:<8} rev edn crc  name",
        "offset", "size", "type"
    );
    for m in &modules {
        println!(
            "{:#08x} {:>8}  {:<8} {:>3} {:>3} {}  {}",
            m.offset,
            m.size,
            m.mod_type.name(),
            m.revision,
            m.edition,
            if m.crc_ok { "ok " } else { "BAD" },
            m.name,
        );
    }

    let rom_type = cdi_os9::identify_rom(&modules);
    let dvc_type = cdi_os9::identify_dvc_rom(&modules);
    if dvc_type != cdi_os9::DvcRomType::Unknown {
        println!(
            "\n{} modules. Identified DVC firmware: {}",
            modules.len(),
            dvc_type.name()
        );
        return Ok(());
    }
    println!(
        "\n{} modules. Identified: {} ({}), board {}",
        modules.len(),
        rom_type.id,
        rom_type.title,
        rom_type.board.name(),
    );
    if let Some(model) = cdi_core::boards::model_by_id(rom_type.id) {
        println!(
            "Model: {} — board {}, slave v{}, {} KB NVRAM",
            model.title,
            model.board.name,
            model.slave_version,
            model.nvram_size / 1024,
        );
    } else {
        println!("No emulation model for this ROM type yet.");
    }
    Ok(())
}
