// SPDX-License-Identifier: GPL-3.0-or-later
//! Disc-library discovery, Store-ZIP resolution, and controller navigation.
//!
//! This module deliberately owns no emulator state. It turns persisted folder
//! preferences and UI/controller actions into paths or host-view effects so
//! library changes cannot silently alter machine behavior.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::store_zip;

/// Disc library slots, in UI order. CD-BGM is a CD-i-based background-music
/// format the player handles like an ordinary CD-i disc.
pub(crate) const SLOTS: [&str; 4] = ["Philips CD-i", "Photo CD", "Video CD", "CD-BGM"];
const FOCUS_COUNT: usize = SLOTS.len() + 1;
pub(crate) const OPEN_FOCUS: usize = SLOTS.len();
pub(crate) const REPEAT_DELAY: Duration = Duration::from_millis(300);
pub(crate) const REPEAT_INTERVAL: Duration = Duration::from_millis(75);
const TRIGGER_PRESS_THRESHOLD: f32 = 0.60;
const TRIGGER_RELEASE_THRESHOLD: f32 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PadAction {
    PreviousTab,
    NextTab,
    PreviousDisc,
    NextDisc,
    Activate,
    Back,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    None,
    Load(PathBuf),
    OpenDisc,
    Close,
}

pub(crate) fn pad_action(
    button: gilrs::Button,
    primary: gilrs::Button,
    secondary: gilrs::Button,
) -> Option<PadAction> {
    use gilrs::Button;
    match button {
        Button::LeftTrigger => Some(PadAction::PreviousTab),
        Button::RightTrigger => Some(PadAction::NextTab),
        Button::DPadUp => Some(PadAction::PreviousDisc),
        Button::DPadDown => Some(PadAction::NextDisc),
        Button::South => Some(PadAction::Activate),
        Button::East => Some(PadAction::Back),
        other if other == primary => Some(PadAction::Activate),
        other if other == secondary => Some(PadAction::Back),
        _ => None,
    }
}

/// Convert analog trigger travel into one navigation edge per deliberate
/// pull. The separate release threshold prevents noisy values around the
/// activation point from switching multiple tabs.
#[derive(Default)]
struct TriggerLatch {
    left_pressed: bool,
    right_pressed: bool,
}

impl TriggerLatch {
    fn update(&mut self, left: f32, right: f32) -> [Option<PadAction>; 2] {
        [
            Self::edge(&mut self.left_pressed, left, PadAction::PreviousTab),
            Self::edge(&mut self.right_pressed, right, PadAction::NextTab),
        ]
    }

    fn edge(pressed: &mut bool, value: f32, action: PadAction) -> Option<PadAction> {
        if *pressed {
            if value <= TRIGGER_RELEASE_THRESHOLD {
                *pressed = false;
            }
            None
        } else if value >= TRIGGER_PRESS_THRESHOLD {
            *pressed = true;
            Some(action)
        } else {
            None
        }
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Normalize either a mapped D-pad button or an unmapped hat Y axis into a
/// vertical Library direction. Gilrs reports up as positive Y.
pub(crate) fn dpad_direction(axis_y: f32, up_button: bool, down_button: bool) -> i8 {
    let up = up_button || axis_y > 0.5;
    let down = down_button || axis_y < -0.5;
    match (up, down) {
        (true, false) => -1,
        (false, true) => 1,
        _ => 0,
    }
}

/// Edge-trigger a new direction, then repeat it at a bounded menu-navigation
/// cadence while held. Restarting from the current poll avoids catch-up bursts
/// after a blocking file dialog.
#[derive(Default)]
pub(crate) struct DpadRepeat {
    direction: i8,
    next_repeat: Option<Instant>,
}

impl DpadRepeat {
    pub(crate) fn update(&mut self, direction: i8, now: Instant) -> i8 {
        if direction == 0 {
            self.reset();
            return 0;
        }
        if direction != self.direction {
            self.direction = direction;
            self.next_repeat = Some(now + REPEAT_DELAY);
            return direction;
        }
        if self.next_repeat.is_some_and(|deadline| now >= deadline) {
            self.next_repeat = Some(now + REPEAT_INTERVAL);
            return direction;
        }
        0
    }

    fn reset(&mut self) {
        self.direction = 0;
        self.next_repeat = None;
    }
}

/// One disc found while scanning the configured library folders.
pub(crate) struct Entry {
    title: String,
    category: usize,
    cue: PathBuf,
}

impl Entry {
    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn cue(&self) -> &Path {
        &self.cue
    }
}

/// Host-only library state shared by mouse rendering and controller input.
pub(crate) struct LibraryModel {
    folders: [Option<String>; SLOTS.len()],
    entries: Vec<Entry>,
    tab: usize,
    focus: usize,
    selection: usize,
    dpad_repeat: DpadRepeat,
    trigger_latch: TriggerLatch,
    scroll_to_top: bool,
    scroll_to_selection: bool,
    strip_width: f32,
}

impl LibraryModel {
    pub(crate) fn new(saved_folders: &[Option<String>]) -> Self {
        let mut folders: [Option<String>; SLOTS.len()] = Default::default();
        for (slot, value) in saved_folders.iter().take(SLOTS.len()).enumerate() {
            folders[slot] = value.clone();
        }
        Self {
            folders,
            entries: Vec::new(),
            tab: 0,
            focus: 0,
            selection: 0,
            dpad_repeat: DpadRepeat::default(),
            trigger_latch: TriggerLatch::default(),
            scroll_to_top: true,
            scroll_to_selection: false,
            strip_width: 0.0,
        }
    }

    pub(crate) fn has_configured_folder(&self) -> bool {
        self.folders.iter().any(Option::is_some)
    }

    pub(crate) fn folders(&self) -> &[Option<String>; SLOTS.len()] {
        &self.folders
    }

    pub(crate) fn folders_vec(&self) -> Vec<Option<String>> {
        self.folders.to_vec()
    }

    pub(crate) fn folder(&self, slot: usize) -> Option<&str> {
        self.folders[slot].as_deref()
    }

    pub(crate) fn set_folder(&mut self, slot: usize, folder: Option<String>) {
        self.folders[slot] = folder;
    }

    pub(crate) fn tab(&self) -> usize {
        self.tab
    }

    pub(crate) fn focus(&self) -> usize {
        self.focus
    }

    /// Whether a format tab owns controller focus and should receive selected
    /// styling. The displayed category remains unchanged while Open `.cue`
    /// owns focus, but must not look simultaneously selected.
    pub(crate) fn tab_is_highlighted(&self, slot: usize) -> bool {
        self.tab == slot && self.focus == slot
    }

    pub(crate) fn selection(&self) -> usize {
        self.selection
    }

    pub(crate) fn strip_width(&self) -> f32 {
        self.strip_width
    }

    pub(crate) fn set_strip_width(&mut self, width: f32) {
        self.strip_width = width;
    }

    pub(crate) fn counts(&self) -> [usize; SLOTS.len()] {
        let mut counts = [0; SLOTS.len()];
        for entry in &self.entries {
            counts[entry.category] += 1;
        }
        counts
    }

    pub(crate) fn entries(&self, category: usize) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(move |entry| entry.category == category)
    }

    pub(crate) fn select_tab(&mut self, slot: usize) {
        self.tab = slot;
        self.focus = slot;
        self.selection = 0;
        self.scroll_to_top = true;
        self.scroll_to_selection = false;
    }

    pub(crate) fn focus_open(&mut self) {
        self.focus = OPEN_FOCUS;
    }

    pub(crate) fn select_row(&mut self, category: usize, row: usize) {
        self.focus = category;
        self.selection = row;
    }

    pub(crate) fn take_scroll_requests(&mut self) -> (bool, bool) {
        (
            std::mem::take(&mut self.scroll_to_top),
            std::mem::take(&mut self.scroll_to_selection),
        )
    }

    pub(crate) fn enter(&mut self) {
        self.scan();
        self.focus = self.tab;
        self.selection = 0;
        self.dpad_repeat.reset();
        self.trigger_latch.reset();
        self.scroll_to_top = true;
        self.scroll_to_selection = false;
    }

    pub(crate) fn repeated_action(&mut self, direction: i8, now: Instant) -> Option<PadAction> {
        match self.dpad_repeat.update(direction, now) {
            value if value < 0 => Some(PadAction::PreviousDisc),
            value if value > 0 => Some(PadAction::NextDisc),
            _ => None,
        }
    }

    pub(crate) fn analog_trigger_actions(
        &mut self,
        left: f32,
        right: f32,
    ) -> [Option<PadAction>; 2] {
        self.trigger_latch.update(left, right)
    }

    pub(crate) fn apply(&mut self, action: PadAction) -> Effect {
        match action {
            PadAction::PreviousTab | PadAction::NextTab => {
                self.focus = cycle_focus(self.focus, action == PadAction::NextTab);
                self.selection = 0;
                self.scroll_to_top = true;
                self.scroll_to_selection = false;
                if self.focus < SLOTS.len() {
                    self.tab = self.focus;
                }
                Effect::None
            }
            PadAction::PreviousDisc | PadAction::NextDisc => {
                if self.focus >= SLOTS.len() {
                    return Effect::None;
                }
                let count = self.entries(self.tab).count();
                if count == 0 {
                    self.selection = 0;
                } else if action == PadAction::NextDisc {
                    self.selection = (self.selection + 1) % count;
                } else {
                    self.selection = (self.selection + count - 1) % count;
                }
                self.scroll_to_selection = true;
                Effect::None
            }
            PadAction::Activate => {
                if self.focus == OPEN_FOCUS {
                    Effect::OpenDisc
                } else {
                    self.entries(self.tab)
                        .nth(self.selection)
                        .map_or(Effect::None, |entry| Effect::Load(entry.cue.clone()))
                }
            }
            PadAction::Back => Effect::Close,
        }
    }

    /// Rebuild the list from the configured folders. A folder may contain
    /// loose CUE/eligible Store-ZIP files or per-disc subdirectories.
    pub(crate) fn scan(&mut self) {
        self.entries.clear();
        for (category, dir) in self.folders.iter().enumerate() {
            let Some(dir) = dir.as_ref().map(PathBuf::from).filter(|path| path.is_dir()) else {
                continue;
            };
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut paths: Vec<PathBuf> = entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .collect();
            paths.sort_by_key(|path| path.file_name().map(|name| name.to_ascii_lowercase()));
            for path in paths {
                let cue = disc_path_in(&path);
                if let Some(cue) = cue {
                    let title = if path.is_dir() { &path } else { &cue }
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_else(|| cue.display().to_string());
                    self.entries.push(Entry {
                        title,
                        category,
                        cue,
                    });
                }
            }
        }

        // Pick a populated initial format once per scan; explicit clicks onto
        // an empty tab remain stable until the next scan.
        if !self.entries.iter().any(|entry| entry.category == self.tab) {
            if let Some(entry) = self.entries.first() {
                self.tab = entry.category;
                self.focus = entry.category;
            }
        }
        let visible_count = self.entries(self.tab).count();
        self.selection = self.selection.min(visible_count.saturating_sub(1));
    }
}

fn cycle_focus(current: usize, forward: bool) -> usize {
    if forward {
        (current + 1) % FOCUS_COUNT
    } else {
        (current + FOCUS_COUNT - 1) % FOCUS_COUNT
    }
}

fn disc_path_in(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        let inner = std::fs::read_dir(path).ok()?;
        let mut candidates: Vec<PathBuf> = inner
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|candidate| {
                candidate.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("cue") || extension.eq_ignore_ascii_case("zip")
                })
            })
            .collect();
        candidates.sort_by_key(|candidate| {
            (
                is_zip_path(candidate),
                candidate.file_name().map(|name| name.to_ascii_lowercase()),
            )
        });
        candidates
            .into_iter()
            .find(|candidate| !is_zip_path(candidate) || store_zip::is_eligible(candidate))
    } else if path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("cue")
            || (extension.eq_ignore_ascii_case("zip") && store_zip::is_eligible(path))
    }) {
        Some(path.to_owned())
    } else {
        None
    }
}

pub(crate) fn is_zip_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

pub(crate) fn resolve_disc_source(
    path: &Path,
) -> Result<(PathBuf, Option<Arc<tempfile::TempDir>>), String> {
    if !is_zip_path(path) {
        return Ok((path.to_owned(), None));
    }
    let extracted = store_zip::extract(path)?;
    let guard = Arc::new(extracted.temp_dir);
    Ok((extracted.cue_path, Some(guard)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_cycles_across_tabs_and_open_action() {
        assert_eq!(cycle_focus(0, false), OPEN_FOCUS);
        assert_eq!(cycle_focus(OPEN_FOCUS, true), 0);

        let mut model = LibraryModel::new(&[]);
        model.select_tab(SLOTS.len() - 1);
        assert!(model.tab_is_highlighted(SLOTS.len() - 1));
        model.focus_open();
        assert_eq!(model.tab(), SLOTS.len() - 1);
        assert!(!model.tab_is_highlighted(SLOTS.len() - 1));
        assert_eq!(model.focus(), OPEN_FOCUS);
    }

    #[test]
    fn repeat_edges_are_immediate_and_hold_is_bounded() {
        let start = Instant::now();
        let mut repeat = DpadRepeat::default();
        assert_eq!(repeat.update(1, start), 1);
        assert_eq!(repeat.update(1, start + REPEAT_DELAY / 2), 0);
        assert_eq!(repeat.update(1, start + REPEAT_DELAY), 1);
        assert_eq!(
            repeat.update(1, start + REPEAT_DELAY + REPEAT_INTERVAL / 2),
            0
        );
        assert_eq!(repeat.update(1, start + REPEAT_DELAY + REPEAT_INTERVAL), 1);
        assert_eq!(repeat.update(1, start + Duration::from_secs(5)), 1);
        assert_eq!(
            repeat.update(1, start + Duration::from_secs(5) + REPEAT_INTERVAL / 2),
            0
        );
        assert_eq!(repeat.update(-1, start + Duration::from_secs(6)), -1);
        assert_eq!(repeat.update(0, start + Duration::from_secs(7)), 0);
        assert_eq!(repeat.update(1, start + Duration::from_secs(8)), 1);
    }

    #[test]
    fn analog_triggers_require_deliberate_travel_and_release() {
        let mut latch = TriggerLatch::default();
        assert_eq!(latch.update(0.10, 0.59), [None, None]);
        assert_eq!(
            latch.update(0.60, 0.80),
            [Some(PadAction::PreviousTab), Some(PadAction::NextTab)]
        );
        assert_eq!(latch.update(0.90, 0.90), [None, None]);
        assert_eq!(latch.update(0.50, 0.50), [None, None]);
        assert_eq!(latch.update(0.35, 0.20), [None, None]);
        assert_eq!(
            latch.update(0.75, 0.70),
            [Some(PadAction::PreviousTab), Some(PadAction::NextTab)]
        );
    }

    #[test]
    fn model_scans_loose_and_nested_cues_in_stable_order() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("Beta.cue"), b"").unwrap();
        let nested = root.path().join("Alpha");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("disc.cue"), b"").unwrap();
        std::fs::write(root.path().join("ignored.bin"), b"").unwrap();

        let mut model = LibraryModel::new(&[Some(root.path().display().to_string())]);
        model.scan();
        let titles: Vec<_> = model.entries(0).map(Entry::title).collect();
        assert_eq!(titles, ["Alpha", "Beta"]);
        assert_eq!(model.counts(), [2, 0, 0, 0]);
    }

    #[test]
    fn controller_selection_wraps_and_activation_returns_path() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("Alpha.cue");
        let second = root.path().join("Beta.cue");
        std::fs::write(&first, b"").unwrap();
        std::fs::write(&second, b"").unwrap();
        let mut model = LibraryModel::new(&[Some(root.path().display().to_string())]);
        model.scan();

        assert_eq!(model.apply(PadAction::PreviousDisc), Effect::None);
        assert_eq!(model.selection(), 1);
        assert_eq!(model.apply(PadAction::Activate), Effect::Load(second));
        assert_eq!(model.apply(PadAction::NextDisc), Effect::None);
        assert_eq!(model.selection(), 0);
        assert_eq!(model.apply(PadAction::Activate), Effect::Load(first));
    }
}
