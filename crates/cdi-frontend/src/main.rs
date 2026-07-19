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
    running: AtomicBool,
    /// Emulated frames per second (diagnostics).
    fps: Mutex<f32>,
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
    if let Some(cue) = args.disc {
        let disc = cdi_disc::DiscImage::load(&cue)?;
        log::info!(
            "disc inserted: {} track(s), lead-out {}",
            disc.tracks().len(),
            disc.leadout_msf()
        );
        machine.set_disc(Some(disc));
    }
    let (fb_w, fb_h) = machine.bus.mcd212.visible_size();

    let (audio_stream, audio_producer) = match start_audio() {
        Ok(pair) => (Some(pair.0), Some(pair.1)),
        Err(error) => {
            log::warn!("audio output disabled: {error}");
            (None, None)
        }
    };

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
        running: AtomicBool::new(true),
        fps: Mutex::new(0.0),
    });

    let emu_shared = Arc::clone(&shared);
    let pal = model.video == cdi_core::VideoStandard::Pal;
    let emu_thread = std::thread::Builder::new()
        .name("emu".into())
        .spawn(move || emu_loop(machine, emu_shared, pal, audio_producer))?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([fb_w as f32, fb_h as f32 + 24.0])
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

fn start_audio() -> Result<(cpal::Stream, Producer<i16>), Box<dyn std::error::Error>> {
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
                fill_audio(data, channels, &mut consumer, |sample| {
                    f32::from(sample) / 32768.0
                });
            },
            on_error,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config,
            move |data: &mut [i16], _| {
                fill_audio(data, channels, &mut consumer, |sample| sample);
            },
            on_error,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_output_stream(
            &config,
            move |data: &mut [u16], _| {
                fill_audio(data, channels, &mut consumer, |sample| {
                    (i32::from(sample) + 32768) as u16
                });
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
    convert: impl Fn(i16) -> T,
) {
    for frame in output.chunks_mut(channels) {
        let left = consumer.pop().unwrap_or(0);
        let right = consumer.pop().unwrap_or(0);
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
}

impl App {
    fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            texture: None,
            last_frame_no: 0,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                match &mut self.texture {
                    Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
                    None => {
                        self.texture =
                            Some(ctx.load_texture("screen", image, egui::TextureOptions::NEAREST));
                    }
                }
            }
        }

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{:.1} fps", *self.shared.fps.lock().unwrap()));
                ui.label("Mouse: CD-i pointer — left/right buttons");
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

                // Map hover position to CD-i pointer coordinates. The
                // pointer-device range (0..767) covers the *active* picture
                // area only — exclude the side borders and rescale.
                if let Some(pos) = response.hover_pos() {
                    let (border, active_w) = {
                        let frame = self.shared.frame.lock().unwrap();
                        (frame.border as f32, frame.active_width as f32)
                    };
                    let uv = (pos - rect.min) / size;
                    let fx = uv.x * tex_size.x;
                    let x = (((fx - border) * 768.0) / active_w).clamp(0.0, 767.0) as i32;
                    let y = (uv.y * tex_size.y).clamp(0.0, 559.0) as i32;
                    let buttons = {
                        let input = ctx.input(|i| {
                            (
                                i.pointer.button_down(egui::PointerButton::Primary),
                                i.pointer.button_down(egui::PointerButton::Secondary),
                            )
                        });
                        u8::from(input.0) | (u8::from(input.1) << 1)
                    };
                    let mut input = self.shared.input.lock().unwrap();
                    input.x = x;
                    input.y = y;
                    input.buttons = buttons;
                }
            });

        ctx.request_repaint_after(Duration::from_millis(10));
    }
}
