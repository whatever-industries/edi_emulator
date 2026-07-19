// SPDX-License-Identifier: GPL-2.0-or-later
//! Desktop frontend: renders the MCD212 framebuffer in an eframe window and
//! feeds mouse input to the SLAVE pointer device.
//!
//! The emulator core runs on its own thread, paced to real time by frame
//! count; the UI thread copies the latest completed frame into a texture.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cdi_core::mcd212::{FB_HEIGHT, FB_WIDTH};
use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, Producer, RingBuffer};

const AUDIO_RATE: u32 = 44_100;
const AUDIO_RING_SAMPLES: usize = AUDIO_RATE as usize * 2;

#[derive(Parser)]
#[command(name = "cdi-frontend", about = "CD-i emulator desktop frontend")]
struct Args {
    /// CD-i system ROM; opens a file picker when omitted.
    rom: Option<PathBuf>,
    /// CUE sheet of a disc image to insert.
    #[arg(long)]
    disc: Option<PathBuf>,
}

#[derive(Default, Clone, Copy)]
struct InputState {
    x: i32,
    y: i32,
    buttons: u8,
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
    muted: Arc<AtomicBool>,
    running: AtomicBool,
    /// Emulated frames per second (diagnostics).
    fps: Mutex<f32>,
}

enum MachineCommand {
    LoadDisc(PathBuf),
    EjectDisc,
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
            "CD-i Emulator",
            true,
            &[
                &PredefinedMenuItem::about(Some("About CD-i Emulator"), None),
                &app_settings,
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::quit(Some("Quit CD-i Emulator")),
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

    let args = Args::parse();
    let rom_path = args.rom.or_else(|| {
        rfd::FileDialog::new()
            .set_title("Select a CD-i system ROM")
            .add_filter("ROM images", &["rom", "bin"])
            .pick_file()
    });
    let Some(rom_path) = rom_path else {
        eprintln!("usage: cdi-frontend <system-rom> [--disc <cue>]");
        std::process::exit(2);
    };

    let image = std::fs::read(&rom_path)?;
    let modules = cdi_os9::scan_modules(&image);
    let rom_type = cdi_os9::identify_rom(&modules);
    let model = cdi_core::boards::model_by_id(rom_type.id)
        .ok_or_else(|| format!("no emulation model for ROM type {}", rom_type.id))?;
    let title = format!("CD-i Emulator — {}", model.title);

    let mut machine = cdi_core::Machine::new(model, image)?;
    let mut disc_status = "No disc inserted".to_owned();
    if let Some(cue) = args.disc {
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
    let pal = model.video == cdi_core::VideoStandard::Pal;
    let emu_thread = std::thread::Builder::new()
        .name("emu".into())
        .spawn(move || emu_loop(machine, emu_shared, pal, audio_producer))?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
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
        Box::new(move |_cc| Ok(Box::new(App::new(app_shared)))),
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

fn emu_loop(
    mut machine: cdi_core::Machine,
    shared: Arc<Shared>,
    pal: bool,
    mut audio: Option<Producer<i16>>,
) {
    let frame_duration = if pal {
        Duration::from_micros(20_000)
    } else {
        Duration::from_micros(16_667)
    };
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
                MachineCommand::Reset => {
                    machine.reset();
                    *shared.status.lock().unwrap() = "Machine reset".to_owned();
                }
            }
        }

        // Apply the latest pointer state.
        {
            let input = *shared.input.lock().unwrap();
            machine
                .bus
                .slave
                .set_pointer(input.x, input.y, input.buttons);
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
    captured_x: f32,
    captured_y: f32,
    #[cfg(target_os = "macos")]
    native_menu: NativeMenu,
}

impl App {
    fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            texture: None,
            last_frame_no: 0,
            settings_open: false,
            show_fps: true,
            smooth_scaling: false,
            capture_mouse_enabled: true,
            mouse_captured: false,
            suppress_capture_click: false,
            game_buttons: 0,
            captured_x: 0.0,
            captured_y: 0.0,
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

    fn set_mouse_capture(&mut self, ctx: &egui::Context, captured: bool) {
        if self.mouse_captured == captured {
            return;
        }
        self.mouse_captured = captured;
        if !captured {
            self.suppress_capture_click = false;
            self.game_buttons = 0;
            self.shared.input.lock().unwrap().buttons = 0;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(if captured {
            egui::viewport::CursorGrab::Locked
        } else {
            egui::viewport::CursorGrab::None
        }));
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(!captured));
        if captured {
            let mut input = self.shared.input.lock().unwrap();
            // Capturing is a frontend action, not a CD-i click. Clear any
            // button held over from the shell launch/capture gesture so a
            // newly booted title cannot consume it as its first selection.
            input.buttons = 0;
            self.captured_x = input.x as f32;
            self.captured_y = input.y as f32;
        }
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
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let escape_pressed =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let release_capture =
            escape_pressed || ctx.input(|input| input.viewport().focused == Some(false));
        if self.mouse_captured && release_capture {
            self.set_mouse_capture(ctx, false);
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

        let mut settings_open = self.settings_open;
        egui::Window::new("Settings")
            .open(&mut settings_open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Audio");
                let mut muted = self.shared.muted.load(Ordering::Relaxed);
                if ui.checkbox(&mut muted, "Mute audio").changed() {
                    self.shared.muted.store(muted, Ordering::Relaxed);
                }
                ui.label("44.1 kHz stereo output");
                ui.separator();
                ui.heading("Display");
                ui.checkbox(&mut self.smooth_scaling, "Smooth scaling");
                ui.checkbox(&mut self.show_fps, "Show frame rate");
                ui.separator();
                ui.heading("Input");
                ui.checkbox(&mut self.capture_mouse_enabled, "Capture mouse on click");
            });
        self.settings_open = settings_open;
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
                    .map(|&px| egui::Color32::from_rgb((px >> 16) as u8, (px >> 8) as u8, px as u8))
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
                if self.show_fps {
                    ui.label(format!("{:.1} fps", *self.shared.fps.lock().unwrap()));
                }
                ui.label(self.shared.status.lock().unwrap().as_str());
                if self.mouse_captured {
                    let input = *self.shared.input.lock().unwrap();
                    ui.label(format!(
                        "Mouse captured — {},{} — arrows move — Esc releases",
                        input.x, input.y
                    ));
                } else if self.capture_mouse_enabled {
                    ui.label("Mouse: click screen to capture — arrows move when captured");
                } else {
                    ui.label("Mouse: direct mapping — capture disabled in Settings");
                }
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
                    let motion =
                        ctx.input(|input| input.pointer.motion().unwrap_or(input.pointer.delta()));
                    let points_per_physical_pixel = 1.0 / ctx.pixels_per_point();
                    let x_scale = tex_size.x * 768.0 / (size.x * active_w);
                    let y_scale = 560.0 / size.y;
                    self.captured_x = (self.captured_x
                        + motion.x * points_per_physical_pixel * x_scale)
                        .clamp(0.0, 767.0);
                    self.captured_y = (self.captured_y
                        + motion.y * points_per_physical_pixel * y_scale)
                        .clamp(0.0, 559.0);
                    if motion != egui::Vec2::ZERO {
                        log::trace!(
                            "captured mouse raw=({:.2},{:.2}) cdi=({:.2},{:.2})",
                            motion.x,
                            motion.y,
                            self.captured_x,
                            self.captured_y
                        );
                    }
                    let mut input = self.shared.input.lock().unwrap();
                    input.x = self.captured_x.round() as i32;
                    input.y = self.captured_y.round() as i32;
                    input.buttons = self.game_buttons;
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
                        input.buttons = self.game_buttons;
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
                self.captured_x = (self.captured_x + keyboard_delta.x).clamp(0.0, 767.0);
                self.captured_y = (self.captured_y + keyboard_delta.y).clamp(0.0, 559.0);
                let mut input = self.shared.input.lock().unwrap();
                input.x = self.captured_x.round() as i32;
                input.y = self.captured_y.round() as i32;
            }
        }

        ctx.request_repaint_after(Duration::from_millis(10));
    }
}
