// SPDX-License-Identifier: GPL-2.0-or-later
//! Headless CD-i emulator harness.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cdi-cli", about = "Headless CD-i emulator harness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
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
        /// Number of instructions to execute.
        #[arg(long, default_value_t = 100_000)]
        instructions: u64,
        /// Print an instruction trace (pc/sr/d0/a7 per step).
        #[arg(long)]
        trace: bool,
        /// CUE sheet of a disc image to insert.
        #[arg(long)]
        disc: Option<PathBuf>,
        /// Click the pointer at "x,y" (device coords 0-767,0-559) at 60% of
        /// the run, e.g. --click 588,265 hits the shell's PLAY CD-I button.
        #[arg(long)]
        click: Option<String>,
        /// Write the final framebuffer to a PNG file.
        #[arg(long)]
        screenshot: Option<PathBuf>,
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
            instructions,
            trace,
            disc,
            click,
            screenshot,
            hash,
        } => boot(
            &rom,
            model.as_deref(),
            instructions,
            trace,
            disc.as_deref(),
            click.as_deref(),
            screenshot.as_deref(),
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
    instructions: u64,
    trace: bool,
    disc: Option<&std::path::Path>,
    click: Option<&str>,
    screenshot: Option<&std::path::Path>,
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
    let model = match model_id {
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
    println!("Booting as {} ({})", model.title, model.board.name);

    let mut machine = cdi_core::Machine::new(model, image)?;
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
    for i in 0..instructions {
        if trace {
            println!(
                "{i:>8} pc={:#010x} sr={:#06x} d0={:#010x} a7={:#010x}",
                machine.cpu.pc, machine.cpu.sr, machine.cpu.d[0], machine.cpu.a[7]
            );
        }
        if let Some((x, y)) = click_pos {
            // Hover from 50%, press at 60-65%, release after.
            let pct = i * 100 / instructions.max(1);
            if i == instructions / 2 {
                machine.bus.slave.set_pointer_absolute(x, y, 0);
            } else if pct >= 50 {
                let buttons = u8::from((60..65).contains(&pct));
                machine.bus.slave.set_pointer(x, y, buttons);
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

    log::info!(
        "video: dcr=[{:#06x},{:#06x}] ddr=[{:#06x},{:#06x}] icm={:#08x} vsr=[{:#08x},{:#08x}] csrw=[{:#06x},{:#06x}] backdrop={}",
        machine.bus.mcd212.dcr[0],
        machine.bus.mcd212.dcr[1],
        machine.bus.mcd212.ddr[0],
        machine.bus.mcd212.ddr[1],
        machine.bus.mcd212.image_coding_method,
        machine.bus.mcd212.get_vsr(0),
        machine.bus.mcd212.get_vsr(1),
        machine.bus.mcd212.csrw[0],
        machine.bus.mcd212.csrw[1],
        machine.bus.mcd212.backdrop_color,
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
            rgb.extend_from_slice(&[(px >> 16) as u8, (px >> 8) as u8, *px as u8]);
        }
        let file = std::fs::File::create(out_path)?;
        let mut encoder =
            png::Encoder::new(std::io::BufWriter::new(file), width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&rgb)?;
        println!("Screenshot written to {}", out_path.display());
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
