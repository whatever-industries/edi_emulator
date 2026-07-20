// SPDX-License-Identifier: GPL-2.0-or-later
//! Desktop frontend: renders the MCD212 framebuffer in an eframe window and
//! feeds mouse and gamepad input to the SLAVE pointer device.
//!
//! The emulator core runs on its own thread, paced to real time by frame
//! count; the UI thread copies the latest completed frame into a texture.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cdi_core::mcd212::{presentation_rgb, FB_HEIGHT, FB_WIDTH};
use clap::{Parser, ValueEnum};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, Producer, RingBuffer};

const AUDIO_RATE: u32 = 44_100;
const AUDIO_RING_SAMPLES: usize = AUDIO_RATE as usize * 2;
const APP_NAME: &str = "E-Di: Emulator Disc Interactive";
const BUNDLED_CDI220B: &[u8] = include_bytes!("../../../firmware/cdi220b.rom");
const BUNDLED_VMPEGA: &[u8] = include_bytes!("../../../firmware/vmpega.rom");
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/icon_256.png");

/// Decode the embedded application icon for the window/taskbar. The macOS
/// Dock icon instead comes from the app bundle's `.icns` (see
/// `scripts/make-app-bundle.sh`).
fn load_app_icon() -> Option<egui::IconData> {
    let decoder = png::Decoder::new(APP_ICON_PNG);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|px| [px[0], px[1], px[2], 0xFF])
            .collect(),
        _ => return None,
    };
    Some(egui::IconData {
        rgba,
        width: info.width,
        height: info.height,
    })
}

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

#[derive(Parser)]
#[command(
    name = "cdi-frontend",
    about = "E-Di: Emulator Disc Interactive desktop frontend"
)]
struct Args {
    /// CD-i system ROM override; uses the bundled CD-i 220 F2 when omitted.
    rom: Option<PathBuf>,
    /// CUE sheet of a disc image to insert.
    #[arg(long)]
    disc: Option<PathBuf>,
    /// Optional VMPEG/IMPEG Digital Video Cartridge firmware ROM.
    #[arg(long)]
    dvc_rom: Option<PathBuf>,
    /// Initial player video standard (default: PAL).
    #[arg(long, value_enum)]
    video_standard: Option<VideoStandardArg>,
}

#[derive(Default, Clone, Copy)]
struct InputState {
    /// Absolute pointer position for the direct mouse mapping; the SLAVE
    /// tracks it by deltas.
    x: i32,
    y: i32,
    /// Pending relative motion from the captured mouse, arrow keys, and
    /// gamepad; consumed once per emulated frame. Relative motion is bounded
    /// only by the SLAVE's device clamp, so it can always reach the screen
    /// edges even after a title reprograms the pointer position.
    dx: i32,
    dy: i32,
    buttons: u8,
}

/// User preferences persisted across runs via eframe storage.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Prefs {
    show_fps: bool,
    smooth_scaling: bool,
    capture_mouse_enabled: bool,
    pad_speed: f32,
    pad_deadzone: f32,
    pad_button1: gilrs::Button,
    pad_button2: gilrs::Button,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            show_fps: true,
            smooth_scaling: false,
            capture_mouse_enabled: true,
            pad_speed: 8.0,
            pad_deadzone: 0.15,
            pad_button1: gilrs::Button::South,
            pad_button2: gilrs::Button::East,
        }
    }
}

const PREFS_KEY: &str = "prefs";

fn button_name(button: gilrs::Button) -> &'static str {
    use gilrs::Button as B;
    match button {
        B::South => "South (A / Cross)",
        B::East => "East (B / Circle)",
        B::North => "North (Y / Triangle)",
        B::West => "West (X / Square)",
        B::C => "C",
        B::Z => "Z",
        B::LeftTrigger => "Left bumper",
        B::LeftTrigger2 => "Left trigger",
        B::RightTrigger => "Right bumper",
        B::RightTrigger2 => "Right trigger",
        B::Select => "Select",
        B::Start => "Start",
        B::Mode => "Mode / Guide",
        B::LeftThumb => "Left stick click",
        B::RightThumb => "Right stick click",
        B::DPadUp => "D-pad up",
        B::DPadDown => "D-pad down",
        B::DPadLeft => "D-pad left",
        B::DPadRight => "D-pad right",
        B::Unknown => "Unknown",
    }
}

struct SharedFrame {
    pixels: Vec<u32>,
    width: usize,
    height: usize,
    /// Side-border width and active picture width inside the framebuffer;
    /// the pointer device range maps onto the active area only.
    border: usize,
    active_width: usize,
    frame_no: u64,
}

struct Shared {
    frame: Mutex<SharedFrame>,
    input: Mutex<InputState>,
    command: Mutex<Option<MachineCommand>>,
    status: Mutex<String>,
    dvc_status: Mutex<String>,
    dvc_path: Mutex<Option<PathBuf>>,
    dvc_inserted: AtomicBool,
    pal: AtomicBool,
    muted: Arc<AtomicBool>,
    running: AtomicBool,
    /// Emulated frames per second (diagnostics).
    fps: Mutex<f32>,
}

enum MachineCommand {
    LoadDisc(PathBuf),
    EjectDisc,
    AttachDvc(PathBuf),
    AttachBundledDvc,
    DetachDvc,
    SetVideoStandard(cdi_core::VideoStandard),
    Reset,
}

#[cfg(target_os = "macos")]
enum NativeMenuAction {
    Open,
    Eject,
    Reset,
    Settings,
}

#[cfg(target_os = "macos")]
struct NativeMenu {
    _menu: muda::Menu,
    open_id: muda::MenuId,
    eject_id: muda::MenuId,
    reset_id: muda::MenuId,
    settings_ids: [muda::MenuId; 2],
}

#[cfg(target_os = "macos")]
impl NativeMenu {
    fn new() -> Result<Self, String> {
        use muda::accelerator::{Accelerator, Code, CMD_OR_CTRL};
        use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};

        let app_settings = MenuItem::with_id(
            "app.settings",
            "Settings…",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::Comma)),
        );
        let app_menu = Submenu::with_items(
            APP_NAME,
            true,
            &[
                &PredefinedMenuItem::about(Some("About E-Di: Emulator Disc Interactive"), None),
                &app_settings,
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::quit(Some("Quit E-Di: Emulator Disc Interactive")),
            ],
        )
        .map_err(|error| error.to_string())?;

        let open = MenuItem::with_id(
            "file.open",
            "Open…",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::KeyO)),
        );
        let eject = MenuItem::with_id("file.eject", "Eject Disc", true, None);
        let reset = MenuItem::with_id(
            "file.reset",
            "Reset",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::KeyR)),
        );
        let file_settings = MenuItem::with_id("file.settings", "Settings…", true, None);
        let file_menu = Submenu::with_items(
            "File",
            true,
            &[
                &open,
                &eject,
                &reset,
                &PredefinedMenuItem::separator(),
                &file_settings,
            ],
        )
        .map_err(|error| error.to_string())?;

        let menu = Menu::with_items(&[&app_menu, &file_menu]).map_err(|error| error.to_string())?;
        menu.init_for_nsapp();

        Ok(Self {
            _menu: menu,
            open_id: open.id().clone(),
            eject_id: eject.id().clone(),
            reset_id: reset.id().clone(),
            settings_ids: [app_settings.id().clone(), file_settings.id().clone()],
        })
    }

    fn actions(&self) -> Vec<NativeMenuAction> {
        let mut actions = Vec::new();
        while let Ok(event) = muda::MenuEvent::receiver().try_recv() {
            let action = if event.id == self.open_id {
                Some(NativeMenuAction::Open)
            } else if event.id == self.eject_id {
                Some(NativeMenuAction::Eject)
            } else if event.id == self.reset_id {
                Some(NativeMenuAction::Reset)
            } else if self.settings_ids.contains(&event.id) {
                Some(NativeMenuAction::Settings)
            } else {
                None
            };
            if let Some(action) = action {
                actions.push(action);
            }
        }
        actions
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let Args {
        rom,
        disc,
        dvc_rom,
        video_standard,
    } = Args::parse();
    let image = if let Some(path) = &rom {
        std::fs::read(path)?
    } else {
        BUNDLED_CDI220B.to_vec()
    };
    let modules = cdi_os9::scan_modules(&image);
    let rom_type = cdi_os9::identify_rom(&modules);
    let detected_model = cdi_core::boards::model_by_id(rom_type.id)
        .ok_or_else(|| format!("no emulation model for ROM type {}", rom_type.id))?;
    let mut model = detected_model.clone();
    if let Some(standard) = video_standard {
        model.video = standard.into();
    }
    let title = format!("{APP_NAME} — {}", model.title);

    let dvc_path = dvc_rom;
    let (dvc, dvc_status) = if let Some(path) = &dvc_path {
        let firmware = std::fs::read(path)?;
        let config = cdi_core::DvcConfig::from_rom(firmware)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let status = format!(
            "DVC inserted — {}: {}",
            config.kind.name(),
            display_name(path)
        );
        (Some(config), status)
    } else {
        let config = cdi_core::DvcConfig::from_rom(BUNDLED_VMPEGA.to_vec())?;
        let status = format!("DVC inserted — {}: bundled vmpega.rom", config.kind.name());
        (Some(config), status)
    };
    let dvc_inserted = dvc.is_some();
    let mut machine = cdi_core::Machine::with_dvc(&model, image, dvc)?;
    let mut disc_status = "No disc inserted".to_owned();
    if let Some(cue) = disc {
        let disc = cdi_disc::DiscImage::load(&cue)?;
        log::info!(
            "disc inserted: {} track(s), lead-out {}",
            disc.tracks().len(),
            disc.leadout_msf()
        );
        machine.set_disc(Some(disc));
        disc_status = format!("Disc: {}", display_name(&cue));
    }
    let (fb_w, fb_h) = machine.bus.mcd212.visible_size();

    let shared = Arc::new(Shared {
        frame: Mutex::new(SharedFrame {
            pixels: vec![0; FB_WIDTH * FB_HEIGHT],
            width: fb_w,
            height: fb_h,
            border: 0,
            active_width: fb_w,
            frame_no: 0,
        }),
        input: Mutex::new(InputState::default()),
        command: Mutex::new(None),
        status: Mutex::new(disc_status),
        dvc_status: Mutex::new(dvc_status),
        dvc_path: Mutex::new(dvc_path),
        dvc_inserted: AtomicBool::new(dvc_inserted),
        pal: AtomicBool::new(model.video == cdi_core::VideoStandard::Pal),
        muted: Arc::new(AtomicBool::new(false)),
        running: AtomicBool::new(true),
        fps: Mutex::new(0.0),
    });

    let (audio_stream, audio_producer) = match start_audio(Arc::clone(&shared.muted)) {
        Ok(pair) => (Some(pair.0), Some(pair.1)),
        Err(error) => {
            log::warn!("audio output disabled: {error}");
            *shared.status.lock().unwrap() = format!("Audio disabled: {error}");
            (None, None)
        }
    };

    let emu_shared = Arc::clone(&shared);
    let emu_thread = std::thread::Builder::new()
        .name("emu".into())
        .spawn(move || emu_loop(machine, emu_shared, audio_producer))?;

    let mut viewport = egui::ViewportBuilder::default();
    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport: viewport
            // Stable id for the eframe preferences store regardless of the
            // ROM-dependent window title.
            .with_app_id("cdi-frontend")
            .with_inner_size([
                fb_w as f32,
                fb_h as f32
                    + if cfg!(target_os = "macos") {
                        24.0
                    } else {
                        48.0
                    },
            ])
            .with_title(&title),
        vsync: true,
        ..Default::default()
    };
    let app_shared = Arc::clone(&shared);
    let result = eframe::run_native(
        &title,
        options,
        Box::new(move |cc| Ok(Box::new(App::new(app_shared, cc)))),
    );

    shared.running.store(false, Ordering::Relaxed);
    let _ = emu_thread.join();
    drop(audio_stream);
    result.map_err(|e| e.to_string())?;
    Ok(())
}

fn display_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into(),
    )
}

fn start_audio(
    muted: Arc<AtomicBool>,
) -> Result<(cpal::Stream, Producer<i16>), Box<dyn std::error::Error>> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no default audio output device")?;
    let supported = device
        .supported_output_configs()?
        .find(|range| {
            range.channels() >= 2
                && range.min_sample_rate().0 <= AUDIO_RATE
                && range.max_sample_rate().0 >= AUDIO_RATE
        })
        .ok_or("output device does not support 44.1 kHz stereo")?;
    let sample_format = supported.sample_format();
    let config = supported
        .with_sample_rate(cpal::SampleRate(AUDIO_RATE))
        .config();
    let channels = usize::from(config.channels);
    let (producer, mut consumer) = RingBuffer::<i16>::new(AUDIO_RING_SAMPLES);
    let on_error = |error| log::warn!("audio stream error: {error}");

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                fill_audio(
                    data,
                    channels,
                    &mut consumer,
                    muted.load(Ordering::Relaxed),
                    |sample| f32::from(sample) / 32768.0,
                );
            },
            on_error,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config,
            move |data: &mut [i16], _| {
                fill_audio(
                    data,
                    channels,
                    &mut consumer,
                    muted.load(Ordering::Relaxed),
                    |sample| sample,
                );
            },
            on_error,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_output_stream(
            &config,
            move |data: &mut [u16], _| {
                fill_audio(
                    data,
                    channels,
                    &mut consumer,
                    muted.load(Ordering::Relaxed),
                    |sample| (i32::from(sample) + 32768) as u16,
                );
            },
            on_error,
            None,
        )?,
        format => return Err(format!("unsupported audio sample format {format}").into()),
    };
    stream.play()?;
    log::info!(
        "audio output: {} channels at {} Hz ({sample_format})",
        config.channels,
        config.sample_rate.0
    );
    Ok((stream, producer))
}

fn fill_audio<T: Copy>(
    output: &mut [T],
    channels: usize,
    consumer: &mut Consumer<i16>,
    muted: bool,
    convert: impl Fn(i16) -> T,
) {
    for frame in output.chunks_mut(channels) {
        let left = consumer.pop().unwrap_or(0);
        let right = consumer.pop().unwrap_or(0);
        let (left, right) = if muted { (0, 0) } else { (left, right) };
        for (channel, sample) in frame.iter_mut().enumerate() {
            *sample = convert(match channel {
                0 => left,
                1 => right,
                _ => 0,
            });
        }
    }
}

fn emu_loop(mut machine: cdi_core::Machine, shared: Arc<Shared>, mut audio: Option<Producer<i16>>) {
    let mut next_frame_deadline = Instant::now();
    let mut fps_window_start = Instant::now();
    let mut fps_frames = 0u32;

    while shared.running.load(Ordering::Relaxed) {
        if let Some(command) = shared.command.lock().unwrap().take() {
            match command {
                MachineCommand::LoadDisc(path) => match cdi_disc::DiscImage::load(&path) {
                    Ok(disc) => {
                        let tracks = disc.tracks().len();
                        machine.set_disc(Some(disc));
                        machine.reset();
                        *shared.status.lock().unwrap() =
                            format!("Disc: {} ({tracks} track(s))", display_name(&path));
                    }
                    Err(error) => {
                        *shared.status.lock().unwrap() = format!("Open failed: {error}");
                    }
                },
                MachineCommand::EjectDisc => {
                    machine.set_disc(None);
                    machine.reset();
                    *shared.status.lock().unwrap() = "No disc inserted".to_owned();
                }
                MachineCommand::AttachDvc(path) => {
                    let result = std::fs::read(&path)
                        .map_err(|error| error.to_string())
                        .and_then(cdi_core::DvcConfig::from_rom)
                        .and_then(|config| {
                            let kind = config.kind;
                            machine.attach_dvc(config)?;
                            Ok(kind)
                        });
                    match result {
                        Ok(kind) => {
                            *shared.dvc_path.lock().unwrap() = Some(path.clone());
                            shared.dvc_inserted.store(true, Ordering::Relaxed);
                            *shared.dvc_status.lock().unwrap() =
                                format!("DVC inserted — {}: {}", kind.name(), display_name(&path));
                        }
                        Err(error) => {
                            *shared.dvc_status.lock().unwrap() =
                                format!("DVC attach failed: {error}");
                        }
                    }
                }
                MachineCommand::AttachBundledDvc => {
                    let result =
                        cdi_core::DvcConfig::from_rom(BUNDLED_VMPEGA.to_vec()).and_then(|config| {
                            let kind = config.kind;
                            machine.attach_dvc(config)?;
                            Ok(kind)
                        });
                    match result {
                        Ok(kind) => {
                            *shared.dvc_path.lock().unwrap() = None;
                            shared.dvc_inserted.store(true, Ordering::Relaxed);
                            *shared.dvc_status.lock().unwrap() =
                                format!("DVC inserted — {}: bundled vmpega.rom", kind.name());
                        }
                        Err(error) => {
                            *shared.dvc_status.lock().unwrap() =
                                format!("DVC attach failed: {error}");
                        }
                    }
                }
                MachineCommand::DetachDvc => {
                    machine.detach_dvc();
                    shared.dvc_inserted.store(false, Ordering::Relaxed);
                    *shared.dvc_status.lock().unwrap() = "DVC removed".to_owned();
                }
                MachineCommand::SetVideoStandard(standard) => {
                    machine.set_video_standard(standard);
                    let pal = standard == cdi_core::VideoStandard::Pal;
                    shared.pal.store(pal, Ordering::Relaxed);
                    *shared.status.lock().unwrap() = format!(
                        "Machine reset — {}",
                        if pal { "PAL (50 Hz)" } else { "NTSC (60 Hz)" }
                    );
                }
                MachineCommand::Reset => {
                    machine.reset();
                    *shared.status.lock().unwrap() = "Machine reset".to_owned();
                }
            }
        }

        // Apply the latest pointer state.
        {
            let input = {
                let mut input = shared.input.lock().unwrap();
                let snapshot = *input;
                input.dx = 0;
                input.dy = 0;
                snapshot
            };
            machine
                .bus
                .slave
                .set_pointer(input.x, input.y, input.buttons);
            if input.dx != 0 || input.dy != 0 {
                machine
                    .bus
                    .slave
                    .move_pointer(input.dx, input.dy, input.buttons);
            }
        }

        // Run until the MCD212 completes the next frame.
        let target_frame = machine.bus.mcd212.frame_count + 1;
        let mut steps: u64 = 0;
        while machine.bus.mcd212.frame_count < target_frame && steps < 3_000_000 {
            machine.step();
            steps += 1;
        }

        let samples = machine.take_audio();
        if let Some(producer) = &mut audio {
            for sample in samples {
                if producer.push(sample).is_err() {
                    break;
                }
            }
        }

        // Publish the frame.
        {
            let mut frame = shared.frame.lock().unwrap();
            let (w, h) = machine.bus.mcd212.visible_size();
            frame.width = w;
            frame.height = h;
            frame.border = machine.bus.mcd212.border_width();
            frame.active_width = machine.bus.mcd212.screen_width();
            frame.pixels[..w * h].copy_from_slice(&machine.bus.mcd212.framebuffer()[..w * h]);
            frame.frame_no += 1;
        }

        fps_frames += 1;
        if fps_window_start.elapsed() >= Duration::from_secs(1) {
            *shared.fps.lock().unwrap() =
                fps_frames as f32 / fps_window_start.elapsed().as_secs_f32();
            fps_window_start = Instant::now();
            fps_frames = 0;
        }

        // Pace to real time (skip ahead if we fell behind).
        let frame_duration = if shared.pal.load(Ordering::Relaxed) {
            Duration::from_micros(20_000)
        } else {
            Duration::from_micros(16_667)
        };
        next_frame_deadline += frame_duration;
        let now = Instant::now();
        if next_frame_deadline > now {
            std::thread::sleep(next_frame_deadline - now);
        } else {
            next_frame_deadline = now;
        }
    }
}

struct App {
    shared: Arc<Shared>,
    texture: Option<egui::TextureHandle>,
    last_frame_no: u64,
    settings_open: bool,
    show_fps: bool,
    smooth_scaling: bool,
    capture_mouse_enabled: bool,
    mouse_captured: bool,
    suppress_capture_click: bool,
    game_buttons: u8,
    gamepad: Option<gilrs::Gilrs>,
    pad_buttons: u8,
    /// Sub-pixel remainder of gamepad pointer motion, carried across frames
    /// so slow stick deflections still move the pointer.
    pad_frac: egui::Vec2,
    pad_speed: f32,
    pad_deadzone: f32,
    pad_button1: gilrs::Button,
    pad_button2: gilrs::Button,
    /// CD-i button (0 or 1) awaiting a controller press to rebind.
    pad_rebind: Option<u8>,
    /// Sub-pixel remainder of captured-mouse motion carried across frames.
    capture_frac: egui::Vec2,
    capture_origin: Option<egui::Pos2>,
    capture_motion_grace: u8,
    #[cfg(target_os = "macos")]
    native_menu: NativeMenu,
}

impl App {
    fn new(shared: Arc<Shared>, cc: &eframe::CreationContext<'_>) -> Self {
        let prefs: Prefs = cc
            .storage
            .and_then(|storage| storage.get_string(PREFS_KEY))
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        Self {
            shared,
            texture: None,
            last_frame_no: 0,
            settings_open: false,
            show_fps: prefs.show_fps,
            smooth_scaling: prefs.smooth_scaling,
            capture_mouse_enabled: prefs.capture_mouse_enabled,
            mouse_captured: false,
            suppress_capture_click: false,
            game_buttons: 0,
            gamepad: match gilrs::Gilrs::new() {
                Ok(gilrs) => Some(gilrs),
                Err(err) => {
                    log::warn!("gamepad support unavailable: {err}");
                    None
                }
            },
            pad_buttons: 0,
            pad_frac: egui::Vec2::ZERO,
            pad_speed: prefs.pad_speed,
            pad_deadzone: prefs.pad_deadzone,
            pad_button1: prefs.pad_button1,
            pad_button2: prefs.pad_button2,
            pad_rebind: None,
            capture_frac: egui::Vec2::ZERO,
            capture_origin: None,
            capture_motion_grace: 0,
            #[cfg(target_os = "macos")]
            native_menu: NativeMenu::new().expect("initialize native macOS menu"),
        }
    }

    fn texture_options(&self) -> egui::TextureOptions {
        if self.smooth_scaling {
            egui::TextureOptions::LINEAR
        } else {
            egui::TextureOptions::NEAREST
        }
    }

    fn open_disc(&self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open a CD-i disc image")
            .add_filter("CUE sheets", &["cue"])
            .pick_file()
        {
            *self.shared.status.lock().unwrap() = format!("Loading {}…", display_name(&path));
            *self.shared.command.lock().unwrap() = Some(MachineCommand::LoadDisc(path));
        }
    }

    fn eject_disc(&self) {
        *self.shared.command.lock().unwrap() = Some(MachineCommand::EjectDisc);
    }

    fn reset_machine(&self) {
        *self.shared.command.lock().unwrap() = Some(MachineCommand::Reset);
    }

    fn choose_dvc(&self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Select DVC cartridge firmware")
            .add_filter("ROM images", &["rom", "bin"])
            .pick_file()
        {
            *self.shared.dvc_status.lock().unwrap() = format!("Inserting {}…", display_name(&path));
            *self.shared.command.lock().unwrap() = Some(MachineCommand::AttachDvc(path));
        }
    }

    fn insert_dvc(&self) {
        let path = self.shared.dvc_path.lock().unwrap().clone();
        if let Some(path) = path {
            *self.shared.dvc_status.lock().unwrap() = format!("Inserting {}…", display_name(&path));
            *self.shared.command.lock().unwrap() = Some(MachineCommand::AttachDvc(path));
        } else {
            *self.shared.dvc_status.lock().unwrap() = "Inserting bundled vmpega.rom…".to_owned();
            *self.shared.command.lock().unwrap() = Some(MachineCommand::AttachBundledDvc);
        }
    }

    fn remove_dvc(&self) {
        *self.shared.command.lock().unwrap() = Some(MachineCommand::DetachDvc);
    }

    fn set_mouse_capture(&mut self, ctx: &egui::Context, captured: bool) {
        if self.mouse_captured == captured {
            return;
        }
        self.mouse_captured = captured;
        if captured {
            self.capture_origin = ctx.input(|input| input.pointer.latest_pos());
            self.capture_motion_grace = 2;
            if let Some(center) = ctx.input(|input| {
                input
                    .viewport()
                    .inner_rect
                    .map(|rect| egui::pos2(rect.width() * 0.5, rect.height() * 0.5))
            }) {
                // Normalize the hidden host cursor before locking it so raw
                // relative travel never depends on where capture began.
                ctx.send_viewport_cmd(egui::ViewportCommand::CursorPosition(center));
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(
                egui::viewport::CursorGrab::Locked,
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(false));
            // Capturing is a frontend action, not a CD-i click. Clear any
            // button held over from the shell launch/capture gesture so a
            // newly booted title cannot consume it as its first selection.
            self.shared.input.lock().unwrap().buttons = 0;
            self.capture_frac = egui::Vec2::ZERO;
        } else {
            self.suppress_capture_click = false;
            self.game_buttons = 0;
            self.capture_motion_grace = 0;
            self.shared.input.lock().unwrap().buttons = 0;
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(
                egui::viewport::CursorGrab::None,
            ));
            if let Some(origin) = self.capture_origin.take() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CursorPosition(origin));
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(true));
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Audio");
        let mut muted = self.shared.muted.load(Ordering::Relaxed);
        if ui.checkbox(&mut muted, "Mute audio").changed() {
            self.shared.muted.store(muted, Ordering::Relaxed);
        }
        ui.label("44.1 kHz stereo output");
        ui.separator();
        ui.heading("DVC Cartridge");
        ui.label(self.shared.dvc_status.lock().unwrap().as_str());
        ui.horizontal(|ui| {
            if self.shared.dvc_inserted.load(Ordering::Relaxed) {
                if ui.button("Remove DVC Cartridge").clicked() {
                    self.remove_dvc();
                }
            } else if ui.button("Insert DVC Cartridge").clicked() {
                self.insert_dvc();
            }
            if ui.button("Choose Firmware…").clicked() {
                self.choose_dvc();
            }
        });
        ui.label(
            "The bundled VMPEG cartridge is inserted by default. Inserting or removing it resets the machine and retains the disc.",
        );
        ui.separator();
        ui.heading("Display");
        let mut pal = self.shared.pal.load(Ordering::Relaxed);
        ui.horizontal(|ui| {
            ui.label("Video standard:");
            ui.radio_value(&mut pal, true, "PAL (50 Hz)");
            ui.radio_value(&mut pal, false, "NTSC (60 Hz)");
        });
        if pal != self.shared.pal.load(Ordering::Relaxed) {
            let standard = if pal {
                cdi_core::VideoStandard::Pal
            } else {
                cdi_core::VideoStandard::Ntsc
            };
            *self.shared.command.lock().unwrap() = Some(MachineCommand::SetVideoStandard(standard));
        }
        ui.checkbox(&mut self.smooth_scaling, "Smooth scaling");
        ui.checkbox(&mut self.show_fps, "Show frame rate");
        ui.separator();
        ui.heading("Input");
        ui.checkbox(&mut self.capture_mouse_enabled, "Capture mouse on click");
        ui.separator();
        ui.heading("Controller");
        match &self.gamepad {
            Some(gilrs) => {
                let names: Vec<String> = gilrs
                    .gamepads()
                    .map(|(_, pad)| pad.name().to_owned())
                    .collect();
                if names.is_empty() {
                    ui.label("No controller connected");
                } else {
                    ui.label(format!("Connected: {}", names.join(", ")));
                }
                ui.add(egui::Slider::new(&mut self.pad_speed, 1.0..=20.0).text("Pointer speed"));
                ui.add(egui::Slider::new(&mut self.pad_deadzone, 0.0..=0.5).text("Stick deadzone"));
                ui.horizontal(|ui| {
                    ui.label("CD-i button 1:");
                    let text = if self.pad_rebind == Some(0) {
                        "Press a controller button…"
                    } else {
                        button_name(self.pad_button1)
                    };
                    if ui.button(text).clicked() {
                        self.pad_rebind = if self.pad_rebind == Some(0) {
                            None
                        } else {
                            Some(0)
                        };
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("CD-i button 2:");
                    let text = if self.pad_rebind == Some(1) {
                        "Press a controller button…"
                    } else {
                        button_name(self.pad_button2)
                    };
                    if ui.button(text).clicked() {
                        self.pad_rebind = if self.pad_rebind == Some(1) {
                            None
                        } else {
                            Some(1)
                        };
                    }
                });
                if ui.button("Reset controller defaults").clicked() {
                    let defaults = Prefs::default();
                    self.pad_speed = defaults.pad_speed;
                    self.pad_deadzone = defaults.pad_deadzone;
                    self.pad_button1 = defaults.pad_button1;
                    self.pad_button2 = defaults.pad_button2;
                    self.pad_rebind = None;
                }
                ui.label(
                    "Left stick and d-pad move the pointer. Layouts are auto-detected via the open-source SDL_GameControllerDB mappings bundled with gilrs.",
                );
            }
            None => {
                ui.label("Controller support unavailable on this system");
            }
        }
    }

    /// Poll connected gamepads and translate them onto the CD-i pointer:
    /// left stick and d-pad move, two configurable buttons map to CD-i
    /// buttons 1/2.
    fn poll_gamepad(&mut self) {
        let Some(gilrs) = self.gamepad.as_mut() else {
            return;
        };
        // Drain the event queue so cached gamepad state stays current; a
        // pending rebind captures the first button press seen here.
        while let Some(event) = gilrs.next_event() {
            if let gilrs::EventType::ButtonPressed(button, _) = event.event {
                match self.pad_rebind {
                    Some(0) => {
                        self.pad_button1 = button;
                        self.pad_rebind = None;
                    }
                    Some(1) => {
                        self.pad_button2 = button;
                        self.pad_rebind = None;
                    }
                    _ => {}
                }
            }
        }

        let deadzone = self.pad_deadzone;
        let mut deflection = egui::Vec2::ZERO;
        let mut buttons = 0u8;
        for (_id, pad) in gilrs.gamepads() {
            let x = pad.value(gilrs::Axis::LeftStickX);
            let y = pad.value(gilrs::Axis::LeftStickY);
            if x.abs() > deadzone {
                deflection.x += x;
            }
            if y.abs() > deadzone {
                // Stick up moves the pointer up (screen Y grows downward).
                deflection.y -= y;
            }
            if pad.is_pressed(gilrs::Button::DPadLeft) {
                deflection.x -= 1.0;
            }
            if pad.is_pressed(gilrs::Button::DPadRight) {
                deflection.x += 1.0;
            }
            if pad.is_pressed(gilrs::Button::DPadUp) {
                deflection.y -= 1.0;
            }
            if pad.is_pressed(gilrs::Button::DPadDown) {
                deflection.y += 1.0;
            }
            if pad.is_pressed(self.pad_button1) {
                buttons |= 1;
            }
            if pad.is_pressed(self.pad_button2) {
                buttons |= 2;
            }
        }
        // Don't feed the press being captured for a rebind into the game.
        if self.pad_rebind.is_some() {
            buttons = 0;
        }

        let delta = deflection.clamp(egui::vec2(-1.0, -1.0), egui::vec2(1.0, 1.0)) * self.pad_speed
            + self.pad_frac;
        let step = egui::vec2(delta.x.trunc(), delta.y.trunc());
        self.pad_frac = delta - step;

        let buttons_changed = buttons != self.pad_buttons;
        self.pad_buttons = buttons;
        if step == egui::Vec2::ZERO && !buttons_changed {
            return;
        }

        let mut input = self.shared.input.lock().unwrap();
        input.dx += step.x as i32;
        input.dy += step.y as i32;
        input.buttons = self.game_buttons | self.pad_buttons;
    }

    #[cfg(target_os = "macos")]
    fn handle_native_menu(&mut self) {
        for action in self.native_menu.actions() {
            match action {
                NativeMenuAction::Open => self.open_disc(),
                NativeMenuAction::Eject => self.eject_disc(),
                NativeMenuAction::Reset => self.reset_machine(),
                NativeMenuAction::Settings => self.settings_open = true,
            }
        }
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let prefs = Prefs {
            show_fps: self.show_fps,
            smooth_scaling: self.smooth_scaling,
            capture_mouse_enabled: self.capture_mouse_enabled,
            pad_speed: self.pad_speed,
            pad_deadzone: self.pad_deadzone,
            pad_button1: self.pad_button1,
            pad_button2: self.pad_button2,
        };
        if let Ok(json) = serde_json::to_string(&prefs) {
            storage.set_string(PREFS_KEY, json);
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let escape_pressed =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let release_capture =
            escape_pressed || ctx.input(|input| input.viewport().focused == Some(false));
        if self.mouse_captured && release_capture {
            self.set_mouse_capture(ctx, false);
        }
        if self.mouse_captured {
            // Reassert this while captured. On macOS the locked mode
            // disconnects hardware motion from the hidden system cursor;
            // keeping it active prevents motion from stopping at a screen
            // edge even if another viewport command re-associated it.
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(
                egui::viewport::CursorGrab::Locked,
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(false));
        }

        #[cfg(target_os = "macos")]
        self.handle_native_menu();

        #[cfg(not(target_os = "macos"))]
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open…").clicked() {
                        ui.close_menu();
                        self.open_disc();
                    }
                    if ui.button("Eject Disc").clicked() {
                        ui.close_menu();
                        self.eject_disc();
                    }
                    if ui.button("Reset").clicked() {
                        ui.close_menu();
                        self.reset_machine();
                    }
                    ui.separator();
                    if ui.button("Settings…").clicked() {
                        ui.close_menu();
                        self.settings_open = true;
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ui.close_menu();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        });

        if self.settings_open {
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("settings"),
                egui::ViewportBuilder::default()
                    .with_title("Settings")
                    .with_inner_size([400.0, 480.0])
                    .with_min_inner_size([320.0, 240.0]),
                |ctx, class| {
                    if class == egui::ViewportClass::Embedded {
                        // Backend without multi-viewport support: fall back to
                        // the overlay window.
                        let mut open = true;
                        egui::Window::new("Settings")
                            .open(&mut open)
                            .resizable(false)
                            .show(ctx, |ui| self.settings_ui(ui));
                        if !open {
                            self.settings_open = false;
                        }
                    } else {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false; 2])
                                .show(ui, |ui| self.settings_ui(ui));
                        });
                        if ctx.input(|input| input.viewport().close_requested()) {
                            self.settings_open = false;
                        }
                    }
                },
            );
        }
        if !self.capture_mouse_enabled && self.mouse_captured {
            self.set_mouse_capture(ctx, false);
        }

        // Upload the newest emulator frame.
        {
            let frame = self.shared.frame.lock().unwrap();
            if frame.frame_no != self.last_frame_no {
                self.last_frame_no = frame.frame_no;
                let (w, h) = (frame.width, frame.height);
                let pixels: Vec<egui::Color32> = frame.pixels[..w * h]
                    .iter()
                    .map(|&px| {
                        let px = presentation_rgb(px);
                        egui::Color32::from_rgb((px >> 16) as u8, (px >> 8) as u8, px as u8)
                    })
                    .collect();
                let image = egui::ColorImage {
                    size: [w, h],
                    pixels,
                };
                let texture_options = self.texture_options();
                match &mut self.texture {
                    Some(tex) => tex.set(image, texture_options),
                    None => {
                        self.texture = Some(ctx.load_texture("screen", image, texture_options));
                    }
                }
            }
        }

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(self.shared.status.lock().unwrap().as_str());
                if self.mouse_captured {
                    ui.weak("Esc releases the mouse");
                } else if self.capture_mouse_enabled {
                    ui.weak("Click the screen to capture the mouse");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.show_fps {
                        ui.weak(format!("{:.0} fps", *self.shared.fps.lock().unwrap()));
                    }
                });
            });
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                let Some(texture) = &self.texture else {
                    return;
                };
                let avail = ui.available_size();
                let tex_size = texture.size_vec2();
                let scale = (avail.x / tex_size.x).min(avail.y / tex_size.y).max(0.1);
                let size = tex_size * scale;
                let rect = egui::Rect::from_center_size(ui.max_rect().center(), size);
                let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );

                let (border, active_w) = {
                    let frame = self.shared.frame.lock().unwrap();
                    (frame.border as f32, frame.active_width as f32)
                };
                let capture_pressed = self.capture_mouse_enabled
                    && !self.mouse_captured
                    && response.hovered()
                    && ctx
                        .input(|input| input.pointer.button_pressed(egui::PointerButton::Primary));
                if capture_pressed {
                    self.set_mouse_capture(ctx, true);
                    self.suppress_capture_click = true;
                }

                let (primary_pressed, primary_released, secondary_pressed, secondary_released) =
                    ctx.input(|input| {
                        (
                            input.pointer.button_pressed(egui::PointerButton::Primary),
                            input.pointer.button_released(egui::PointerButton::Primary),
                            input.pointer.button_pressed(egui::PointerButton::Secondary),
                            input
                                .pointer
                                .button_released(egui::PointerButton::Secondary),
                        )
                    });
                if primary_released {
                    self.game_buttons &= !1;
                    self.suppress_capture_click = false;
                }
                if secondary_released {
                    self.game_buttons &= !2;
                }
                if self.mouse_captured {
                    if primary_pressed && !self.suppress_capture_click {
                        self.game_buttons |= 1;
                    }
                    if secondary_pressed {
                        self.game_buttons |= 2;
                    }
                } else if !self.capture_mouse_enabled && response.hovered() {
                    if primary_pressed {
                        self.game_buttons |= 1;
                    }
                    if secondary_pressed {
                        self.game_buttons |= 2;
                    }
                }

                if self.mouse_captured {
                    // Raw motion is in physical pixels. Convert through egui
                    // points and the displayed active-picture size into the
                    // CD-i pointer's 768x560 coordinate space.
                    let motion = if self.capture_motion_grace == 0 {
                        ctx.input(|input| input.pointer.motion().unwrap_or(input.pointer.delta()))
                    } else {
                        self.capture_motion_grace -= 1;
                        egui::Vec2::ZERO
                    };
                    // macOS NSEvent deltas are already logical points. Other
                    // winit backends report physical/raw units here.
                    let motion_points = if cfg!(target_os = "macos") {
                        motion
                    } else {
                        motion / ctx.pixels_per_point()
                    };
                    let x_scale = tex_size.x * 768.0 / (size.x * active_w);
                    let y_scale = 560.0 / size.y;
                    let scaled = egui::vec2(motion_points.x * x_scale, motion_points.y * y_scale)
                        + self.capture_frac;
                    let step = egui::vec2(scaled.x.trunc(), scaled.y.trunc());
                    self.capture_frac = scaled - step;
                    if motion != egui::Vec2::ZERO {
                        log::trace!(
                            "captured mouse raw=({:.2},{:.2}) cdi delta=({:.0},{:.0})",
                            motion.x,
                            motion.y,
                            step.x,
                            step.y
                        );
                    }
                    let mut input = self.shared.input.lock().unwrap();
                    input.dx += step.x as i32;
                    input.dy += step.y as i32;
                    input.buttons = self.game_buttons | self.pad_buttons;
                } else if !self.capture_mouse_enabled
                    && ctx.input(|input| input.pointer.delta() != egui::Vec2::ZERO)
                {
                    if let Some(pos) = response.hover_pos() {
                        // Uncaptured mode maps the system pointer onto the active
                        // picture. Only actual pointer motion updates coordinates:
                        // video-mode/layout changes under a stationary cursor are
                        // not CD-i mouse movement.
                        let uv = (pos - rect.min) / size;
                        let fx = uv.x * tex_size.x;
                        let x = (((fx - border) * 768.0) / active_w).clamp(0.0, 767.0) as i32;
                        let y = (uv.y * tex_size.y).clamp(0.0, 559.0) as i32;
                        let mut input = self.shared.input.lock().unwrap();
                        input.x = x;
                        input.y = y;
                        input.buttons = self.game_buttons | self.pad_buttons;
                    }
                }
            });

        if self.mouse_captured {
            const KEYBOARD_STEP: f32 = 6.0;
            let keyboard_delta = ctx.input(|input| {
                egui::vec2(
                    f32::from(input.key_down(egui::Key::ArrowRight))
                        - f32::from(input.key_down(egui::Key::ArrowLeft)),
                    f32::from(input.key_down(egui::Key::ArrowDown))
                        - f32::from(input.key_down(egui::Key::ArrowUp)),
                ) * KEYBOARD_STEP
            });
            if keyboard_delta != egui::Vec2::ZERO {
                let mut input = self.shared.input.lock().unwrap();
                input.dx += keyboard_delta.x as i32;
                input.dy += keyboard_delta.y as i32;
            }
        }

        self.poll_gamepad();

        ctx.request_repaint_after(Duration::from_millis(10));
    }
}
