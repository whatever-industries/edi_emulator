// SPDX-License-Identifier: GPL-3.0-or-later
//! Desktop frontend: renders the MCD212 framebuffer in an eframe window and
//! feeds mouse and gamepad input to the SLAVE pointer device.
//!
//! The emulator core runs on its own thread, paced to real time by frame
//! count; the UI thread copies the latest completed frame into a texture.

mod disc_profiles;
mod store_zip;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use cdi_core::mcd212::{presentation_rgb, DisplayGeometry, FB_HEIGHT, FB_WIDTH};
use clap::{Parser, ValueEnum};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, Producer, RingBuffer};

use disc_profiles::{DiscIdentity, VideoStandardRecommendation};

const AUDIO_RATE: u32 = 44_100;
const AUDIO_RING_SAMPLES: usize = AUDIO_RATE as usize * 2;
const APP_NAME: &str = "E-Di: Emulator Disc Interactive";
const UI_BACKGROUND: egui::Color32 = egui::Color32::from_rgb(15, 17, 20);
const UI_SURFACE: egui::Color32 = egui::Color32::from_rgb(23, 26, 30);
const UI_SURFACE_HOVER: egui::Color32 = egui::Color32::from_rgb(34, 39, 45);
const UI_BORDER: egui::Color32 = egui::Color32::from_rgb(46, 51, 58);
const UI_ACCENT: egui::Color32 = egui::Color32::from_rgb(67, 82, 98);
const UI_MUTED_TEXT: egui::Color32 = egui::Color32::from_rgb(145, 151, 160);
const PARENTAL_PASSCODE_COLOR: egui::Color32 = egui::Color32::from_rgb(184, 126, 70);
const PARENTAL_PASSCODE_BACKGROUND: egui::Color32 = egui::Color32::from_rgb(50, 39, 29);
const BUNDLED_CDI220B: &[u8] = include_bytes!("../../../firmware/cdi220b.rom");
const BUNDLED_VMPEGA: &[u8] = include_bytes!("../../../firmware/vmpega.rom");

fn configure_ui(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 7.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = UI_BACKGROUND;
    style.visuals.window_fill = UI_BACKGROUND;
    style.visuals.faint_bg_color = UI_SURFACE;
    style.visuals.extreme_bg_color = egui::Color32::from_rgb(9, 10, 12);
    style.visuals.selection.bg_fill = UI_ACCENT;
    style.visuals.widgets.inactive.weak_bg_fill = UI_SURFACE;
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, UI_BORDER);
    style.visuals.widgets.hovered.weak_bg_fill = UI_SURFACE_HOVER;
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, UI_ACCENT);
    style.visuals.widgets.active.weak_bg_fill = UI_ACCENT;
    ctx.set_style(style);
}

fn chrome_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(UI_SURFACE)
        .stroke(egui::Stroke::new(1.0, UI_BORDER))
        .inner_margin(egui::Margin::symmetric(10, 5))
}

/// A selectable bundled system ROM.
///
/// Region is a property of the *player*, not the ROM bytes: the same image
/// shipped in 50 Hz and 60 Hz machines, with the standard set by hardware
/// configuration and reported through the SLAVE `F6` query. The region here
/// names the market the model was sold into and only supplies the *default*
/// video standard, which stays user-overridable.
struct SystemRom {
    label: &'static str,
    region: &'static str,
    bytes: &'static [u8],
    /// Default video standard implied by the market.
    pal: bool,
    /// Whether this ROM's board is emulated; unsupported ones are listed so
    /// they can be tried, but report a clear message instead of booting.
    emulated: bool,
}

const SYSTEM_ROMS: &[SystemRom] = &[
    SystemRom {
        label: "CD-i 220 F2",
        region: "Europe",
        bytes: BUNDLED_CDI220B,
        pal: true,
        emulated: true,
    },
    SystemRom {
        label: "CD-i 220",
        region: "Europe",
        bytes: include_bytes!("../../../firmware/cdi220.rom"),
        pal: true,
        emulated: true,
    },
    SystemRom {
        label: "CD-i 200 F1",
        region: "Europe",
        bytes: include_bytes!("../../../firmware/cdi200.rom"),
        pal: true,
        emulated: true,
    },
    SystemRom {
        label: "CD-i 490",
        region: "Europe",
        bytes: include_bytes!("../../../firmware/cdi490a.rom"),
        pal: true,
        emulated: false,
    },
    SystemRom {
        label: "CD-i 910",
        region: "USA",
        bytes: include_bytes!("../../../firmware/cdi910.rom"),
        pal: false,
        emulated: false,
    },
];

/// Guess the player video standard from a disc's release-region tag, as used
/// by the common `Title (Region)` dump naming.
///
/// This is a naming convention, not disc content: region is a property of the
/// player, and cross-region pressings share one image across markets. It is a
/// convenience default only, and always user-overridable.
fn region_is_pal(disc_name: &str) -> Option<bool> {
    const NTSC: &[&str] = &[
        "usa", "u.s.a", "us", "japan", "jp", "canada", "korea", "taiwan", "mexico", "brazil",
    ];
    const PAL: &[&str] = &[
        "europe",
        "germany",
        "uk",
        "united kingdom",
        "france",
        "italy",
        "spain",
        "netherlands",
        "belgium",
        "sweden",
        "australia",
        "austria",
        "switzerland",
        "denmark",
        "norway",
        "finland",
        "poland",
        "portugal",
        "ireland",
    ];
    let lower = disc_name.to_lowercase();
    let mut found_ntsc = false;
    let mut found_pal = false;
    // Scan parenthesised tags, e.g. "Title (Europe) (Rev 2)".
    for tag in lower.split('(').skip(1) {
        let tag = tag.split(')').next().unwrap_or("");
        for part in tag.split(',').map(str::trim) {
            if NTSC.contains(&part) {
                found_ntsc = true;
            }
            if PAL.contains(&part) {
                found_pal = true;
            }
        }
    }
    match (found_pal, found_ntsc) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        // A cross-region pressing does not identify the target player. Keep
        // the user's current standard instead of trusting whichever region
        // happens to be listed first in the filename.
        _ => None,
    }
}

fn presentation_aspect(
    aperture: DisplayAperture,
    geometry: DisplayGeometry,
    crt_aspect: bool,
) -> f32 {
    if aperture.height == 0 {
        return 4.0 / 3.0;
    }
    if !crt_aspect || geometry.pixel_aspect_num == 0 {
        return aperture.width as f32 / aperture.height as f32;
    }
    aperture.width as f32 * geometry.pixel_aspect_den as f32
        / (aperture.height as f32 * geometry.pixel_aspect_num as f32)
}

fn fit_aspect(available: egui::Vec2, aspect: f32) -> egui::Vec2 {
    if available.x <= 0.0 || available.y <= 0.0 || aspect <= 0.0 {
        return egui::Vec2::ZERO;
    }
    let width = available.x.min(available.y * aspect);
    egui::vec2(width, width / aspect)
}

struct ParentalPasscode {
    title: &'static str,
    code: Option<&'static str>,
}

const PARENTAL_PASSCODES: &[ParentalPasscode] = &[
    ParentalPasscode {
        title: "Voyeur",
        code: Some("3333"),
    },
    ParentalPasscode {
        title: "Vegas Girls",
        code: Some("1234"),
    },
    ParentalPasscode {
        title: "Loving for a Lifetime",
        code: Some("6969"),
    },
    // Keep the title in the registry so its code can be added without having
    // to rediscover where parental notices are maintained.
    ParentalPasscode {
        title: "Pleasures of Sex, The",
        code: None,
    },
];

/// Return a known parental passcode from the common dump-style disc name.
/// Unknown and explicitly pending codes stay out of the UI.
fn parental_passcode(disc_name: &str) -> Option<&'static str> {
    let stem = disc_name
        .strip_suffix(".cue")
        .or_else(|| disc_name.strip_suffix(".CUE"))
        .unwrap_or(disc_name);
    let title = stem.split_once(" (").map_or(stem, |(title, _)| title);
    PARENTAL_PASSCODES
        .iter()
        .find(|entry| entry.title.eq_ignore_ascii_case(title.trim()))
        .and_then(|entry| entry.code)
}

impl SystemRom {
    fn display(&self) -> String {
        format!("{} ({})", self.label, self.region)
    }

    /// Menu text, flagging boards that cannot boot yet.
    fn menu_text(&self) -> String {
        if self.emulated {
            self.display()
        } else {
            format!("{} — board not emulated", self.display())
        }
    }
}
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
    /// Start with blank player storage and do not save it (diagnostics).
    #[arg(long, hide = true)]
    ephemeral_nvram: bool,
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
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Prefs {
    /// Disc library folders, one per [`LIBRARY_SLOTS`] entry. Stored as a
    /// growable list so adding categories doesn't invalidate saved prefs.
    libraries: Vec<Option<String>>,
    /// Folder screenshots/photos are saved to; `None` means Downloads.
    save_dir: Option<String>,
    show_fps: bool,
    smooth_scaling: bool,
    #[serde(default = "default_true")]
    crt_aspect: bool,
    display_area: DisplayArea,
    capture_mouse_enabled: bool,
    #[serde(default = "default_true")]
    auto_region: bool,
    /// User choices keyed by exact ordered track SHA-1 fingerprint. Path and
    /// filename changes therefore do not lose a disc's settings.
    disc_overrides: BTreeMap<String, DiscOverride>,
    #[serde(default = "default_ff_key")]
    ff_key: egui::Key,
    pad_speed: f32,
    pad_deadzone: f32,
    #[serde(default = "default_true")]
    pad_analog_enabled: bool,
    pad_button1: gilrs::Button,
    pad_button2: gilrs::Button,
    kb_speed: f32,
    kb_up: egui::Key,
    kb_down: egui::Key,
    kb_left: egui::Key,
    kb_right: egui::Key,
    kb_button1: egui::Key,
    kb_button2: egui::Key,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            libraries: Vec::new(),
            save_dir: None,
            show_fps: true,
            smooth_scaling: false,
            crt_aspect: true,
            display_area: DisplayArea::TypicalCrt,
            capture_mouse_enabled: true,
            auto_region: true,
            disc_overrides: BTreeMap::new(),
            ff_key: default_ff_key(),
            pad_speed: 8.0,
            pad_deadzone: 0.15,
            pad_analog_enabled: true,
            pad_button1: gilrs::Button::South,
            pad_button2: gilrs::Button::East,
            kb_speed: 6.0,
            kb_up: egui::Key::ArrowUp,
            kb_down: egui::Key::ArrowDown,
            kb_left: egui::Key::ArrowLeft,
            kb_right: egui::Key::ArrowRight,
            kb_button1: egui::Key::Z,
            kb_button2: egui::Key::X,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
enum DisplayArea {
    #[default]
    TypicalCrt,
    FullSignal,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(default)]
struct DiscOverride {
    /// A manually selected hardware standard for this exact pressing.
    video_standard: Option<VideoStandardRecommendation>,
}

/// Disc library slots, in UI order. CD-BGM is a CD-i-based background-music
/// format the player handles like an ordinary CD-i disc.
const LIBRARY_SLOTS: [&str; 4] = ["Philips CD-i", "Photo CD", "Video CD", "CD-BGM"];
const LIBRARY_FOCUS_COUNT: usize = LIBRARY_SLOTS.len() + 1;
const LIBRARY_OPEN_FOCUS: usize = LIBRARY_SLOTS.len();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LibraryPadAction {
    PreviousTab,
    NextTab,
    PreviousDisc,
    NextDisc,
    Activate,
    Back,
}

fn library_pad_action(
    button: gilrs::Button,
    primary: gilrs::Button,
    secondary: gilrs::Button,
) -> Option<LibraryPadAction> {
    use gilrs::Button;
    match button {
        Button::LeftTrigger | Button::LeftTrigger2 => Some(LibraryPadAction::PreviousTab),
        Button::RightTrigger | Button::RightTrigger2 => Some(LibraryPadAction::NextTab),
        Button::DPadUp => Some(LibraryPadAction::PreviousDisc),
        Button::DPadDown => Some(LibraryPadAction::NextDisc),
        Button::South => Some(LibraryPadAction::Activate),
        Button::East => Some(LibraryPadAction::Back),
        other if other == primary => Some(LibraryPadAction::Activate),
        other if other == secondary => Some(LibraryPadAction::Back),
        _ => None,
    }
}

fn cycle_library_focus(current: usize, forward: bool) -> usize {
    if forward {
        (current + 1) % LIBRARY_FOCUS_COUNT
    } else {
        (current + LIBRARY_FOCUS_COUNT - 1) % LIBRARY_FOCUS_COUNT
    }
}

/// Normalize either a mapped D-pad button or an unmapped hat Y axis into a
/// vertical Library direction. Gilrs reports up as positive Y.
fn library_dpad_direction(axis_y: f32, up_button: bool, down_button: bool) -> i8 {
    let up = up_button || axis_y > 0.5;
    let down = down_button || axis_y < -0.5;
    match (up, down) {
        (true, false) => -1,
        (false, true) => 1,
        _ => 0,
    }
}

/// One disc found while scanning the configured library folders.
struct LibraryEntry {
    title: String,
    /// Index into [`LIBRARY_SLOTS`] naming the category it was found under.
    category: usize,
    cue: PathBuf,
}

/// Settings window tabs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    System,
    Input,
    Libraries,
}

/// The keyboard binding slots, in UI order.
const KB_SLOTS: [&str; 6] = [
    "Pointer up",
    "Pointer down",
    "Pointer left",
    "Pointer right",
    "CD-i button 1",
    "CD-i button 2",
];

const PREFS_KEY: &str = "prefs";

fn default_ff_key() -> egui::Key {
    egui::Key::Tab
}

fn default_true() -> bool {
    true
}

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

/// Select one controller movement source for this poll. A held D-pad takes
/// priority over the left stick so stick drift cannot weaken, cancel, or skew
/// digital movement. The D-pad vector retains both axes for diagonals.
fn controller_deflection(
    stick: egui::Vec2,
    dpad: egui::Vec2,
    dpad_active: bool,
    analog_enabled: bool,
) -> egui::Vec2 {
    let selected = if dpad_active {
        dpad
    } else if analog_enabled {
        stick
    } else {
        egui::Vec2::ZERO
    };
    selected.clamp(egui::vec2(-1.0, -1.0), egui::vec2(1.0, 1.0))
}

/// Apply a continuous radial deadzone to a stick. Values just beyond the
/// deadzone start near zero instead of jumping directly to the raw input.
fn controller_stick_deflection(stick: egui::Vec2, deadzone: f32) -> egui::Vec2 {
    let magnitude = stick.length();
    let deadzone = deadzone.clamp(0.0, 0.99);
    if magnitude <= deadzone || magnitude == 0.0 {
        return egui::Vec2::ZERO;
    }

    let scaled = (magnitude.min(1.0) - deadzone) / (1.0 - deadzone);
    stick / magnitude * scaled
}

/// Merge a controller's D-pad buttons and hat axes into one screen-space
/// direction. Some macOS controller mappings (including 8BitDo modes) expose
/// the D-pad only as `DPadX`/`DPadY`, while others expose buttons. Buttons
/// take precedence on an axis so two opposing held directions remain neutral.
fn controller_dpad_deflection(
    axis_x: f32,
    axis_y: f32,
    left: bool,
    right: bool,
    up: bool,
    down: bool,
) -> (egui::Vec2, bool) {
    const AXIS_THRESHOLD: f32 = 0.5;

    let horizontal_buttons_active = left || right;
    let vertical_buttons_active = up || down;
    let horizontal_axis_active = axis_x.abs() > AXIS_THRESHOLD;
    let vertical_axis_active = axis_y.abs() > AXIS_THRESHOLD;

    let x = if horizontal_buttons_active {
        i32::from(right) as f32 - i32::from(left) as f32
    } else if horizontal_axis_active {
        axis_x.signum()
    } else {
        0.0
    };
    let y = if vertical_buttons_active {
        i32::from(down) as f32 - i32::from(up) as f32
    } else if vertical_axis_active {
        // Gilrs DPadY grows upward; screen Y grows downward.
        -axis_y.signum()
    } else {
        0.0
    };

    (
        egui::vec2(x, y),
        horizontal_buttons_active
            || vertical_buttons_active
            || horizontal_axis_active
            || vertical_axis_active,
    )
}

/// Commands for the Photo CD worker thread.
enum PcdCmd {
    Open {
        path: PathBuf,
        archive_guard: Option<Arc<tempfile::TempDir>>,
    },
    Decode {
        index: usize,
        tier: usize,
    },
    Close,
}

/// Events from the Photo CD worker thread.
enum PcdEvent {
    Opened {
        names: Vec<String>,
        max_tier: usize,
        /// The disc carries a CD-i application (root `CDI` directory, per the
        /// CD Bridge layout in the Photo CD System Description III.2.2). When
        /// true the emulated player drives photo display natively and the
        /// host-side viewer stays out of the way.
        has_cdi_app: bool,
    },
    NotPhotoCd,
    Decoded {
        index: usize,
        image: cdi_photocd::decode::DecodedImage,
    },
    DecodeError(String),
}

/// UI-side state for a detected Photo CD.
struct PhotoCdUi {
    names: Vec<String>,
    current: usize,
    tier: usize,
    max_tier: usize,
    /// The disc carries its own CD-i application; the toggle between CD-i and
    /// the high-fidelity host viewer is offered. When false the viewer is the
    /// only mode and the panel shows a notice instead of the toggle.
    has_cdi_app: bool,
    view_photo: bool,
    slideshow: bool,
    last_advance: Instant,
    decoding: bool,
    decoded: Option<cdi_photocd::decode::DecodedImage>,
    texture: Option<egui::TextureHandle>,
    error: Option<String>,
}

/// Photo CD worker: owns the opened disc (its own file handles, independent
/// of the emulation core) and performs detection and image-pack decoding off
/// the UI thread.
fn photocd_worker(rx: mpsc::Receiver<PcdCmd>, tx: mpsc::Sender<PcdEvent>, ctx: egui::Context) {
    let mut disc: Option<cdi_photocd::disc::OpenedDisc> = None;
    let mut archive_guard: Option<Arc<tempfile::TempDir>> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            PcdCmd::Open {
                path,
                archive_guard: next_archive_guard,
            } => {
                disc = None;
                archive_guard = next_archive_guard;
                match cdi_photocd::disc::open_disc(&path) {
                    Ok(mut opened) if !opened.images.is_empty() => {
                        let names = opened.images.iter().map(|i| i.name.clone()).collect();
                        let max_tier = cdi_photocd::decode::image_max_tier(&mut opened, 0);
                        let has_cdi_app = cdi_photocd::iso9660::find_entry(
                            &mut *opened.reader,
                            opened.pvd.root_lba,
                            opened.pvd.root_size,
                            "CDI",
                        )
                        .ok()
                        .flatten()
                        .map(|entry| entry.is_dir)
                        .unwrap_or(false);
                        disc = Some(opened);
                        let _ = tx.send(PcdEvent::Opened {
                            names,
                            max_tier,
                            has_cdi_app,
                        });
                    }
                    _ => {
                        archive_guard = None;
                        let _ = tx.send(PcdEvent::NotPhotoCd);
                    }
                }
            }
            PcdCmd::Decode { index, tier } => {
                if let Some(opened) = disc.as_mut() {
                    match cdi_photocd::decode::decode_image(opened, index, tier) {
                        Ok(image) => {
                            let _ = tx.send(PcdEvent::Decoded { index, image });
                        }
                        Err(error) => {
                            let _ = tx.send(PcdEvent::DecodeError(error));
                        }
                    }
                }
            }
            PcdCmd::Close => {
                disc = None;
                archive_guard = None;
            }
        }
        // The binding is intentionally retained alongside the opened disc.
        let _ = &archive_guard;
        ctx.request_repaint();
    }
}

/// Write a decoded photo as PNG via a save dialog.
/// Per-user data directory for files the app owns, such as saved NVRAM.
fn app_data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dir = if cfg!(target_os = "macos") {
        home?.join("Library/Application Support/cdi-frontend")
    } else if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var_os("APPDATA")?).join("cdi-frontend")
    } else {
        match std::env::var_os("XDG_DATA_HOME") {
            Some(base) => PathBuf::from(base).join("cdi-frontend"),
            None => home?.join(".local/share/cdi-frontend"),
        }
    };
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn configured_nvram_path(
    board_name: &str,
    pal: bool,
    dvc_inserted: bool,
    persistent: bool,
) -> Option<PathBuf> {
    if !persistent {
        return None;
    }
    let standard = if pal { "pal" } else { "ntsc" };
    let cartridge = if dvc_inserted { "vmpeg" } else { "base" };
    app_data_dir().map(|dir| dir.join(format!("{board_name}-{standard}-{cartridge}.nvr")))
}

fn load_nvram(path: Option<&std::path::Path>, expected_len: usize) -> Vec<u8> {
    let Some(path) = path else {
        return vec![0; expected_len];
    };
    match std::fs::read(path) {
        Ok(saved) if saved.len() == expected_len => {
            log::info!("nvram restored from {}", path.display());
            saved
        }
        Ok(saved) => {
            log::warn!(
                "nvram {}: expected {} bytes, found {}; ignoring",
                path.display(),
                expected_len,
                saved.len()
            );
            vec![0; expected_len]
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => vec![0; expected_len],
        Err(error) => {
            log::warn!("nvram {}: {error}", path.display());
            vec![0; expected_len]
        }
    }
}

/// The platform Downloads folder, used as the default screenshot location.
fn downloads_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join("Downloads")
}

fn save_rgb_png(
    dialog_title: &str,
    suggested_name: &str,
    save_dir: Option<&str>,
    image: &cdi_photocd::decode::DecodedImage,
) -> Option<Result<PathBuf, String>> {
    // Start in the configured save folder, else Downloads, else the first
    // existing of the two.
    let start_dir = save_dir
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(downloads_dir);
    let mut dialog = rfd::FileDialog::new()
        .set_title(dialog_title)
        .set_file_name(suggested_name);
    if start_dir.is_dir() {
        dialog = dialog.set_directory(start_dir);
    }
    let path = dialog.save_file()?;
    let file = match std::fs::File::create(&path) {
        Ok(file) => file,
        Err(error) => return Some(Err(format!("{}: {error}", path.display()))),
    };
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let result = encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&image.rgb))
        .map(|()| path)
        .map_err(|error| error.to_string());
    Some(result)
}

struct SharedFrame {
    pixels: Vec<u32>,
    width: usize,
    height: usize,
    /// Hardware-derived timing aperture for this exact frame.
    geometry: DisplayGeometry,
    frame_no: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DisplayAperture {
    left: usize,
    top: usize,
    width: usize,
    height: usize,
}

fn display_aperture(frame: &SharedFrame, display_area: DisplayArea) -> DisplayAperture {
    let hardware = DisplayAperture {
        left: frame.geometry.active_x,
        top: frame.geometry.active_y,
        width: frame.geometry.active_width,
        height: frame.geometry.active_height,
    };
    if display_area == DisplayArea::TypicalCrt
        && frame.geometry.raster_height == 480
        && hardware
            == (DisplayAperture {
                left: 0,
                top: 0,
                width: 768,
                height: 480,
            })
    {
        // Philips TN 093 gives the 525-line player pixel aspect as 1.225.
        // The 360x220 normal-resolution television viewing area therefore
        // presents at 1.336:1, effectively 4:3, while the surrounding
        // 384x240 signal remains available through Full signal. Four-sided
        // windowboxed material stays centered. A picture which reaches the
        // bottom overscan edge uses the title/game convention that places
        // the same 220-line area against the bottom of the signal.
        let mut bottom_picture_pixels = 0usize;
        for y in 460..480 {
            bottom_picture_pixels += frame.pixels[y * frame.width + 24..y * frame.width + 744]
                .iter()
                .filter(|pixel| {
                    let pixel = **pixel;
                    ((pixel >> 16) & 0xFF) > 20 || ((pixel >> 8) & 0xFF) > 20 || (pixel & 0xFF) > 20
                })
                .count();
        }
        let bottom_edge_picture = bottom_picture_pixels >= 3_600;
        return DisplayAperture {
            left: 24,
            top: if bottom_edge_picture { 40 } else { 20 },
            width: 720,
            height: 440,
        };
    }
    hardware
}

fn pointer_mapping(
    aperture: DisplayAperture,
    geometry: DisplayGeometry,
) -> (egui::Pos2, egui::Vec2) {
    let origin = egui::pos2(
        (aperture.left - geometry.active_x) as f32 * 768.0 / geometry.active_width as f32,
        (aperture.top - geometry.active_y) as f32 * 560.0 / geometry.active_height as f32,
    );
    let extent = egui::vec2(
        aperture.width as f32 * 768.0 / geometry.active_width as f32,
        aperture.height as f32 * 560.0 / geometry.active_height as f32,
    );
    (origin, extent)
}

fn screenshot_dimensions(
    aperture: DisplayAperture,
    geometry: DisplayGeometry,
    crt_aspect: bool,
) -> (usize, usize) {
    if crt_aspect {
        let aspect = presentation_aspect(aperture, geometry, true);
        (
            aperture.width,
            (aperture.width as f32 / aspect).round() as usize,
        )
    } else {
        (aperture.width, aperture.height)
    }
}

/// Capture the same hardware/presentation aperture and television-pixel
/// correction shown by the player, without including host-window chrome.
/// Nearest-neighbor row selection keeps source pixels crisp while giving the
/// PNG square display pixels.
fn screenshot_image(
    frame: &SharedFrame,
    display_area: DisplayArea,
    crt_aspect: bool,
) -> cdi_photocd::decode::DecodedImage {
    let aperture = display_aperture(frame, display_area);
    let (width, height) = screenshot_dimensions(aperture, frame.geometry, crt_aspect);
    let mut rgb = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        let source_y = aperture.top + y * aperture.height / height;
        for x in 0..width {
            let source_x = aperture.left + x * aperture.width / width;
            let px = presentation_rgb(frame.pixels[source_y * frame.width + source_x]);
            rgb.extend_from_slice(&[(px >> 16) as u8, (px >> 8) as u8, px as u8]);
        }
    }
    cdi_photocd::decode::DecodedImage {
        width: width as u32,
        height: height as u32,
        rgb,
    }
}

struct Shared {
    frame: Mutex<SharedFrame>,
    input: Mutex<InputState>,
    command: Mutex<Option<MachineCommand>>,
    status: Mutex<String>,
    /// Name of the loaded disc, shown in the top bar; `None` when empty.
    disc_name: Mutex<Option<String>>,
    /// Path of the loaded disc, so it can be restored across a machine rebuild.
    disc_path: Mutex<Option<PathBuf>>,
    /// Exact ordered track hashes and an optional compact Redump profile.
    disc_identity: Mutex<Option<DiscIdentity>>,
    /// UI preferences mirrored for the emulation thread so the standard is
    /// selected before the reset which boots a newly inserted disc.
    disc_standard_overrides: Mutex<BTreeMap<String, VideoStandardRecommendation>>,
    /// Human-readable current system ROM, shown in Settings.
    rom_status: Mutex<String>,
    dvc_status: Mutex<String>,
    dvc_path: Mutex<Option<PathBuf>>,
    dvc_inserted: AtomicBool,
    pal: AtomicBool,
    /// Match the player's video standard to the disc's region tag on load.
    auto_region: AtomicBool,
    /// Host-side turbo: run the emulation unthrottled while held.
    fast_forward: AtomicBool,
    muted: Arc<AtomicBool>,
    /// Silence and drain host audio while the Library covers the player,
    /// independently of the user's persistent mute preference.
    audio_suppressed: Arc<AtomicBool>,
    running: AtomicBool,
    /// Emulated frames per second (diagnostics).
    fps: Mutex<f32>,
}

enum MachineCommand {
    LoadDisc {
        path: PathBuf,
        archive_guard: Option<Arc<tempfile::TempDir>>,
    },
    EjectDisc,
    AttachDvc(PathBuf),
    AttachBundledDvc,
    DetachDvc,
    SetVideoStandard(cdi_core::VideoStandard),
    /// Swap the system ROM: rebuilds the machine, restoring disc and DVC.
    SetSystemRom {
        image: Vec<u8>,
        label: String,
    },
    /// Back up and clear the player's battery-backed storage, then reboot.
    ResetNvram,
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
        ephemeral_nvram,
    } = Args::parse();
    let image = if let Some(path) = &rom {
        std::fs::read(path)?
    } else {
        BUNDLED_CDI220B.to_vec()
    };
    let initial_rom_status = SYSTEM_ROMS
        .iter()
        .find(|r| r.bytes == image.as_slice())
        .map(SystemRom::display)
        .or_else(|| rom.as_ref().map(|p| display_name(p)));
    let modules = cdi_os9::scan_modules(&image);
    let rom_type = cdi_os9::identify_rom(&modules);
    let detected_model = cdi_core::boards::model_by_id(rom_type.id)
        .ok_or_else(|| format!("no emulation model for ROM type {}", rom_type.id))?;
    let mut model = detected_model.clone();
    if let Some(standard) = video_standard {
        model.video = standard.into();
    }
    let title = APP_NAME.to_owned();

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

    let (initial_disc, initial_archive_guard) = match disc {
        Some(source) => {
            let (cue, guard) = resolve_disc_source(&source)
                .map_err(|error| format!("{}: {error}", source.display()))?;
            (Some(cue), guard)
        }
        None => (None, None),
    };
    let mut disc_name: Option<String> = None;
    let mut initial_disc_identity = None;
    if let Some(cue) = &initial_disc {
        let disc = cdi_disc::DiscImage::load(cue)?;
        log::info!(
            "disc inserted: {} track(s), lead-out {}",
            disc.tracks().len(),
            disc.leadout_msf()
        );
        machine.set_disc(Some(disc));
        disc_name = Some(display_name(cue));
        match disc_profiles::identify_disc(cue) {
            Ok(identity) => {
                if video_standard.is_none() {
                    if let Some(profile) = &identity.profile {
                        let pal = profile.video_standard.is_pal();
                        machine.set_video_standard(if pal {
                            cdi_core::VideoStandard::Pal
                        } else {
                            cdi_core::VideoStandard::Ntsc
                        });
                        model.video = if pal {
                            cdi_core::VideoStandard::Pal
                        } else {
                            cdi_core::VideoStandard::Ntsc
                        };
                    }
                }
                initial_disc_identity = Some(identity);
            }
            Err(error) => log::warn!("disc identification failed: {error}"),
        }
    }
    // Battery-backed SRAM belongs to a physical player configuration. E-Di
    // can change video standards and add/remove a DVC at runtime, so keeping
    // one file per board would expose titles to a CSD and application state
    // generated for different hardware. Configuration-scoped stores preserve
    // saves normally without requiring destructive resets.
    let nvram_path = configured_nvram_path(
        model.board.name,
        model.video == cdi_core::VideoStandard::Pal,
        dvc_inserted,
        !ephemeral_nvram,
    );
    let saved_nvram = load_nvram(nvram_path.as_deref(), machine.bus.nvram.len());
    machine.bus.nvram.copy_from_slice(&saved_nvram);
    let initial_geometry = machine.bus.mcd212.display_geometry();
    let (fb_w, fb_h) = (
        initial_geometry.raster_width,
        initial_geometry.raster_height,
    );

    let shared = Arc::new(Shared {
        frame: Mutex::new(SharedFrame {
            pixels: vec![0; FB_WIDTH * FB_HEIGHT],
            width: fb_w,
            height: fb_h,
            geometry: initial_geometry,
            frame_no: 0,
        }),
        input: Mutex::new(InputState::default()),
        command: Mutex::new(None),
        status: Mutex::new(String::new()),
        disc_name: Mutex::new(disc_name),
        disc_path: Mutex::new(initial_disc.clone()),
        disc_identity: Mutex::new(initial_disc_identity),
        disc_standard_overrides: Mutex::new(BTreeMap::new()),
        rom_status: Mutex::new(initial_rom_status.unwrap_or_else(|| model.title.to_owned())),
        dvc_status: Mutex::new(dvc_status),
        dvc_path: Mutex::new(dvc_path),
        dvc_inserted: AtomicBool::new(dvc_inserted),
        pal: AtomicBool::new(model.video == cdi_core::VideoStandard::Pal),
        auto_region: AtomicBool::new(true),
        fast_forward: AtomicBool::new(false),
        muted: Arc::new(AtomicBool::new(false)),
        audio_suppressed: Arc::new(AtomicBool::new(false)),
        running: AtomicBool::new(true),
        fps: Mutex::new(0.0),
    });

    let (audio_stream, audio_producer) = match start_audio(
        Arc::clone(&shared.muted),
        Arc::clone(&shared.audio_suppressed),
    ) {
        Ok(pair) => (Some(pair.0), Some(pair.1)),
        Err(error) => {
            log::warn!("audio output disabled: {error}");
            *shared.status.lock().unwrap() = format!("Audio disabled: {error}");
            (None, None)
        }
    };

    let emu_shared = Arc::clone(&shared);
    let emu_thread = std::thread::Builder::new().name("emu".into()).spawn({
        let initial_archive_guard = initial_archive_guard.clone();
        move || {
            emu_loop(
                machine,
                emu_shared,
                audio_producer,
                nvram_path,
                model.board.name.to_owned(),
                initial_archive_guard,
                !ephemeral_nvram,
            )
        }
    })?;

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
                fb_w as f32 / (4.0 / 3.0)
                    + if cfg!(target_os = "macos") {
                        24.0
                    } else {
                        48.0
                    },
            ])
            .with_title(&title),
        vsync: true,
        // Keep preference storage, but start close to the television shape.
        // The first UI frame resizes to the selected display-area and exact
        // Philips player pixel aspect.
        persist_window: false,
        ..Default::default()
    };
    let app_shared = Arc::clone(&shared);
    let result = eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            Ok(Box::new(App::new(
                app_shared,
                cc,
                initial_disc,
                initial_archive_guard,
            )))
        }),
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

fn is_zip_path(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

fn resolve_disc_source(
    path: &std::path::Path,
) -> Result<(PathBuf, Option<Arc<tempfile::TempDir>>), String> {
    if !is_zip_path(path) {
        return Ok((path.to_owned(), None));
    }
    let extracted = store_zip::extract(path)?;
    let guard = Arc::new(extracted.temp_dir);
    Ok((extracted.cue_path, Some(guard)))
}

fn start_audio(
    muted: Arc<AtomicBool>,
    suppressed: Arc<AtomicBool>,
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
                    muted.load(Ordering::Relaxed) || suppressed.load(Ordering::Relaxed),
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
                    muted.load(Ordering::Relaxed) || suppressed.load(Ordering::Relaxed),
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
                    muted.load(Ordering::Relaxed) || suppressed.load(Ordering::Relaxed),
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

fn switch_nvram_store(
    machine: &mut cdi_core::Machine,
    nvram_path: &mut Option<PathBuf>,
    nvram_mirror: &mut Vec<u8>,
    new_path: Option<PathBuf>,
) {
    if *nvram_path == new_path {
        return;
    }
    if machine.bus.nvram != *nvram_mirror {
        if let Err(error) = write_nvram(nvram_path.as_deref(), &machine.bus.nvram) {
            log::warn!("NVRAM flush before hardware change: {error}");
        }
    }
    let saved = load_nvram(new_path.as_deref(), machine.bus.nvram.len());
    machine.bus.nvram.copy_from_slice(&saved);
    nvram_mirror.clone_from(&saved);
    *nvram_path = new_path;
}

fn emu_loop(
    mut machine: cdi_core::Machine,
    shared: Arc<Shared>,
    mut audio: Option<Producer<i16>>,
    mut nvram_path: Option<PathBuf>,
    mut nvram_board_name: String,
    mut disc_archive_guard: Option<Arc<tempfile::TempDir>>,
    persistent_nvram: bool,
) {
    let mut next_frame_deadline = Instant::now();
    let mut fps_window_start = Instant::now();
    let mut fps_frames = 0u32;
    // Mirror of the last NVRAM contents written, so saves only hit disk when
    // the title actually changed something.
    let mut nvram_mirror = machine.bus.nvram.clone();
    let mut last_nvram_flush = Instant::now();
    let trace_dvc_composition = std::env::var_os("EDI_DVC_COMPOSITION_TRACE").is_some();
    let mut traced_dvc_play_events = 0;
    let mut traced_dvc_visible = 0;
    let mut next_dvc_frame_trace = 30;
    let mut awaiting_first_dvc_frame = false;

    while shared.running.load(Ordering::Relaxed) {
        if let Some(command) = shared.command.lock().unwrap().take() {
            match command {
                MachineCommand::LoadDisc {
                    path,
                    archive_guard,
                } => match cdi_disc::DiscImage::load(&path) {
                    Ok(disc) => {
                        // Identify the exact pressing before inserting it.
                        // Hashing is intentionally done on the emulation
                        // thread so the GUI remains responsive for large BINs.
                        let identity = match disc_profiles::identify_disc(&path) {
                            Ok(identity) => Some(identity),
                            Err(error) => {
                                log::warn!("disc identification failed: {error}");
                                None
                            }
                        };

                        // Exact user choice, then exact tracked profile, then
                        // filename region are progressively weaker evidence.
                        if shared.auto_region.load(Ordering::Relaxed) {
                            let remembered = identity.as_ref().and_then(|identity| {
                                shared
                                    .disc_standard_overrides
                                    .lock()
                                    .unwrap()
                                    .get(&identity.fingerprint)
                                    .copied()
                            });
                            let profiled = identity
                                .as_ref()
                                .and_then(|identity| identity.profile.as_ref())
                                .map(|profile| profile.video_standard);
                            let pal = remembered
                                .or(profiled)
                                .map(VideoStandardRecommendation::is_pal)
                                .or_else(|| region_is_pal(&display_name(&path)));
                            if let Some(pal) = pal {
                                if pal != shared.pal.load(Ordering::Relaxed) {
                                    let new_path = configured_nvram_path(
                                        &nvram_board_name,
                                        pal,
                                        shared.dvc_inserted.load(Ordering::Relaxed),
                                        persistent_nvram,
                                    );
                                    switch_nvram_store(
                                        &mut machine,
                                        &mut nvram_path,
                                        &mut nvram_mirror,
                                        new_path,
                                    );
                                    machine.set_video_standard(if pal {
                                        cdi_core::VideoStandard::Pal
                                    } else {
                                        cdi_core::VideoStandard::Ntsc
                                    });
                                    shared.pal.store(pal, Ordering::Relaxed);
                                }
                            }
                        }
                        // A live drive reports its SERVO status through the
                        // SLAVE X-Bus channel. Do not throw away the player
                        // shell boot merely because the user inserted media.
                        machine.change_disc(Some(disc));
                        *shared.disc_name.lock().unwrap() = Some(display_name(&path));
                        *shared.disc_path.lock().unwrap() = Some(path);
                        *shared.disc_identity.lock().unwrap() = identity;
                        disc_archive_guard = archive_guard;
                        shared.status.lock().unwrap().clear();
                    }
                    Err(error) => {
                        *shared.disc_name.lock().unwrap() = None;
                        *shared.disc_path.lock().unwrap() = None;
                        *shared.disc_identity.lock().unwrap() = None;
                        *shared.status.lock().unwrap() = format!("Open failed: {error}");
                    }
                },
                MachineCommand::EjectDisc => {
                    machine.set_disc(None);
                    disc_archive_guard = None;
                    machine.reset();
                    *shared.disc_name.lock().unwrap() = None;
                    *shared.disc_path.lock().unwrap() = None;
                    *shared.disc_identity.lock().unwrap() = None;
                    shared.status.lock().unwrap().clear();
                }
                MachineCommand::SetSystemRom { image, label } => {
                    let modules = cdi_os9::scan_modules(&image);
                    let rom_type = cdi_os9::identify_rom(&modules);
                    match cdi_core::boards::model_by_id(rom_type.id) {
                        Some(detected) => {
                            let mut model = detected.clone();
                            model.video = if shared.pal.load(Ordering::Relaxed) {
                                cdi_core::VideoStandard::Pal
                            } else {
                                cdi_core::VideoStandard::Ntsc
                            };
                            // Rebuild with the DVC that is currently inserted.
                            let dvc = if shared.dvc_inserted.load(Ordering::Relaxed) {
                                let path = shared.dvc_path.lock().unwrap().clone();
                                let firmware = match &path {
                                    Some(p) => std::fs::read(p).ok(),
                                    None => Some(BUNDLED_VMPEGA.to_vec()),
                                };
                                firmware.and_then(|f| cdi_core::DvcConfig::from_rom(f).ok())
                            } else {
                                None
                            };
                            match cdi_core::Machine::with_dvc(&model, image, dvc) {
                                Ok(mut rebuilt) => {
                                    let rebuilt_nvram_path = configured_nvram_path(
                                        model.board.name,
                                        model.video == cdi_core::VideoStandard::Pal,
                                        shared.dvc_inserted.load(Ordering::Relaxed),
                                        persistent_nvram,
                                    );
                                    if rebuilt_nvram_path == nvram_path {
                                        // Keep changes newer than the last periodic
                                        // flush when the hardware configuration did
                                        // not change.
                                        rebuilt.bus.nvram.copy_from_slice(&machine.bus.nvram);
                                    } else {
                                        if machine.bus.nvram != nvram_mirror {
                                            if let Err(error) = write_nvram(
                                                nvram_path.as_deref(),
                                                &machine.bus.nvram,
                                            ) {
                                                log::warn!(
                                                    "nvram flush before board change: {error}"
                                                );
                                            }
                                        }
                                        let saved = load_nvram(
                                            rebuilt_nvram_path.as_deref(),
                                            rebuilt.bus.nvram.len(),
                                        );
                                        rebuilt.bus.nvram.copy_from_slice(&saved);
                                        nvram_path = rebuilt_nvram_path;
                                    }
                                    machine = rebuilt;
                                    nvram_board_name = model.board.name.to_owned();
                                    nvram_mirror.clone_from(&machine.bus.nvram);
                                    // Restore the disc across the rebuild.
                                    let disc_path = shared.disc_path.lock().unwrap().clone();
                                    if let Some(path) = disc_path {
                                        if let Ok(disc) = cdi_disc::DiscImage::load(&path) {
                                            machine.set_disc(Some(disc));
                                        }
                                    }
                                    machine.reset();
                                    *shared.rom_status.lock().unwrap() = label;
                                    shared.status.lock().unwrap().clear();
                                }
                                Err(error) => {
                                    *shared.status.lock().unwrap() = format!("{label}: {error}");
                                }
                            }
                        }
                        None => {
                            *shared.status.lock().unwrap() = format!(
                                "{label}: board not emulated yet (ROM type {})",
                                rom_type.id
                            );
                        }
                    }
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
                            let new_path = configured_nvram_path(
                                &nvram_board_name,
                                shared.pal.load(Ordering::Relaxed),
                                true,
                                persistent_nvram,
                            );
                            switch_nvram_store(
                                &mut machine,
                                &mut nvram_path,
                                &mut nvram_mirror,
                                new_path,
                            );
                            machine.reset();
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
                            let new_path = configured_nvram_path(
                                &nvram_board_name,
                                shared.pal.load(Ordering::Relaxed),
                                true,
                                persistent_nvram,
                            );
                            switch_nvram_store(
                                &mut machine,
                                &mut nvram_path,
                                &mut nvram_mirror,
                                new_path,
                            );
                            machine.reset();
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
                    let new_path = configured_nvram_path(
                        &nvram_board_name,
                        shared.pal.load(Ordering::Relaxed),
                        false,
                        persistent_nvram,
                    );
                    switch_nvram_store(&mut machine, &mut nvram_path, &mut nvram_mirror, new_path);
                    machine.reset();
                    shared.dvc_inserted.store(false, Ordering::Relaxed);
                    *shared.dvc_status.lock().unwrap() = "DVC removed".to_owned();
                }
                MachineCommand::SetVideoStandard(standard) => {
                    let pal = standard == cdi_core::VideoStandard::Pal;
                    let new_path = configured_nvram_path(
                        &nvram_board_name,
                        pal,
                        shared.dvc_inserted.load(Ordering::Relaxed),
                        persistent_nvram,
                    );
                    switch_nvram_store(&mut machine, &mut nvram_path, &mut nvram_mirror, new_path);
                    machine.set_video_standard(standard);
                    shared.pal.store(pal, Ordering::Relaxed);
                    *shared.status.lock().unwrap() = format!(
                        "Machine reset — {}",
                        if pal { "PAL (50 Hz)" } else { "NTSC (60 Hz)" }
                    );
                }
                MachineCommand::ResetNvram => {
                    match backup_nvram(nvram_path.as_deref(), &machine.bus.nvram) {
                        Ok(backup) => {
                            machine.bus.nvram.fill(0);
                            nvram_mirror.fill(0);
                            machine.reset();
                            match write_nvram(nvram_path.as_deref(), &machine.bus.nvram) {
                                Ok(()) => {
                                    *shared.status.lock().unwrap() = backup.map_or_else(
                                        || "Player storage reset".to_owned(),
                                        |path| {
                                            format!(
                                                "Player storage reset — backup: {}",
                                                path.display()
                                            )
                                        },
                                    );
                                }
                                Err(error) => {
                                    *shared.status.lock().unwrap() =
                                        format!("Player storage reset, but save failed: {error}");
                                }
                            }
                        }
                        Err(error) => {
                            *shared.status.lock().unwrap() =
                                format!("Player storage backup failed; reset cancelled: {error}");
                        }
                    }
                }
                MachineCommand::Reset => {
                    machine.reset();
                    *shared.status.lock().unwrap() = "Machine reset".to_owned();
                }
            }
        }

        // Keep extracted Store ZIP contents alive for the machine's open
        // sector files and for system-ROM rebuilds which reopen the CUE.
        let _ = &disc_archive_guard;

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

        if trace_dvc_composition {
            let snapshot = machine.diagnostic_snapshot();
            if let Some(dvc) = snapshot.dvc {
                let (raster_min, raster_max) = machine
                    .bus
                    .mcd212
                    .framebuffer()
                    .iter()
                    .map(|pixel| pixel & 0x00FF_FFFF)
                    .fold((u32::MAX, 0), |(min, max), pixel| {
                        (min.min(pixel), max.max(pixel))
                    });
                if dvc.play_events != traced_dvc_play_events {
                    traced_dvc_play_events = dvc.play_events;
                    awaiting_first_dvc_frame = true;
                    next_dvc_frame_trace = dvc.presented_video_frames.saturating_add(30);
                    log_dvc_composition("play", &snapshot.mcd212, &dvc, raster_min, raster_max);
                }
                if dvc.video_visible != traced_dvc_visible {
                    traced_dvc_visible = dvc.video_visible;
                    log_dvc_composition(
                        "visibility",
                        &snapshot.mcd212,
                        &dvc,
                        raster_min,
                        raster_max,
                    );
                }
                if awaiting_first_dvc_frame && dvc.current_video_frame_hash != 0 {
                    awaiting_first_dvc_frame = false;
                    log_dvc_composition(
                        "first-frame",
                        &snapshot.mcd212,
                        &dvc,
                        raster_min,
                        raster_max,
                    );
                }
                if dvc.presented_video_frames >= next_dvc_frame_trace {
                    log_dvc_composition(
                        "frame-sample",
                        &snapshot.mcd212,
                        &dvc,
                        raster_min,
                        raster_max,
                    );
                    next_dvc_frame_trace = dvc.presented_video_frames.saturating_add(30);
                }
            }
        }

        let fast_forward = shared.fast_forward.load(Ordering::Relaxed);
        let samples = machine.take_audio();
        // While fast-forwarding, samples are produced far faster than the
        // 44.1 kHz device drains them; dropping them here keeps the output
        // silent instead of a stutter, and costs nothing when idle.
        if let (Some(producer), false) = (&mut audio, fast_forward) {
            for sample in samples {
                if producer.push(sample).is_err() {
                    break;
                }
            }
        }

        // Publish the frame.
        {
            let mut frame = shared.frame.lock().unwrap();
            let geometry = machine.bus.mcd212.display_geometry();
            let (w, h) = (geometry.raster_width, geometry.raster_height);
            frame.width = w;
            frame.height = h;
            frame.geometry = geometry;
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
        if fast_forward {
            // Run flat out and keep the deadline anchored to now, so releasing
            // the key does not try to "catch up" the skipped time.
            next_frame_deadline = Instant::now();
        } else {
            next_frame_deadline += frame_duration;
            let now = Instant::now();
            if next_frame_deadline > now {
                std::thread::sleep(next_frame_deadline - now);
            } else {
                next_frame_deadline = now;
            }
        }

        // Flush saves periodically so a crash or force-quit does not lose
        // them; comparing 8 KB a few times a minute is free next to a frame.
        if last_nvram_flush.elapsed() >= Duration::from_secs(5) {
            last_nvram_flush = Instant::now();
            if machine.bus.nvram != nvram_mirror {
                nvram_mirror.copy_from_slice(&machine.bus.nvram);
                if let Err(error) = write_nvram(nvram_path.as_deref(), &nvram_mirror) {
                    log::warn!("nvram save: {error}");
                }
            }
        }
    }

    if machine.bus.nvram != nvram_mirror {
        if let Err(error) = write_nvram(nvram_path.as_deref(), &machine.bus.nvram) {
            log::warn!("final nvram save: {error}");
        }
    }
}

fn log_dvc_composition(
    event: &str,
    mcd212: &cdi_core::diagnostics::Mcd212DiagnosticSnapshot,
    dvc: &cdi_core::DvcStats,
    raster_min: u32,
    raster_max: u32,
) {
    log::info!(
        "DVC composition {event}: play={} visible={} presented={} frame={}x{} rgb={}..{} \
         hash={:016x} display=({}, {}) window=({}, {}) {}x{} ICM={:08x} TCR={:08x} \
         POR={:08x} raster={}x{} rgb={raster_min}..{raster_max} active=({}, {}) {}x{}",
        dvc.play_events,
        dvc.video_visible,
        dvc.presented_video_frames,
        dvc.current_video_width,
        dvc.current_video_height,
        dvc.current_video_rgb_min,
        dvc.current_video_rgb_max,
        dvc.current_video_frame_hash,
        dvc.video_display_x,
        dvc.video_display_y,
        dvc.video_window_x,
        dvc.video_window_y,
        dvc.video_window_width,
        dvc.video_window_height,
        mcd212.image_coding_method,
        mcd212.transparency_control,
        mcd212.plane_order,
        mcd212.geometry.raster_width,
        mcd212.geometry.raster_height,
        mcd212.geometry.active_x,
        mcd212.geometry.active_y,
        mcd212.geometry.active_width,
        mcd212.geometry.active_height,
    );
}

/// Write battery-backed SRAM atomically, replacing any previous contents.
fn write_nvram(path: Option<&std::path::Path>, data: &[u8]) -> Result<(), String> {
    let Some(path) = path else {
        return Ok(());
    };
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        format!(
            "create temporary NVRAM file in {}: {error}",
            parent.display()
        )
    })?;
    temporary
        .write_all(data)
        .map_err(|error| format!("write temporary NVRAM file: {error}"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| format!("sync temporary NVRAM file: {error}"))?;
    temporary
        .persist(path)
        .map_err(|error| format!("replace {}: {}", path.display(), error.error))?;
    log::debug!("nvram saved to {}", path.display());
    Ok(())
}

fn backup_nvram(path: Option<&std::path::Path>, data: &[u8]) -> Result<Option<PathBuf>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    if data.iter().all(|&byte| byte == 0) {
        return Ok(None);
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("system clock: {error}"))?
        .as_secs();
    let backup = path.with_extension(format!("nvr.backup-{timestamp}"));
    std::fs::write(&backup, data)
        .map_err(|error| format!("write backup {}: {error}", backup.display()))?;
    Ok(Some(backup))
}

struct App {
    shared: Arc<Shared>,
    texture: Option<egui::TextureHandle>,
    last_frame_no: u64,
    settings_open: bool,
    reset_nvram_confirmation: bool,
    show_fps: bool,
    smooth_scaling: bool,
    crt_aspect: bool,
    display_area: DisplayArea,
    disc_overrides: BTreeMap<String, DiscOverride>,
    active_fingerprint: Option<String>,
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
    pad_analog_enabled: bool,
    pad_button1: gilrs::Button,
    pad_button2: gilrs::Button,
    /// CD-i button (0 or 1) awaiting a controller press to rebind.
    pad_rebind: Option<u8>,
    kb_speed: f32,
    kb_keys: [egui::Key; 6],
    kb_buttons: u8,
    /// Sub-pixel remainder of keyboard pointer motion.
    kb_frac: egui::Vec2,
    /// Binding slot (index into KB_SLOTS) awaiting a key press to rebind.
    kb_rebind: Option<usize>,
    /// Hold-to-fast-forward key, and whether it is awaiting a rebind press.
    ff_key: egui::Key,
    ff_rebind: bool,
    /// Sub-pixel remainder of captured-mouse motion carried across frames.
    capture_frac: egui::Vec2,
    capture_origin: Option<egui::Pos2>,
    capture_motion_grace: u8,
    pcd_tx: mpsc::Sender<PcdCmd>,
    pcd_rx: mpsc::Receiver<PcdEvent>,
    photocd: Option<PhotoCdUi>,
    libraries: [Option<String>; LIBRARY_SLOTS.len()],
    library: Vec<LibraryEntry>,
    show_library: bool,
    /// Selected format tab in the library view (index into [`LIBRARY_SLOTS`]).
    library_tab: usize,
    /// Controller focus across the four format tabs plus Open .cue.
    library_focus: usize,
    /// Controller-selected row within the active format.
    library_selection: usize,
    /// Last mapped D-pad/hat direction, for edge-triggered row navigation.
    library_dpad_y: i8,
    /// Host-side controller menu shown over the current title.
    quick_menu_open: bool,
    /// Selected quick-menu row: Library or Return to Current Title.
    quick_menu_selection: usize,
    /// Last D-pad/hat direction, for edge-triggered quick-menu navigation.
    quick_menu_dpad_y: i8,
    /// Measured width of the library tab strip, used to center it next frame.
    library_strip_w: f32,
    /// Current window title, so it is only pushed when the disc changes.
    window_title: String,
    save_dir: Option<String>,
    settings_tab: SettingsTab,
    #[cfg(target_os = "macos")]
    native_menu: NativeMenu,
}

impl App {
    fn new(
        shared: Arc<Shared>,
        cc: &eframe::CreationContext<'_>,
        initial_disc: Option<PathBuf>,
        initial_archive_guard: Option<Arc<tempfile::TempDir>>,
    ) -> Self {
        configure_ui(&cc.egui_ctx);
        let (pcd_tx, cmd_rx) = mpsc::channel();
        let (event_tx, pcd_rx) = mpsc::channel();
        let worker_ctx = cc.egui_ctx.clone();
        std::thread::Builder::new()
            .name("photocd".into())
            .spawn(move || photocd_worker(cmd_rx, event_tx, worker_ctx))
            .expect("spawn photocd worker");
        let has_initial_disc = initial_disc.is_some();
        if let Some(path) = initial_disc {
            let _ = pcd_tx.send(PcdCmd::Open {
                path,
                archive_guard: initial_archive_guard,
            });
        }
        let prefs: Prefs = cc
            .storage
            .and_then(|storage| storage.get_string(PREFS_KEY))
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        let mut disc_overrides = prefs.disc_overrides.clone();
        // Older builds persisted host-only crop selections. Serde ignores
        // that obsolete field; discard entries which now contain no
        // hardware-standard override so saving preferences prunes them.
        disc_overrides.retain(|_, settings| settings.video_standard.is_some());
        let mut app = Self {
            shared,
            texture: None,
            last_frame_no: 0,
            settings_open: false,
            reset_nvram_confirmation: false,
            show_fps: prefs.show_fps,
            smooth_scaling: prefs.smooth_scaling,
            crt_aspect: prefs.crt_aspect,
            display_area: prefs.display_area,
            disc_overrides,
            active_fingerprint: None,
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
            pad_analog_enabled: prefs.pad_analog_enabled,
            pad_button1: prefs.pad_button1,
            pad_button2: prefs.pad_button2,
            pad_rebind: None,
            kb_speed: prefs.kb_speed,
            kb_keys: [
                prefs.kb_up,
                prefs.kb_down,
                prefs.kb_left,
                prefs.kb_right,
                prefs.kb_button1,
                prefs.kb_button2,
            ],
            kb_buttons: 0,
            kb_frac: egui::Vec2::ZERO,
            kb_rebind: None,
            ff_key: prefs.ff_key,
            ff_rebind: false,
            capture_frac: egui::Vec2::ZERO,
            capture_origin: None,
            capture_motion_grace: 0,
            pcd_tx,
            pcd_rx,
            photocd: None,
            libraries: {
                // Normalize the stored list to the fixed slot count so an
                // added category can't shift or drop existing folders.
                let mut libs: [Option<String>; LIBRARY_SLOTS.len()] = Default::default();
                for (slot, value) in prefs.libraries.iter().take(LIBRARY_SLOTS.len()).enumerate() {
                    libs[slot] = value.clone();
                }
                libs
            },
            library: Vec::new(),
            // Open into the library when folders are configured and no disc was
            // passed on the command line.
            show_library: !has_initial_disc && prefs.libraries.iter().flatten().next().is_some(),
            library_tab: 0,
            library_focus: 0,
            library_selection: 0,
            library_dpad_y: 0,
            quick_menu_open: false,
            quick_menu_selection: 0,
            quick_menu_dpad_y: 0,
            library_strip_w: 0.0,
            window_title: APP_NAME.to_owned(),
            save_dir: prefs.save_dir.clone(),
            settings_tab: SettingsTab::System,
            #[cfg(target_os = "macos")]
            native_menu: NativeMenu::new().expect("initialize native macOS menu"),
        };
        // Shared is built before prefs are loaded, so apply stored flags now.
        app.shared
            .auto_region
            .store(prefs.auto_region, Ordering::Relaxed);
        *app.shared.disc_standard_overrides.lock().unwrap() = app
            .disc_overrides
            .iter()
            .filter_map(|(fingerprint, settings)| {
                settings
                    .video_standard
                    .map(|standard| (fingerprint.clone(), standard))
            })
            .collect();
        app.sync_disc_identity();
        app.shared
            .audio_suppressed
            .store(app.show_library, Ordering::Relaxed);
        app.scan_libraries();
        app
    }

    /// Track the exact pressing when the emulation thread finishes
    /// identifying a newly inserted disc.
    fn sync_disc_identity(&mut self) {
        let fingerprint = self
            .shared
            .disc_identity
            .lock()
            .unwrap()
            .as_ref()
            .map(|identity| identity.fingerprint.clone());
        if fingerprint == self.active_fingerprint {
            return;
        }
        self.active_fingerprint = fingerprint;
    }

    fn store_disc_override(&mut self, settings: DiscOverride) {
        let Some(fingerprint) = self.active_fingerprint.clone() else {
            return;
        };
        if settings == DiscOverride::default() {
            self.disc_overrides.remove(&fingerprint);
        } else {
            self.disc_overrides.insert(fingerprint, settings);
        }
        *self.shared.disc_standard_overrides.lock().unwrap() = self
            .disc_overrides
            .iter()
            .filter_map(|(fingerprint, settings)| {
                settings
                    .video_standard
                    .map(|standard| (fingerprint.clone(), standard))
            })
            .collect();
    }

    fn current_disc_override(&self) -> DiscOverride {
        self.active_fingerprint
            .as_ref()
            .and_then(|fingerprint| self.disc_overrides.get(fingerprint))
            .copied()
            .unwrap_or_default()
    }

    /// Rebuild the library list from the configured folders. Each library is
    /// a folder of per-disc subdirectories (each holding a `.cue` or eligible
    /// Store ZIP), matching how the disc images are organized; loose CUE/ZIP
    /// files are also picked up.
    fn scan_libraries(&mut self) {
        self.library.clear();
        for (category, dir) in self.libraries.iter().enumerate() {
            let Some(dir) = dir.as_ref().map(PathBuf::from).filter(|p| p.is_dir()) else {
                continue;
            };
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut paths: Vec<PathBuf> =
                entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
            paths.sort_by_key(|p| p.file_name().map(|s| s.to_ascii_lowercase()));
            for path in paths {
                let cue = if path.is_dir() {
                    std::fs::read_dir(&path).ok().and_then(|inner| {
                        let mut candidates: Vec<PathBuf> = inner
                            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                            .filter(|candidate| {
                                candidate.extension().is_some_and(|extension| {
                                    extension.eq_ignore_ascii_case("cue")
                                        || extension.eq_ignore_ascii_case("zip")
                                })
                            })
                            .collect();
                        candidates.sort_by_key(|candidate| {
                            (
                                is_zip_path(candidate),
                                candidate.file_name().map(|name| name.to_ascii_lowercase()),
                            )
                        });
                        candidates.into_iter().find(|candidate| {
                            !is_zip_path(candidate) || store_zip::is_eligible(candidate)
                        })
                    })
                } else if path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("cue")
                        || (extension.eq_ignore_ascii_case("zip") && store_zip::is_eligible(&path))
                }) {
                    Some(path.clone())
                } else {
                    None
                };
                if let Some(cue) = cue {
                    let title = if path.is_dir() { &path } else { &cue }
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| cue.display().to_string());
                    self.library.push(LibraryEntry {
                        title,
                        category,
                        cue,
                    });
                }
            }
        }
        // Open on a format that actually has discs. Done here rather than per
        // frame so a deliberate click onto an empty tab is not undone.
        if !self.library.iter().any(|e| e.category == self.library_tab) {
            if let Some(entry) = self.library.first() {
                self.library_tab = entry.category;
                self.library_focus = entry.category;
            }
        }
        let visible_count = self
            .library
            .iter()
            .filter(|entry| entry.category == self.library_tab)
            .count();
        self.library_selection = self.library_selection.min(visible_count.saturating_sub(1));
    }

    /// Draw the disc-library browser and return a disc to load if clicked.
    fn paint_library(&mut self, ctx: &egui::Context) -> Option<PathBuf> {
        // Per-format disc counts. Every tab stays clickable: selecting a
        // format with no folder set is how the user reaches its prompt.
        let mut counts = [0usize; LIBRARY_SLOTS.len()];
        for entry in &self.library {
            counts[entry.category] += 1;
        }

        let mut selected = self.library_tab;
        let mut load_path = None;
        let mut needs_open = false;
        // Library folder to configure, set from the empty-state link.
        let mut pick_slot: Option<usize> = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            let visuals = ui.visuals();
            let hover_fill = visuals.widgets.hovered.weak_bg_fill;
            let text_normal = visuals.text_color();
            let text_strong = visuals.strong_text_color();

            ui.add_space(18.0);

            // Format tabs in a shared squircle container. The Frame expands to
            // full width inside a centered layout, so center it manually with
            // the strip width measured last frame.
            let container_fill = ui.visuals().faint_bg_color;
            const GROUP_GAP: f32 = 8.0;
            let pad = ((ui.available_width() - self.library_strip_w) * 0.5).max(0.0);
            let mut strip_w = self.library_strip_w;
            let mut open_clicked = false;
            let squircle = || {
                egui::Frame::new()
                    .fill(container_fill)
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(egui::Margin::symmetric(4, 4))
            };
            ui.horizontal(|ui| {
                ui.add_space(pad);
                let tabs = squircle().show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (slot, name) in LIBRARY_SLOTS.iter().enumerate() {
                        // Formats with no discs are dimmed but still
                        // selectable, so their set-folder prompt is reachable.
                        let mut text = egui::RichText::new(*name).size(15.0);
                        if counts[slot] == 0 {
                            text = text.weak();
                        }
                        if ui.selectable_label(selected == slot, text).clicked() {
                            selected = slot;
                            self.library_focus = slot;
                            self.library_selection = 0;
                        }
                    }
                });
                ui.add_space(GROUP_GAP);
                let open = squircle().show(ui, |ui| {
                    if ui
                        .selectable_label(
                            self.library_focus == LIBRARY_OPEN_FOCUS,
                            egui::RichText::new("Open .cue").size(15.0),
                        )
                        .clicked()
                    {
                        self.library_focus = LIBRARY_OPEN_FOCUS;
                        open_clicked = true;
                    }
                });
                strip_w = tabs.response.rect.width() + GROUP_GAP + open.response.rect.width();
            });
            self.library_strip_w = strip_w;
            ui.add_space(10.0);
            if open_clicked {
                needs_open = true;
            }

            // Per-format empty state: prompt for the selected format's folder.
            if counts[selected] == 0 {
                ui.add_space(48.0);
                ui.vertical_centered(|ui| {
                    let configured = self.libraries[selected].is_some();
                    ui.weak(if configured {
                        "No discs found in this folder."
                    } else {
                        "No library folder set for this format."
                    });
                    ui.add_space(4.0);
                    if ui
                        .link(format!(
                            "Click here to set {} library folder.",
                            LIBRARY_SLOTS[selected]
                        ))
                        .clicked()
                    {
                        pick_slot = Some(selected);
                    }
                });
                return;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let col_w = ui.available_width().min(640.0);
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(col_w);
                        ui.add_space(4.0);
                        for (row, entry) in self
                            .library
                            .iter()
                            .filter(|e| e.category == selected)
                            .enumerate()
                        {
                            let row_h = 30.0;
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), row_h),
                                egui::Sense::click(),
                            );
                            let hovered = resp.hovered();
                            let controller_selected =
                                self.library_focus == selected && row == self.library_selection;
                            if hovered || controller_selected {
                                ui.painter().rect_filled(
                                    rect,
                                    egui::CornerRadius::same(5),
                                    hover_fill,
                                );
                            }
                            if hovered {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            ui.painter().text(
                                egui::pos2(rect.left() + 12.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                &entry.title,
                                egui::FontId::proportional(15.0),
                                if hovered || controller_selected {
                                    text_strong
                                } else {
                                    text_normal
                                },
                            );
                            if resp.clicked() {
                                self.library_focus = selected;
                                self.library_selection = row;
                                load_path = Some(entry.cue.clone());
                            }
                        }
                        ui.add_space(16.0);
                    });
                });
        });
        self.library_tab = selected;
        if let Some(slot) = pick_slot {
            if let Some(dir) = rfd::FileDialog::new()
                .set_title(format!("Choose the {} library folder", LIBRARY_SLOTS[slot]))
                .pick_folder()
            {
                self.libraries[slot] = Some(dir.display().to_string());
                self.scan_libraries();
            }
        }
        if needs_open {
            self.open_disc();
        }
        ctx.request_repaint_after(Duration::from_millis(50));
        load_path
    }

    fn enter_library_from_quick_menu(&mut self) {
        self.quick_menu_open = false;
        self.show_library = true;
        self.shared.audio_suppressed.store(true, Ordering::Relaxed);
        self.scan_libraries();
        self.library_focus = self.library_tab;
        self.library_selection = 0;
        self.library_dpad_y = 0;
    }

    fn paint_quick_menu(&mut self, ctx: &egui::Context) {
        if !self.quick_menu_open {
            return;
        }

        let mut chosen = None;
        egui::Area::new(egui::Id::new("controller_quick_menu"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_black_alpha(235))
                    .stroke(egui::Stroke::new(1.0, UI_BORDER))
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::symmetric(18, 16))
                    .show(ui, |ui| {
                        ui.set_width(280.0);
                        ui.vertical_centered(|ui| {
                            ui.heading("E-Di");
                            ui.add_space(10.0);
                            for (row, label) in ["Go to Library", "Return to Current Title"]
                                .iter()
                                .enumerate()
                            {
                                let selected = self.quick_menu_selection == row;
                                let response =
                                    ui.add_sized(
                                        [260.0, 38.0],
                                        egui::Button::new(
                                            egui::RichText::new(*label)
                                                .size(15.0)
                                                .color(egui::Color32::WHITE),
                                        )
                                        .fill(if selected { UI_ACCENT } else { UI_SURFACE }),
                                    );
                                if response.hovered() {
                                    self.quick_menu_selection = row;
                                }
                                if response.clicked() {
                                    chosen = Some(row);
                                }
                                ui.add_space(4.0);
                            }
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("D-pad · A select · B or Start close")
                                    .size(11.0)
                                    .color(UI_MUTED_TEXT),
                            );
                        });
                    });
            });

        match chosen {
            Some(0) => self.enter_library_from_quick_menu(),
            Some(1) => self.quick_menu_open = false,
            _ => {}
        }
    }

    fn texture_options(&self) -> egui::TextureOptions {
        if self.smooth_scaling {
            egui::TextureOptions::LINEAR
        } else {
            egui::TextureOptions::NEAREST
        }
    }

    fn open_disc(&mut self) {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Open a CD-i disc image")
            .add_filter("CD-i discs", &["cue", "zip"]);
        // Start in the first configured library folder that exists.
        if let Some(dir) = self
            .libraries
            .iter()
            .flatten()
            .map(PathBuf::from)
            .find(|p| p.is_dir())
        {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.load_disc_path(path);
        }
    }

    /// Load a disc from a known path (library click or file dialog): route it
    /// to both the emulation core and the Photo CD detector, and leave the
    /// library view.
    fn load_disc_path(&mut self, path: PathBuf) {
        let source_name = display_name(&path);
        *self.shared.status.lock().unwrap() = if is_zip_path(&path) {
            format!("Opening Store ZIP {source_name}…")
        } else {
            format!("Loading {source_name}…")
        };
        let (cue_path, archive_guard) = match resolve_disc_source(&path) {
            Ok(resolved) => resolved,
            Err(error) => {
                *self.shared.status.lock().unwrap() = format!("Open failed: {error}");
                return;
            }
        };
        *self.shared.disc_identity.lock().unwrap() = None;
        self.sync_disc_identity();
        self.photocd = None;
        self.show_library = false;
        self.shared.audio_suppressed.store(false, Ordering::Relaxed);
        let _ = self.pcd_tx.send(PcdCmd::Open {
            path: cue_path.clone(),
            archive_guard: archive_guard.clone(),
        });
        *self.shared.command.lock().unwrap() = Some(MachineCommand::LoadDisc {
            path: cue_path,
            archive_guard,
        });
    }

    fn eject_disc(&mut self) {
        self.photocd = None;
        *self.shared.disc_identity.lock().unwrap() = None;
        self.sync_disc_identity();
        let _ = self.pcd_tx.send(PcdCmd::Close);
        *self.shared.command.lock().unwrap() = Some(MachineCommand::EjectDisc);
    }

    /// Ask the worker to decode the current photo at the current tier.
    fn request_photo_decode(&mut self) {
        if let Some(p) = &mut self.photocd {
            if p.names.is_empty() {
                return;
            }
            p.decoding = true;
            p.error = None;
            let _ = self.pcd_tx.send(PcdCmd::Decode {
                index: p.current,
                tier: p.tier,
            });
        }
    }

    fn save_player_screenshot(&self) {
        let image = {
            let frame = self.shared.frame.lock().unwrap();
            screenshot_image(&frame, self.display_area, self.crt_aspect)
        };
        let disc_name = self.shared.disc_name.lock().unwrap().clone();
        let stem = disc_name
            .as_deref()
            .and_then(|name| std::path::Path::new(name).file_stem())
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or(APP_NAME);
        let safe_stem: String = stem
            .chars()
            .map(|ch| {
                if matches!(ch, '/' | ':' | '\\') {
                    '_'
                } else {
                    ch
                }
            })
            .collect();
        let suggested_name = format!("{safe_stem} screenshot.png");
        match save_rgb_png(
            "Save player screenshot as PNG",
            &suggested_name,
            self.save_dir.as_deref(),
            &image,
        ) {
            Some(Ok(path)) => {
                *self.shared.status.lock().unwrap() =
                    format!("Screenshot saved to {}", path.display());
            }
            Some(Err(error)) => {
                *self.shared.status.lock().unwrap() = format!("Screenshot save failed: {error}");
            }
            None => {}
        }
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
        ui.label(egui::RichText::new("Settings").size(22.0).strong());
        ui.label(
            egui::RichText::new("Player, input, and library preferences")
                .color(UI_MUTED_TEXT)
                .size(12.0),
        );
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(18, 21, 24))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(4, 4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::System, "System");
                    ui.selectable_value(&mut self.settings_tab, SettingsTab::Input, "Input");
                    ui.selectable_value(
                        &mut self.settings_tab,
                        SettingsTab::Libraries,
                        "Libraries",
                    );
                });
            });
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(UI_SURFACE)
            .stroke(egui::Stroke::new(1.0, UI_BORDER))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(14, 12))
            .show(ui, |ui| match self.settings_tab {
                SettingsTab::System => self.settings_system_ui(ui),
                SettingsTab::Input => self.settings_input_ui(ui),
                SettingsTab::Libraries => self.settings_libraries_ui(ui),
            });
    }

    fn settings_libraries_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Disc Libraries");
        ui.label(
            "Folders where your disc images live. The Open dialog starts in the first configured library.",
        );
        ui.add_space(4.0);
        let mut libraries_changed = false;
        for (slot, name) in LIBRARY_SLOTS.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.strong(format!("{name}:"));
                if ui.button("Choose…").clicked() {
                    if let Some(dir) = rfd::FileDialog::new()
                        .set_title(format!("Choose the {name} library folder"))
                        .pick_folder()
                    {
                        self.libraries[slot] = Some(dir.display().to_string());
                        libraries_changed = true;
                    }
                }
                if self.libraries[slot].is_some() && ui.button("Clear").clicked() {
                    self.libraries[slot] = None;
                    libraries_changed = true;
                }
            });
            match &self.libraries[slot] {
                Some(path) => {
                    ui.monospace(path);
                }
                None => {
                    ui.weak("not set");
                }
            }
            ui.add_space(6.0);
        }
        if libraries_changed {
            self.scan_libraries();
        }

        ui.separator();
        ui.heading("Screenshot Save Folder");
        ui.label("Where saved photos and screenshots go. Defaults to Downloads.");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Choose…").clicked() {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("Choose the screenshot save folder")
                    .pick_folder()
                {
                    self.save_dir = Some(dir.display().to_string());
                }
            }
            if self.save_dir.is_some() && ui.button("Use Downloads").clicked() {
                self.save_dir = None;
            }
        });
        match &self.save_dir {
            Some(path) => {
                ui.monospace(path);
            }
            None => {
                ui.weak(format!("Downloads ({})", downloads_dir().display()));
            }
        }
    }

    fn settings_system_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("System ROM");
        ui.label(self.shared.rom_status.lock().unwrap().as_str());
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("system_rom")
                .selected_text("Select model…")
                .show_ui(ui, |ui| {
                    for entry in SYSTEM_ROMS {
                        if ui.selectable_label(false, entry.menu_text()).clicked() {
                            // Region only seeds the default video standard;
                            // it stays overridable below.
                            self.shared.pal.store(entry.pal, Ordering::Relaxed);
                            *self.shared.command.lock().unwrap() =
                                Some(MachineCommand::SetSystemRom {
                                    image: entry.bytes.to_vec(),
                                    label: entry.display(),
                                });
                        }
                    }
                });
            if ui.button("Choose ROM…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Select a CD-i system ROM")
                    .add_filter("ROM images", &["rom", "bin"])
                    .pick_file()
                {
                    if let Ok(image) = std::fs::read(&path) {
                        *self.shared.command.lock().unwrap() = Some(MachineCommand::SetSystemRom {
                            image,
                            label: display_name(&path),
                        });
                    }
                }
            }
        });
        ui.label(
            "Region names the market a model was sold into and only sets the default video standard below — the same ROM shipped in 50 Hz and 60 Hz players. Changing the ROM resets the machine and keeps the disc.",
        );
        ui.separator();
        ui.heading("Player Storage");
        ui.label(
            "Battery-backed CD-i memory stores saved games, high scores, and player settings.",
        );
        if self.reset_nvram_confirmation {
            egui::Frame::new()
                .fill(PARENTAL_PASSCODE_BACKGROUND)
                .stroke(egui::Stroke::new(1.0, PARENTAL_PASSCODE_COLOR))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.label("Clear saved games, high scores, and settings for this CD-i board?");
                    ui.weak("E-Di creates a timestamped backup first, then restarts the title.");
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.reset_nvram_confirmation = false;
                        }
                        if ui.button("Back Up and Reset").clicked() {
                            *self.shared.command.lock().unwrap() = Some(MachineCommand::ResetNvram);
                            self.reset_nvram_confirmation = false;
                            self.settings_open = false;
                        }
                    });
                });
        } else if ui.button("Reset Player Storage…").clicked() {
            self.reset_nvram_confirmation = true;
        }
        ui.weak("Resetting first creates a recoverable backup, then restarts the current title.");
        ui.separator();
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
        let identity = self.shared.disc_identity.lock().unwrap().clone();
        if let Some(identity) = &identity {
            if let Some(profile) = &identity.profile {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Exact disc:");
                    ui.label(&profile.name);
                    ui.hyperlink_to(
                        format!("Redump #{}", profile.redump_id),
                        format!("https://redump.info/disc/{}/", profile.redump_id),
                    );
                });
                let mut details = Vec::new();
                if let Some(serial) = &profile.serial {
                    details.push(format!("serial {serial}"));
                }
                if let Some(version) = &profile.version {
                    details.push(format!("version {version}"));
                }
                details.push(format!(
                    "{} recommendation",
                    if profile.video_standard.is_pal() {
                        "PAL/50 Hz"
                    } else {
                        "NTSC/60 Hz"
                    }
                ));
                ui.weak(details.join(" · "));
            } else {
                ui.weak(format!(
                    "Exact pressing identified ({}) but no display profile is tracked yet.",
                    &identity.fingerprint[..identity.fingerprint.len().min(12)]
                ));
            }
            ui.add_space(4.0);
        }
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
            if self.active_fingerprint.is_some() {
                let mut settings = self.current_disc_override();
                settings.video_standard = Some(if pal {
                    VideoStandardRecommendation::Pal
                } else {
                    VideoStandardRecommendation::Ntsc
                });
                self.store_disc_override(settings);
            }
        }
        let remembered_standard = self.current_disc_override().video_standard;
        if let Some(remembered) = remembered_standard {
            ui.horizontal(|ui| {
                ui.weak(format!(
                    "Remembered for this exact disc: {}",
                    if remembered.is_pal() {
                        "PAL/50 Hz"
                    } else {
                        "NTSC/60 Hz"
                    }
                ));
                if ui.button("Use detected default").clicked() {
                    let mut settings = self.current_disc_override();
                    settings.video_standard = None;
                    self.store_disc_override(settings);
                    if let Some(profile) = identity
                        .as_ref()
                        .and_then(|identity| identity.profile.as_ref())
                    {
                        let profile_pal = profile.video_standard.is_pal();
                        if profile_pal != self.shared.pal.load(Ordering::Relaxed) {
                            *self.shared.command.lock().unwrap() =
                                Some(MachineCommand::SetVideoStandard(if profile_pal {
                                    cdi_core::VideoStandard::Pal
                                } else {
                                    cdi_core::VideoStandard::Ntsc
                                }));
                        }
                    }
                }
            });
        }
        let mut auto_region = self.shared.auto_region.load(Ordering::Relaxed);
        if ui
            .checkbox(&mut auto_region, "Match region to disc on load")
            .changed()
        {
            self.shared
                .auto_region
                .store(auto_region, Ordering::Relaxed);
        }
        ui.label(
            "Uses an exact Redump profile when available, otherwise an unambiguous region tag in the disc's name (USA/Japan → 60 Hz, Europe → 50 Hz). Mixed-region and untagged pressings keep the current setting. This changes timing, not the screen shape.",
        );
        ui.horizontal(|ui| {
            ui.label("Display area:");
            ui.radio_value(
                &mut self.display_area,
                DisplayArea::TypicalCrt,
                "Typical CRT",
            );
            ui.radio_value(
                &mut self.display_area,
                DisplayArea::FullSignal,
                "Full signal",
            );
        });
        ui.label(
            "Typical CRT hides the nominal analog overscan margin on a 525-line player. Full signal exposes the complete hardware aperture. PAL and MCD212 Compatibility Mode remain hardware-defined.",
        );
        ui.checkbox(
            &mut self.crt_aspect,
            "Correct for Philips CD-i CRT pixel shape",
        );
        ui.label(
            "Uses Philips player measurements (PAL 1.025, NTSC 1.225 pixel height/width). Disable only for raw square-pixel diagnostics.",
        );
        ui.checkbox(&mut self.smooth_scaling, "Smooth scaling");
        ui.checkbox(&mut self.show_fps, "Show frame rate");
    }

    fn settings_input_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("CD-i Peripherals");
        ui.label("The emulated player exposes a relative pointing device; every input source below drives it.");

        // A pending keyboard rebind captures the next key pressed while the
        // settings window has focus.
        if let Some(slot) = self.kb_rebind {
            let captured = ui.ctx().input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Key {
                        key, pressed: true, ..
                    } => Some(*key),
                    _ => None,
                })
            });
            if let Some(key) = captured {
                if key != egui::Key::Escape {
                    self.kb_keys[slot] = key;
                }
                self.kb_rebind = None;
            }
        }

        egui::CollapsingHeader::new("Pointing device — Mouse")
            .default_open(true)
            .show(ui, |ui| {
                ui.checkbox(&mut self.capture_mouse_enabled, "Capture mouse on click");
                ui.label("Captured: relative motion like a real CD-i mouse. Uncaptured: the pointer follows the cursor over the picture.");
            });

        if ui.available_width() >= 620.0 {
            ui.columns(2, |columns| {
                self.settings_keyboard_ui(&mut columns[0]);
                self.settings_controller_ui(&mut columns[1]);
            });
        } else {
            self.settings_keyboard_ui(ui);
            self.settings_controller_ui(ui);
        }

        ui.separator();
        ui.heading("Emulator Controls");
        // A pending fast-forward rebind captures the next key pressed.
        if self.ff_rebind {
            let captured = ui.ctx().input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Key {
                        key, pressed: true, ..
                    } => Some(*key),
                    _ => None,
                })
            });
            if let Some(key) = captured {
                if key != egui::Key::Escape {
                    self.ff_key = key;
                }
                self.ff_rebind = false;
            }
        }
        ui.horizontal(|ui| {
            ui.label("Fast-forward (hold):");
            let text = if self.ff_rebind {
                "Press a key… (Esc cancels)".to_owned()
            } else {
                self.ff_key.name().to_owned()
            };
            if ui.button(text).clicked() {
                self.ff_rebind = !self.ff_rebind;
            }
            if self.ff_key != default_ff_key() && ui.button("Reset").clicked() {
                self.ff_key = default_ff_key();
                self.ff_rebind = false;
            }
        });
        ui.label("Runs the emulation as fast as this machine allows while held; audio is silenced during it.");

        egui::CollapsingHeader::new("CD-i Keyboard (not yet emulated)")
            .default_open(false)
            .show(ui, |ui| {
                ui.label(
                    "Some CD-i players (notably authoring/industrial models) support a keyboard peripheral. Emulating it needs core-side support for the keyboard port; planned in TODO.md.",
                );
            });
    }

    fn settings_keyboard_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Pointing device — Keyboard")
            .default_open(true)
            .show(ui, |ui| {
                ui.add(egui::Slider::new(&mut self.kb_speed, 1.0..=20.0).text("Pointer speed"));
                for (slot, name) in KB_SLOTS.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{name}:"));
                        let text = if self.kb_rebind == Some(slot) {
                            "Press a key… (Esc cancels)".to_owned()
                        } else {
                            self.kb_keys[slot].name().to_owned()
                        };
                        if ui.button(text).clicked() {
                            self.kb_rebind = if self.kb_rebind == Some(slot) {
                                None
                            } else {
                                Some(slot)
                            };
                        }
                    });
                }
                if ui.button("Reset keyboard defaults").clicked() {
                    let defaults = Prefs::default();
                    self.kb_speed = defaults.kb_speed;
                    self.kb_keys = [
                        defaults.kb_up,
                        defaults.kb_down,
                        defaults.kb_left,
                        defaults.kb_right,
                        defaults.kb_button1,
                        defaults.kb_button2,
                    ];
                    self.kb_rebind = None;
                }
            });
    }

    fn settings_controller_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Pointing device — Controller")
            .default_open(true)
            .show(ui, |ui| match &self.gamepad {
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
                    ui.add(
                        egui::Slider::new(&mut self.pad_speed, 1.0..=20.0).text("Pointer speed"),
                    );
                    ui.checkbox(&mut self.pad_analog_enabled, "Enable left analog stick");
                    ui.add_enabled(
                        self.pad_analog_enabled,
                        egui::Slider::new(&mut self.pad_deadzone, 0.0..=0.5).text("Stick deadzone"),
                    );
                    for (button, label) in [(0, "CD-i button 1:"), (1, "CD-i button 2:")] {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            let text = if self.pad_rebind == Some(button) {
                                "Press a button…"
                            } else if button == 0 {
                                button_name(self.pad_button1)
                            } else {
                                button_name(self.pad_button2)
                            };
                            if ui.button(text).clicked() {
                                self.pad_rebind = if self.pad_rebind == Some(button) {
                                    None
                                } else {
                                    Some(button)
                                };
                            }
                        });
                    }
                    if ui.button("Reset controller defaults").clicked() {
                        let defaults = Prefs::default();
                        self.pad_speed = defaults.pad_speed;
                        self.pad_deadzone = defaults.pad_deadzone;
                        self.pad_analog_enabled = defaults.pad_analog_enabled;
                        self.pad_button1 = defaults.pad_button1;
                        self.pad_button2 = defaults.pad_button2;
                        self.pad_rebind = None;
                    }
                    ui.weak(if self.pad_analog_enabled {
                        "The D-pad is always active and takes priority over stick drift."
                    } else {
                        "D-pad movement remains active; the left stick is disabled."
                    });
                    ui.weak("Controller layouts are auto-detected.");
                }
                None => {
                    ui.label("Controller support unavailable on this system");
                }
            });
    }

    /// Poll the bindable keyboard controls and translate them onto the CD-i
    /// pointer, mirroring the gamepad path (relative deltas, SLAVE-clamped).
    fn poll_keyboard(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        // While viewing photos the keyboard drives image navigation, not the
        // CD-i pointer.
        if self.photocd.as_ref().is_some_and(|p| p.view_photo) {
            return;
        }
        let (delta, buttons) = ctx.input(|input| {
            let held = |key: egui::Key| input.key_down(key);
            let delta = egui::vec2(
                f32::from(held(self.kb_keys[3])) - f32::from(held(self.kb_keys[2])),
                f32::from(held(self.kb_keys[1])) - f32::from(held(self.kb_keys[0])),
            );
            let mut buttons = 0u8;
            if held(self.kb_keys[4]) {
                buttons |= 1;
            }
            if held(self.kb_keys[5]) {
                buttons |= 2;
            }
            (delta, buttons)
        });
        let scaled = delta * self.kb_speed + self.kb_frac;
        let step = egui::vec2(scaled.x.trunc(), scaled.y.trunc());
        self.kb_frac = scaled - step;
        // Don't feed a key being captured for a settings rebind into the game.
        let buttons = if self.kb_rebind.is_some() { 0 } else { buttons };
        let buttons_changed = buttons != self.kb_buttons;
        self.kb_buttons = buttons;
        if step == egui::Vec2::ZERO && !buttons_changed {
            return;
        }
        let mut input = self.shared.input.lock().unwrap();
        input.dx += step.x as i32;
        input.dy += step.y as i32;
        input.buttons = self.game_buttons | self.pad_buttons | self.kb_buttons;
    }

    /// Handle controller navigation while the Library owns the central view.
    /// Button-press events provide natural edge triggering, so held triggers
    /// and D-pad directions cannot race through several tabs or rows.
    fn poll_library_gamepad(&mut self) -> Option<PathBuf> {
        let gilrs = self.gamepad.as_mut()?;
        let mut actions = Vec::new();
        while let Some(event) = gilrs.next_event() {
            if let gilrs::EventType::ButtonPressed(button, _) = event.event {
                if let Some(action) = library_pad_action(button, self.pad_button1, self.pad_button2)
                {
                    // D-pad buttons and unmapped D-pad hat axes share the
                    // state path below, avoiding a double step on pads which
                    // expose both representations.
                    if !matches!(
                        action,
                        LibraryPadAction::PreviousDisc | LibraryPadAction::NextDisc
                    ) {
                        actions.push(action);
                    }
                }
            }
        }
        let dpad_y = gilrs
            .gamepads()
            .map(|(_, pad)| {
                library_dpad_direction(
                    pad.value(gilrs::Axis::DPadY),
                    pad.is_pressed(gilrs::Button::DPadUp),
                    pad.is_pressed(gilrs::Button::DPadDown),
                )
            })
            .find(|&direction| direction != 0)
            .unwrap_or(0);
        if dpad_y != self.library_dpad_y {
            if dpad_y < 0 {
                actions.push(LibraryPadAction::PreviousDisc);
            } else if dpad_y > 0 {
                actions.push(LibraryPadAction::NextDisc);
            }
            self.library_dpad_y = dpad_y;
        }

        let mut load_path = None;
        let mut open_disc = false;
        for action in actions {
            match action {
                LibraryPadAction::PreviousTab | LibraryPadAction::NextTab => {
                    self.library_focus = cycle_library_focus(
                        self.library_focus,
                        action == LibraryPadAction::NextTab,
                    );
                    self.library_selection = 0;
                    if self.library_focus < LIBRARY_SLOTS.len() {
                        self.library_tab = self.library_focus;
                    }
                }
                LibraryPadAction::PreviousDisc | LibraryPadAction::NextDisc => {
                    if self.library_focus >= LIBRARY_SLOTS.len() {
                        continue;
                    }
                    let count = self
                        .library
                        .iter()
                        .filter(|entry| entry.category == self.library_tab)
                        .count();
                    if count == 0 {
                        self.library_selection = 0;
                    } else if action == LibraryPadAction::NextDisc {
                        self.library_selection = (self.library_selection + 1) % count;
                    } else {
                        self.library_selection = (self.library_selection + count - 1) % count;
                    }
                }
                LibraryPadAction::Activate => {
                    if self.library_focus == LIBRARY_OPEN_FOCUS {
                        open_disc = true;
                    } else {
                        load_path = self
                            .library
                            .iter()
                            .filter(|entry| entry.category == self.library_tab)
                            .nth(self.library_selection)
                            .map(|entry| entry.cue.clone());
                    }
                }
                LibraryPadAction::Back => {
                    self.show_library = false;
                    self.shared.audio_suppressed.store(false, Ordering::Relaxed);
                }
            }
        }
        self.pad_buttons = 0;
        self.pad_frac = egui::Vec2::ZERO;
        if open_disc {
            self.open_disc();
        }
        load_path
    }

    /// Poll connected gamepads and translate them onto the CD-i pointer:
    /// left stick and d-pad move, two configurable buttons map to CD-i
    /// buttons 1/2.
    fn poll_gamepad(&mut self, ctx: &egui::Context) {
        let Some(gilrs) = self.gamepad.as_mut() else {
            return;
        };
        // Drain the event queue so cached gamepad state stays current; a
        // pending rebind captures the first button press seen here.
        let mut start_pressed = false;
        let mut activate_pressed = false;
        let mut back_pressed = false;
        while let Some(event) = gilrs.next_event() {
            if let gilrs::EventType::ButtonPressed(button, _) = event.event {
                start_pressed |= button == gilrs::Button::Start;
                activate_pressed |= button == self.pad_button1 || button == gilrs::Button::South;
                back_pressed |= button == self.pad_button2 || button == gilrs::Button::East;
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
        let mut menu_dpad_y = 0;
        let mut stick_deflection = egui::Vec2::ZERO;
        let mut dpad_deflection = egui::Vec2::ZERO;
        let mut dpad_active = false;
        let mut buttons = 0u8;
        for (_id, pad) in gilrs.gamepads() {
            if menu_dpad_y == 0 {
                menu_dpad_y = library_dpad_direction(
                    pad.value(gilrs::Axis::DPadY),
                    pad.is_pressed(gilrs::Button::DPadUp),
                    pad.is_pressed(gilrs::Button::DPadDown),
                );
            }

            // Stick up moves the pointer up (screen Y grows downward).
            let stick = egui::vec2(
                pad.value(gilrs::Axis::LeftStickX),
                -pad.value(gilrs::Axis::LeftStickY),
            );
            stick_deflection += controller_stick_deflection(stick, deadzone);

            let (dpad, active) = controller_dpad_deflection(
                pad.value(gilrs::Axis::DPadX),
                pad.value(gilrs::Axis::DPadY),
                pad.is_pressed(gilrs::Button::DPadLeft),
                pad.is_pressed(gilrs::Button::DPadRight),
                pad.is_pressed(gilrs::Button::DPadUp),
                pad.is_pressed(gilrs::Button::DPadDown),
            );
            dpad_deflection += dpad;
            dpad_active |= active;
            if pad.is_pressed(self.pad_button1) {
                buttons |= 1;
            }
            if pad.is_pressed(self.pad_button2) {
                buttons |= 2;
            }
        }

        if start_pressed && !self.show_library {
            self.quick_menu_open = !self.quick_menu_open;
            self.quick_menu_selection = 0;
            self.quick_menu_dpad_y = menu_dpad_y;
            if self.quick_menu_open && self.mouse_captured {
                self.set_mouse_capture(ctx, false);
            }
        } else if self.quick_menu_open {
            if menu_dpad_y != self.quick_menu_dpad_y {
                if menu_dpad_y != 0 {
                    self.quick_menu_selection ^= 1;
                }
                self.quick_menu_dpad_y = menu_dpad_y;
            }
            if back_pressed {
                self.quick_menu_open = false;
            } else if activate_pressed {
                if self.quick_menu_selection == 0 {
                    self.enter_library_from_quick_menu();
                } else {
                    self.quick_menu_open = false;
                }
            }
        }

        if self.quick_menu_open {
            self.pad_buttons = 0;
            self.pad_frac = egui::Vec2::ZERO;
            let mut input = self.shared.input.lock().unwrap();
            input.buttons = 0;
            return;
        }

        // Don't feed the press being captured for a rebind into the game.
        if self.pad_rebind.is_some() {
            buttons = 0;
        }

        let deflection = controller_deflection(
            stick_deflection,
            dpad_deflection,
            dpad_active,
            self.pad_analog_enabled,
        );
        let delta = deflection * self.pad_speed + self.pad_frac;
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
        input.buttons = self.game_buttons | self.pad_buttons | self.kb_buttons;
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
            libraries: self.libraries.to_vec(),
            save_dir: self.save_dir.clone(),
            show_fps: self.show_fps,
            smooth_scaling: self.smooth_scaling,
            crt_aspect: self.crt_aspect,
            display_area: self.display_area,
            capture_mouse_enabled: self.capture_mouse_enabled,
            auto_region: self.shared.auto_region.load(Ordering::Relaxed),
            disc_overrides: self.disc_overrides.clone(),
            ff_key: self.ff_key,
            pad_speed: self.pad_speed,
            pad_deadzone: self.pad_deadzone,
            pad_analog_enabled: self.pad_analog_enabled,
            pad_button1: self.pad_button1,
            pad_button2: self.pad_button2,
            kb_speed: self.kb_speed,
            kb_up: self.kb_keys[0],
            kb_down: self.kb_keys[1],
            kb_left: self.kb_keys[2],
            kb_right: self.kb_keys[3],
            kb_button1: self.kb_keys[4],
            kb_button2: self.kb_keys[5],
        };
        if let Ok(json) = serde_json::to_string(&prefs) {
            storage.set_string(PREFS_KEY, json);
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync_disc_identity();

        // Photo CD worker events.
        let mut request_first_decode = false;
        while let Ok(event) = self.pcd_rx.try_recv() {
            match event {
                PcdEvent::Opened {
                    names,
                    max_tier,
                    has_cdi_app,
                } => {
                    // With a CD-i application the disc's own program drives
                    // display by default and the host viewer is an opt-in
                    // high-fidelity mode. Without one (e.g. Kodak USA High
                    // Sierra discs, Aktuelles Berlin) the viewer is the only
                    // mode and takes over immediately.
                    let view_photo = !has_cdi_app;
                    self.photocd = Some(PhotoCdUi {
                        names,
                        current: 0,
                        tier: 0,
                        max_tier,
                        has_cdi_app,
                        view_photo,
                        slideshow: false,
                        last_advance: Instant::now(),
                        decoding: false,
                        decoded: None,
                        texture: None,
                        error: None,
                    });
                    request_first_decode = view_photo;
                }
                PcdEvent::NotPhotoCd => self.photocd = None,
                PcdEvent::Decoded { index, image } => {
                    if let Some(p) = &mut self.photocd {
                        p.decoding = false;
                        p.current = index.min(p.names.len().saturating_sub(1));
                        let size = [image.width as usize, image.height as usize];
                        let color = egui::ColorImage::from_rgb(size, &image.rgb);
                        p.texture = Some(ctx.load_texture(
                            "photocd_image",
                            color,
                            egui::TextureOptions::LINEAR,
                        ));
                        p.decoded = Some(image);
                    }
                }
                PcdEvent::DecodeError(error) => {
                    if let Some(p) = &mut self.photocd {
                        p.decoding = false;
                        p.error = Some(error);
                    }
                }
            }
        }
        // Left/right arrows change the picture while viewing photos (unless a
        // text field has focus). Consumed here so they don't also drive the
        // CD-i pointer via poll_keyboard.
        if self
            .photocd
            .as_ref()
            .is_some_and(|p| p.view_photo && p.names.len() > 1)
            && !ctx.wants_keyboard_input()
        {
            let (left, right) = ctx.input_mut(|input| {
                (
                    input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
                    input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
                )
            });
            if let Some(p) = &mut self.photocd {
                if left {
                    p.current = (p.current + p.names.len() - 1) % p.names.len();
                    p.last_advance = Instant::now();
                    request_first_decode = true;
                } else if right {
                    p.current = (p.current + 1) % p.names.len();
                    p.last_advance = Instant::now();
                    request_first_decode = true;
                }
            }
        }

        // Slideshow auto-advance.
        if let Some(p) = &mut self.photocd {
            if p.slideshow
                && p.view_photo
                && !p.decoding
                && p.names.len() > 1
                && p.last_advance.elapsed() >= Duration::from_secs(5)
            {
                p.current = (p.current + 1) % p.names.len();
                p.last_advance = Instant::now();
                request_first_decode = true;
            }
        }
        if request_first_decode {
            self.request_photo_decode();
        }

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

        // In fullscreen the top disc bar and bottom toolbars are hidden for an
        // uncluttered view; keyboard shortcuts (Esc, photo arrows) still work.
        let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));

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

        // The loaded disc names the window, like a document title, rather
        // than costing a row of the picture area.
        {
            let wanted = self
                .shared
                .disc_name
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| APP_NAME.to_owned());
            if wanted != self.window_title {
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(wanted.clone()));
                self.window_title = wanted;
            }
        }

        if self.settings_open {
            let settings_size = if self.settings_tab == SettingsTab::Input {
                [760.0, 540.0]
            } else {
                [470.0, 580.0]
            };
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("settings"),
                egui::ViewportBuilder::default()
                    .with_title("Settings")
                    .with_inner_size(settings_size)
                    .with_min_inner_size([380.0, 320.0]),
                |ctx, class| {
                    if class == egui::ViewportClass::Embedded {
                        // Backend without multi-viewport support: fall back to
                        // the overlay window.
                        let mut open = true;
                        egui::Window::new("Settings")
                            .open(&mut open)
                            .resizable(true)
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

        let mut toggle_library = false;
        let mut save_player_screenshot = false;
        if !fullscreen {
            egui::TopBottomPanel::bottom("status")
                .resizable(false)
                .frame(chrome_frame())
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        // Always offer a way between the library and the player.
                        let label = if self.show_library {
                            "Player"
                        } else {
                            "Library"
                        };
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new(label).size(12.0).strong())
                                    .frame(false),
                            )
                            .clicked()
                        {
                            toggle_library = true;
                        }
                        ui.separator();
                        let status = self.shared.status.lock().unwrap().clone();
                        if !status.is_empty() {
                            ui.label(egui::RichText::new(status).color(UI_MUTED_TEXT).size(12.0));
                        }
                        if !self.show_library {
                            let disc_name = self.shared.disc_name.lock().unwrap().clone();
                            if let Some(code) = disc_name.as_deref().and_then(parental_passcode) {
                                egui::Frame::new()
                                    .fill(PARENTAL_PASSCODE_BACKGROUND)
                                    .corner_radius(egui::CornerRadius::same(6))
                                    .inner_margin(egui::Margin::symmetric(7, 2))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "Parental passcode  {code}"
                                            ))
                                            .color(PARENTAL_PASSCODE_COLOR)
                                            .size(11.0),
                                        );
                                    });
                            }
                        }
                        let viewing_photos =
                            self.photocd.as_ref().map(|p| p.view_photo).unwrap_or(false);
                        if self.mouse_captured {
                            ui.label(
                                egui::RichText::new("Esc releases the mouse")
                                    .color(UI_MUTED_TEXT)
                                    .size(11.0),
                            );
                        } else if self.capture_mouse_enabled
                            && !viewing_photos
                            && !self.show_library
                        {
                            ui.label(
                                egui::RichText::new("Click the screen to capture the mouse")
                                    .color(UI_MUTED_TEXT)
                                    .size(11.0),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Right-to-left: added first sits furthest right.
                            if self.show_fps {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:.0} fps",
                                        *self.shared.fps.lock().unwrap()
                                    ))
                                    .color(UI_MUTED_TEXT)
                                    .size(11.0),
                                );
                            }
                            ui.label(
                                egui::RichText::new(if self.shared.pal.load(Ordering::Relaxed) {
                                    "PAL"
                                } else {
                                    "NTSC"
                                })
                                .color(UI_MUTED_TEXT)
                                .size(11.0),
                            );
                            if self.shared.fast_forward.load(Ordering::Relaxed) {
                                ui.label(
                                    egui::RichText::new("▶▶")
                                        .color(egui::Color32::LIGHT_GRAY)
                                        .strong(),
                                );
                            }
                            if !self.show_library
                                && !viewing_photos
                                && ui
                                    .add(
                                        egui::Button::new(egui::RichText::new("⬇").size(16.0))
                                            .frame(false),
                                    )
                                    .on_hover_text("Save the displayed CD-i picture as PNG")
                                    .clicked()
                            {
                                save_player_screenshot = true;
                            }
                        });
                    });
                });
        }
        if save_player_screenshot {
            self.save_player_screenshot();
        }
        if toggle_library {
            self.show_library = !self.show_library;
            self.shared
                .audio_suppressed
                .store(self.show_library, Ordering::Relaxed);
            if self.show_library {
                self.scan_libraries();
                self.library_focus = self.library_tab;
                self.library_selection = 0;
                self.library_dpad_y = 0;
                self.pad_buttons = 0;
                self.pad_frac = egui::Vec2::ZERO;
                if self.mouse_captured {
                    self.set_mouse_capture(ctx, false);
                }
            }
        }

        // Photo CD control panel (stacks above the status bar).
        let mut photo_decode = false;
        let mut photo_save: Option<(String, cdi_photocd::decode::DecodedImage)> = None;
        let mut photo_entered_view = false;
        if let (false, Some(p)) = (fullscreen, &mut self.photocd) {
            egui::TopBottomPanel::bottom("photocd_panel")
                .resizable(false)
                .frame(chrome_frame())
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Photo CD").size(12.0).strong());
                        if p.has_cdi_app {
                            let view_label = if p.view_photo {
                                "Back to CD-i"
                            } else {
                                "View Raw Images"
                            };
                            if ui.button(view_label).clicked() {
                                p.view_photo = !p.view_photo;
                                if p.view_photo {
                                    photo_entered_view = true;
                                    if p.decoded.is_none() && !p.decoding {
                                        photo_decode = true;
                                    }
                                } else {
                                    p.slideshow = false;
                                }
                            }
                        } else {
                            p.view_photo = true;
                            ui.weak("No CD-i support on this disc");
                        }
                        if p.view_photo && !p.names.is_empty() {
                            ui.separator();
                            if ui.button("◀").clicked() {
                                p.current = (p.current + p.names.len() - 1) % p.names.len();
                                photo_decode = true;
                            }
                            if ui.button("▶").clicked() {
                                p.current = (p.current + 1) % p.names.len();
                                photo_decode = true;
                            }
                            ui.label(format!("{} / {}", p.current + 1, p.names.len()));
                            ui.separator();
                            let prev_tier = p.tier;
                            egui::ComboBox::from_id_salt("pcd_tier")
                                .selected_text(cdi_photocd::decode::TIER_LABELS[p.tier])
                                .show_ui(ui, |ui| {
                                    for tier in 0..=p.max_tier.min(2) {
                                        ui.selectable_value(
                                            &mut p.tier,
                                            tier,
                                            cdi_photocd::decode::TIER_LABELS[tier],
                                        );
                                    }
                                });
                            if p.tier != prev_tier {
                                photo_decode = true;
                            }
                            if p.names.len() > 1 {
                                let label = if p.slideshow { "■" } else { "▶" };
                                if ui.button(label).clicked() {
                                    p.slideshow = !p.slideshow;
                                    p.last_advance = Instant::now();
                                }
                            }
                            if p.decoded.is_some()
                                && ui
                                    .button(egui::RichText::new("⬇").size(16.0))
                                    .on_hover_text("Save photo as PNG")
                                    .clicked()
                            {
                                let stem = p.names[p.current]
                                    .rsplit_once('.')
                                    .map(|(s, _)| s.to_owned())
                                    .unwrap_or_else(|| p.names[p.current].clone());
                                let name = format!(
                                    "{stem}_{}.png",
                                    cdi_photocd::decode::TIER_LABELS[p.tier]
                                );
                                photo_save = Some((name, p.decoded.clone().unwrap()));
                            }
                            if p.decoding {
                                ui.spinner();
                                ui.weak("Decoding…");
                            } else if let Some(error) = &p.error {
                                ui.colored_label(egui::Color32::RED, error);
                            }
                        }
                    });
                });
        }
        if photo_entered_view && self.mouse_captured {
            self.set_mouse_capture(ctx, false);
        }
        if photo_decode {
            self.request_photo_decode();
        }
        if let Some((name, image)) = photo_save {
            let _ = save_rgb_png("Save photo as PNG", &name, self.save_dir.as_deref(), &image);
        }

        // Library browser replaces the central view while active; the
        // emulation keeps running underneath.
        if self.show_library {
            let controller_path = self.poll_library_gamepad();
            if !self.show_library {
                ctx.request_repaint();
                return;
            }
            let mouse_path = self.paint_library(ctx);
            if let Some(path) = controller_path.or(mouse_path) {
                self.load_disc_path(path);
            }
            return;
        }

        // Keep the picture area at its presentation aspect. The raw PAL and
        // NTSC rasters use non-square television pixels, while the live
        // MCD212 timing aperture can switch between 720/768 by 240/280 lines.
        if !fullscreen {
            let aspect = {
                let frame = self.shared.frame.lock().unwrap();
                let aperture = display_aperture(&frame, self.display_area);
                presentation_aspect(aperture, frame.geometry, self.crt_aspect)
            };
            let picture = ctx.available_rect();
            if let Some(inner) = ctx.input(|input| input.viewport().inner_rect) {
                if aspect > 0.0 && picture.width() > 0.0 {
                    let wanted = picture.width() / aspect;
                    let delta = wanted - picture.height();
                    // Ignore sub-pixel drift so this cannot oscillate.
                    if delta.abs() > 1.0 {
                        let max_h = ctx
                            .input(|input| input.viewport().monitor_size)
                            .map_or(f32::MAX, |size| size.y);
                        let height = (inner.height() + delta).min(max_h);
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                            inner.width(),
                            height,
                        )));
                    }
                }
            }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                // Photo view replaces the emulated picture while active; the
                // emulation itself keeps running untouched.
                if let Some(p) = &self.photocd {
                    if p.view_photo {
                        if let Some(texture) = &p.texture {
                            let avail = ui.available_size();
                            let tex_size = texture.size_vec2();
                            let scale = (avail.x / tex_size.x).min(avail.y / tex_size.y).max(0.05);
                            let size = tex_size * scale;
                            let rect = egui::Rect::from_center_size(ui.max_rect().center(), size);
                            ui.painter().image(
                                texture.id(),
                                rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        }
                        return;
                    }
                }
                let Some(texture) = &self.texture else {
                    return;
                };
                let avail = ui.available_size();
                let (uv_rect, aspect, pointer_origin, pointer_extent) = {
                    let frame = self.shared.frame.lock().unwrap();
                    let aperture = display_aperture(&frame, self.display_area);
                    let uv_min = egui::pos2(
                        aperture.left as f32 / frame.width as f32,
                        aperture.top as f32 / frame.height as f32,
                    );
                    let uv_max = egui::pos2(
                        (aperture.left + aperture.width) as f32 / frame.width as f32,
                        (aperture.top + aperture.height) as f32 / frame.height as f32,
                    );
                    let (pointer_origin, pointer_extent) =
                        pointer_mapping(aperture, frame.geometry);
                    (
                        egui::Rect::from_min_max(uv_min, uv_max),
                        presentation_aspect(aperture, frame.geometry, self.crt_aspect),
                        pointer_origin,
                        pointer_extent,
                    )
                };
                let size = fit_aspect(avail, aspect);
                let rect = egui::Rect::from_center_size(ui.max_rect().center(), size);
                let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                ui.painter()
                    .image(texture.id(), rect, uv_rect, egui::Color32::WHITE);
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
                    let x_scale = pointer_extent.x / size.x;
                    let y_scale = pointer_extent.y / size.y;
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
                    input.buttons = self.game_buttons | self.pad_buttons | self.kb_buttons;
                } else if !self.capture_mouse_enabled
                    && ctx.input(|input| input.pointer.delta() != egui::Vec2::ZERO)
                {
                    if let Some(pos) = response.hover_pos() {
                        // Uncaptured mode maps the system pointer onto the active
                        // picture. Only actual pointer motion updates coordinates:
                        // video-mode/layout changes under a stationary cursor are
                        // not CD-i mouse movement.
                        let uv = (pos - rect.min) / size;
                        let x =
                            (pointer_origin.x + uv.x * pointer_extent.x).clamp(0.0, 767.0) as i32;
                        let y =
                            (pointer_origin.y + uv.y * pointer_extent.y).clamp(0.0, 559.0) as i32;
                        let mut input = self.shared.input.lock().unwrap();
                        input.x = x;
                        input.y = y;
                        input.buttons = self.game_buttons | self.pad_buttons | self.kb_buttons;
                    }
                }
            });

        self.paint_quick_menu(ctx);

        // Hold-to-fast-forward. Ignored while a text field or a pending
        // rebind wants the keyboard.
        let fast_forward = !ctx.wants_keyboard_input()
            && !self.ff_rebind
            && ctx.input(|i| i.key_down(self.ff_key));
        self.shared
            .fast_forward
            .store(fast_forward, Ordering::Relaxed);

        if !self.quick_menu_open {
            self.poll_keyboard(ctx);
        } else {
            self.kb_buttons = 0;
            self.kb_frac = egui::Vec2::ZERO;
        }
        self.poll_gamepad(ctx);

        ctx.request_repaint_after(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        backup_nvram, configured_nvram_path, controller_deflection, controller_dpad_deflection,
        controller_stick_deflection, cycle_library_focus, display_aperture, fill_audio, fit_aspect,
        library_dpad_direction, library_pad_action, load_nvram, parental_passcode, pointer_mapping,
        presentation_aspect, region_is_pal, screenshot_dimensions, screenshot_image, write_nvram,
        DiscOverride, DisplayAperture, DisplayArea, LibraryPadAction, SharedFrame,
        LIBRARY_OPEN_FOCUS,
    };
    use cdi_core::mcd212::DisplayGeometry;

    #[test]
    fn nvram_write_is_replaceable_and_wrong_sizes_start_clean() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mono1.nvr");
        write_nvram(Some(&path), &[1, 2, 3, 4]).unwrap();
        assert_eq!(load_nvram(Some(&path), 4), [1, 2, 3, 4]);
        write_nvram(Some(&path), &[5, 6, 7, 8]).unwrap();
        assert_eq!(load_nvram(Some(&path), 4), [5, 6, 7, 8]);
        assert_eq!(load_nvram(Some(&path), 8), [0; 8]);
    }

    #[test]
    fn ephemeral_nvram_has_no_host_path() {
        assert_eq!(configured_nvram_path("mono1", true, true, false), None);
    }

    #[test]
    fn nvram_reset_backup_preserves_nonempty_storage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mono1.nvr");
        let data = [0xDE, 0xAD, 0xC0, 0xDE];
        let backup = backup_nvram(Some(&path), &data).unwrap().unwrap();
        assert_eq!(std::fs::read(backup).unwrap(), data);
        assert_eq!(backup_nvram(Some(&path), &[0; 4]).unwrap(), None);
    }

    #[test]
    fn suppressed_audio_is_silent_and_drains_queued_samples() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::<i16>::new(8);
        for sample in [100, -100, 200, -200] {
            producer.push(sample).unwrap();
        }
        let mut output = [1i16; 4];
        fill_audio(&mut output, 2, &mut consumer, true, |sample| sample);
        assert_eq!(output, [0; 4]);
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn known_parental_passcode_matches_dump_style_disc_names() {
        assert_eq!(parental_passcode("Voyeur (Europe).cue"), Some("3333"));
        assert_eq!(parental_passcode("voyeur (USA) (Disc 1).CUE"), Some("3333"));
        assert_eq!(parental_passcode("Vegas Girls (Europe).cue"), Some("1234"));
        assert_eq!(
            parental_passcode("Loving for a Lifetime (USA).cue"),
            Some("6969")
        );
    }

    #[test]
    fn pending_and_unrelated_parental_passcodes_stay_hidden() {
        assert_eq!(
            parental_passcode("Pleasures of Sex, The (Europe).cue"),
            None
        );
        assert_eq!(parental_passcode("Voyeur II (Europe).cue"), None);
    }

    #[test]
    fn controller_dpad_preserves_diagonals_and_overrides_stick_drift() {
        let diagonal =
            controller_deflection(egui::vec2(-0.3, -0.2), egui::vec2(1.0, 1.0), true, true);
        assert_eq!(diagonal, egui::vec2(1.0, 1.0));
    }

    #[test]
    fn controller_uses_left_stick_while_dpad_is_idle() {
        let stick = controller_deflection(egui::vec2(0.75, -0.5), egui::Vec2::ZERO, false, true);
        assert_eq!(stick, egui::vec2(0.75, -0.5));
    }

    #[test]
    fn controller_can_disable_left_stick() {
        let stopped = controller_deflection(egui::vec2(0.75, -0.5), egui::Vec2::ZERO, false, false);
        assert_eq!(stopped, egui::Vec2::ZERO);
    }

    #[test]
    fn opposing_dpad_directions_do_not_fall_through_to_stick() {
        let stopped = controller_deflection(egui::vec2(0.8, 0.6), egui::Vec2::ZERO, true, true);
        assert_eq!(stopped, egui::Vec2::ZERO);
    }

    #[test]
    fn controller_reads_dpad_hat_axes_in_screen_coordinates() {
        let (up_right, active) = controller_dpad_deflection(1.0, 1.0, false, false, false, false);
        assert!(active);
        assert_eq!(up_right, egui::vec2(1.0, -1.0));

        let (down_left, active) =
            controller_dpad_deflection(-1.0, -1.0, false, false, false, false);
        assert!(active);
        assert_eq!(down_left, egui::vec2(-1.0, 1.0));
    }

    #[test]
    fn controller_dpad_buttons_override_hat_axis_per_axis() {
        let (opposed, active) = controller_dpad_deflection(1.0, 0.0, true, true, false, false);
        assert!(active);
        assert_eq!(opposed, egui::Vec2::ZERO);
    }

    #[test]
    fn library_triggers_cycle_all_tabs_and_open_cue() {
        assert_eq!(cycle_library_focus(0, false), LIBRARY_OPEN_FOCUS);
        assert_eq!(cycle_library_focus(LIBRARY_OPEN_FOCUS, true), 0);
        assert_eq!(
            library_pad_action(
                gilrs::Button::LeftTrigger2,
                gilrs::Button::South,
                gilrs::Button::East,
            ),
            Some(LibraryPadAction::PreviousTab)
        );
        assert_eq!(
            library_pad_action(
                gilrs::Button::RightTrigger,
                gilrs::Button::South,
                gilrs::Button::East,
            ),
            Some(LibraryPadAction::NextTab)
        );
    }

    #[test]
    fn library_dpad_and_face_buttons_have_edge_triggered_actions() {
        assert_eq!(
            library_pad_action(
                gilrs::Button::DPadUp,
                gilrs::Button::South,
                gilrs::Button::East,
            ),
            Some(LibraryPadAction::PreviousDisc)
        );
        assert_eq!(
            library_pad_action(
                gilrs::Button::DPadDown,
                gilrs::Button::South,
                gilrs::Button::East,
            ),
            Some(LibraryPadAction::NextDisc)
        );
        assert_eq!(
            library_pad_action(
                gilrs::Button::South,
                gilrs::Button::South,
                gilrs::Button::East,
            ),
            Some(LibraryPadAction::Activate)
        );
        assert_eq!(
            library_pad_action(
                gilrs::Button::East,
                gilrs::Button::South,
                gilrs::Button::East,
            ),
            Some(LibraryPadAction::Back)
        );
    }

    #[test]
    fn library_accepts_dpad_hat_axes_and_mapped_buttons() {
        assert_eq!(library_dpad_direction(1.0, false, false), -1);
        assert_eq!(library_dpad_direction(-1.0, false, false), 1);
        assert_eq!(library_dpad_direction(0.0, true, false), -1);
        assert_eq!(library_dpad_direction(0.0, false, true), 1);
        assert_eq!(library_dpad_direction(0.0, true, true), 0);
        assert_eq!(library_dpad_direction(0.2, false, false), 0);
    }

    #[test]
    fn controller_stick_deadzone_has_no_step_discontinuity() {
        assert_eq!(
            controller_stick_deflection(egui::vec2(0.15, 0.0), 0.15),
            egui::Vec2::ZERO
        );
        let just_outside = controller_stick_deflection(egui::vec2(0.16, 0.0), 0.15);
        assert!(just_outside.x > 0.0);
        assert!(just_outside.x < 0.02);
        assert_eq!(just_outside.y, 0.0);
    }

    #[test]
    fn controller_stick_deadzone_preserves_direction_and_full_scale() {
        let diagonal = controller_stick_deflection(egui::vec2(0.6, 0.8), 0.2);
        assert!((diagonal.length() - 1.0).abs() < f32::EPSILON);
        assert!((diagonal.x - 0.6).abs() < f32::EPSILON);
        assert!((diagonal.y - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn region_tag_selects_video_standard() {
        assert_eq!(region_is_pal("Windsurfing (USA)"), Some(false));
        assert_eq!(region_is_pal("Ghost in the Shell (Japan)"), Some(false));
        assert_eq!(region_is_pal("Akt Aesthetik (Germany)"), Some(true));
        assert_eq!(
            region_is_pal("Vincent van Gogh Vol. 1, The (Europe)"),
            Some(true)
        );
        assert_eq!(region_is_pal("Nobelia (USA, Europe)"), None);
    }

    #[test]
    fn later_tags_and_untagged_names_are_handled() {
        // A revision tag after the region must not confuse the scan.
        assert_eq!(region_is_pal("Alien Gate (Europe) (Rev 2)"), Some(true));
        // No region tag: leave the current standard alone.
        assert_eq!(region_is_pal("Some Untagged Disc"), None);
        // Parentheses that are not regions.
        assert_eq!(region_is_pal("Title (Demo)"), None);
    }

    #[test]
    fn philips_player_pixel_aspect_controls_presentation() {
        let ntsc = DisplayGeometry {
            raster_width: 768,
            raster_height: 480,
            active_x: 0,
            active_y: 0,
            active_width: 768,
            active_height: 480,
            compatibility_mode: false,
            interlaced: false,
            odd_field: false,
            frame_duration_60hz: true,
            pixel_aspect_num: 49,
            pixel_aspect_den: 40,
        };
        let full = DisplayAperture {
            left: 0,
            top: 0,
            width: 768,
            height: 480,
        };
        let typical = DisplayAperture {
            left: 24,
            top: 20,
            width: 720,
            height: 440,
        };
        assert!((presentation_aspect(full, ntsc, true) - 1.306_122_4).abs() < 0.000_001);
        assert!((presentation_aspect(typical, ntsc, true) - 1.335_807).abs() < 0.000_001);

        let pal = DisplayGeometry {
            raster_height: 560,
            active_height: 560,
            frame_duration_60hz: false,
            pixel_aspect_num: 41,
            ..ntsc
        };
        let pal_aperture = DisplayAperture {
            height: 560,
            ..full
        };
        assert!((presentation_aspect(pal_aperture, pal, true) - 1.337_979_1).abs() < 0.000_001);

        let size = fit_aspect(egui::vec2(720.0, 700.0), 720.0 / 539.0);
        assert_eq!(size, egui::vec2(720.0, 539.0));
    }

    #[test]
    fn raw_presentation_preserves_active_pixel_geometry() {
        let geometry = DisplayGeometry {
            raster_width: 768,
            raster_height: 480,
            active_x: 0,
            active_y: 0,
            active_width: 768,
            active_height: 480,
            compatibility_mode: false,
            interlaced: false,
            odd_field: false,
            frame_duration_60hz: true,
            pixel_aspect_num: 49,
            pixel_aspect_den: 40,
        };
        assert_eq!(
            presentation_aspect(
                DisplayAperture {
                    left: 0,
                    top: 0,
                    width: 768,
                    height: 480,
                },
                geometry,
                false,
            ),
            1.6
        );
    }

    #[test]
    fn player_screenshot_uses_the_displayed_aperture_and_aspect() {
        let ntsc = DisplayGeometry {
            raster_width: 768,
            raster_height: 480,
            active_x: 0,
            active_y: 0,
            active_width: 768,
            active_height: 480,
            compatibility_mode: false,
            interlaced: false,
            odd_field: false,
            frame_duration_60hz: true,
            pixel_aspect_num: 49,
            pixel_aspect_den: 40,
        };
        assert_eq!(
            screenshot_dimensions(
                DisplayAperture {
                    left: 24,
                    top: 20,
                    width: 720,
                    height: 440,
                },
                ntsc,
                true,
            ),
            (720, 539)
        );
        assert_eq!(
            screenshot_dimensions(
                DisplayAperture {
                    left: 0,
                    top: 0,
                    width: 768,
                    height: 480,
                },
                ntsc,
                true,
            ),
            (768, 588)
        );

        let mut pixels = vec![0x0010_1010; 16];
        for y in 1..3 {
            for x in 1..3 {
                pixels[y * 4 + x] = 0x00EB_EBEB;
            }
        }
        let frame = SharedFrame {
            pixels,
            width: 4,
            height: 4,
            geometry: DisplayGeometry {
                raster_width: 4,
                raster_height: 4,
                active_x: 1,
                active_y: 1,
                active_width: 2,
                active_height: 2,
                compatibility_mode: true,
                interlaced: false,
                odd_field: false,
                frame_duration_60hz: false,
                pixel_aspect_num: 1,
                pixel_aspect_den: 1,
            },
            frame_no: 0,
        };
        let image = screenshot_image(&frame, DisplayArea::FullSignal, false);
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(image.rgb, vec![255; 12]);
    }

    #[test]
    fn hardware_aperture_is_independent_of_frame_contents() {
        let geometry = DisplayGeometry {
            raster_width: 768,
            raster_height: 560,
            active_x: 24,
            active_y: 40,
            active_width: 720,
            active_height: 480,
            compatibility_mode: true,
            interlaced: true,
            odd_field: true,
            frame_duration_60hz: false,
            pixel_aspect_num: 41,
            pixel_aspect_den: 40,
        };
        let black = SharedFrame {
            pixels: vec![0; 768 * 560],
            width: 768,
            height: 560,
            geometry,
            frame_no: 0,
        };
        let patterned = SharedFrame {
            pixels: vec![0x00FF_00FF; 768 * 560],
            width: 768,
            height: 560,
            geometry,
            frame_no: 0,
        };
        let aperture = display_aperture(&patterned, DisplayArea::TypicalCrt);
        assert_eq!(
            (aperture.left, aperture.top, aperture.width, aperture.height),
            (24, 40, 720, 480)
        );
        assert_eq!(display_aperture(&black, DisplayArea::TypicalCrt), aperture);
    }

    #[test]
    fn mouse_endpoints_match_the_displayed_hardware_aperture() {
        let geometry = DisplayGeometry {
            raster_width: 768,
            raster_height: 560,
            active_x: 24,
            active_y: 40,
            active_width: 720,
            active_height: 480,
            compatibility_mode: true,
            interlaced: false,
            odd_field: false,
            frame_duration_60hz: false,
            pixel_aspect_num: 41,
            pixel_aspect_den: 40,
        };
        let aperture = DisplayAperture {
            left: geometry.active_x,
            top: geometry.active_y,
            width: geometry.active_width,
            height: geometry.active_height,
        };
        let (origin, extent) = pointer_mapping(aperture, geometry);
        assert_eq!(origin, egui::Pos2::ZERO);
        assert_eq!(extent, egui::vec2(768.0, 560.0));
    }

    #[test]
    fn ntsc_crt_framing_centers_windowboxes_and_fills_bottom_aligned_pictures() {
        let geometry = DisplayGeometry {
            raster_width: 768,
            raster_height: 480,
            active_x: 0,
            active_y: 0,
            active_width: 768,
            active_height: 480,
            compatibility_mode: false,
            interlaced: false,
            odd_field: false,
            frame_duration_60hz: true,
            pixel_aspect_num: 49,
            pixel_aspect_den: 40,
        };
        let bottom_aligned = SharedFrame {
            pixels: vec![0x0012_3456; 768 * 480],
            width: 768,
            height: 480,
            geometry,
            frame_no: 0,
        };
        let black = SharedFrame {
            pixels: vec![0; 768 * 480],
            width: 768,
            height: 480,
            geometry,
            frame_no: 0,
        };
        let centered = display_aperture(&black, DisplayArea::TypicalCrt);
        assert_eq!(
            (centered.left, centered.top, centered.width, centered.height),
            (24, 20, 720, 440)
        );
        let filled = display_aperture(&bottom_aligned, DisplayArea::TypicalCrt);
        assert_eq!(
            (filled.left, filled.top, filled.width, filled.height),
            (24, 40, 720, 440)
        );
        assert_eq!(
            display_aperture(&black, DisplayArea::FullSignal),
            DisplayAperture {
                left: 0,
                top: 0,
                width: 768,
                height: 480,
            }
        );

        let (origin, extent) = pointer_mapping(filled, geometry);
        assert_eq!(origin, egui::pos2(24.0, 560.0 / 12.0));
        assert_eq!(extent, egui::vec2(720.0, 560.0 * 11.0 / 12.0));
    }

    #[test]
    fn obsolete_fill_preference_is_ignored() {
        let legacy: DiscOverride =
            serde_json::from_str(r#"{"fill_compatibility_frame":true}"#).unwrap();
        assert_eq!(legacy, DiscOverride::default());
    }
}
