// SPDX-License-Identifier: GPL-3.0-or-later
//! Host presentation geometry shared by rendering, screenshots, and pointer
//! mapping.

use cdi_core::mcd212::{presentation_rgb, DisplayGeometry};

use super::SharedFrame;

/// Per-channel deltas which distinguish moving field content from small
/// decoder/rounding noise. Once motion starts, a lower continuation threshold
/// and short hold prevent fades and thin edges from chattering between weave
/// and reconstruction on consecutive fields.
const MOTION_START_THRESHOLD: u8 = 8;
const MOTION_CONTINUE_THRESHOLD: u8 = 3;
const MOTION_HOLD_FIELDS: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DisplayAperture {
    pub(super) left: usize,
    pub(super) top: usize,
    pub(super) width: usize,
    pub(super) height: usize,
}

/// Return whether two fields use the same raster layout. Field parity is
/// intentionally excluded: it alternates every field in a stable mode.
pub(super) fn same_field_layout(a: DisplayGeometry, b: DisplayGeometry) -> bool {
    a.raster_width == b.raster_width
        && a.raster_height == b.raster_height
        && a.active_x == b.active_x
        && a.active_y == b.active_y
        && a.active_width == b.active_width
        && a.active_height == b.active_height
        && a.compatibility_mode == b.compatibility_mode
        && a.interlaced == b.interlaced
        && a.frame_duration_60hz == b.frame_duration_60hz
}

fn component_delta(a: u32, b: u32, shift: u32) -> u8 {
    let a = ((a >> shift) & 0xff) as u8;
    let b = ((b >> shift) & 0xff) as u8;
    a.abs_diff(b)
}

fn pixel_changed(a: u32, b: u32, threshold: u8) -> bool {
    component_delta(a, b, 16) >= threshold
        || component_delta(a, b, 8) >= threshold
        || component_delta(a, b, 0) >= threshold
}

fn average_pixel(a: u32, b: u32) -> u32 {
    let rb = ((a & 0x00ff_00ff) + (b & 0x00ff_00ff) + 0x0001_0001) >> 1;
    let g = ((a & 0x0000_ff00) + (b & 0x0000_ff00) + 0x0000_0100) >> 1;
    (rb & 0x00ff_00ff) | (g & 0x0000_ff00)
}

fn bob_pixel(current: &[u32], width: usize, height: usize, x: usize, y: usize) -> u32 {
    let upper = if y > 0 {
        y - 1
    } else {
        (y + 1).min(height - 1)
    };
    let lower = if y + 1 < height { y + 1 } else { upper };
    average_pixel(current[upper * width + x], current[lower * width + x])
}

fn nearby_field_motion(motion: &[u8], width: usize, height: usize, x: usize, y: usize) -> bool {
    let first_row = y.saturating_sub(1);
    let last_row = (y + 1).min(height - 1);
    let first_column = x.saturating_sub(1);
    let last_column = (x + 1).min(width - 1);
    (first_row..=last_row).any(|row| {
        motion[row * width + first_column..=row * width + last_column]
            .iter()
            .any(|value| *value != 0)
    })
}

/// Convert the raw MCD212 field weave into a progressive host image.
///
/// Static areas retain both original field rows. In moving areas, one stable
/// field phase is retained and the complementary rows are reconstructed from
/// it. Holding that phase until its next field arrives avoids the one-line
/// vertical jitter of alternating-phase bob at the cost of presenting motion
/// at the completed-frame cadence (25/30 Hz). This is presentation-only: the
/// core framebuffer and compatibility diagnostic captures remain the exact
/// raw hardware weave.
pub(super) fn motion_adaptive_deinterlace(
    current: &[u32],
    previous: Option<&[u32]>,
    output: &mut [u32],
    motion: &mut [u8],
    width: usize,
    height: usize,
    geometry: DisplayGeometry,
) {
    let len = width.saturating_mul(height);
    assert!(current.len() >= len);
    assert!(output.len() >= len);
    assert!(motion.len() >= len);
    output[..len].copy_from_slice(&current[..len]);
    if !geometry.interlaced || width == 0 || height == 0 {
        return;
    }

    // MCD212 toggles PA after composing a completed field, so the live
    // geometry reports the next field. Compare only the freshly rendered
    // parity with its preceding same-parity field to find motion.
    let current_row_parity = usize::from(!geometry.odd_field);
    let previous = previous.filter(|frame| frame.len() >= len);
    if let Some(previous) = previous {
        for value in &mut motion[..len] {
            *value = value.saturating_sub(1);
        }
        for y in (current_row_parity..height).step_by(2) {
            for x in 0..width {
                let index = y * width + x;
                let threshold = if motion[index] == 0 {
                    MOTION_START_THRESHOLD
                } else {
                    MOTION_CONTINUE_THRESHOLD
                };
                if pixel_changed(current[index], previous[index], threshold) {
                    motion[index] = MOTION_HOLD_FIELDS;
                }
            }
        }
    } else {
        motion[..len].fill(MOTION_HOLD_FIELDS);
    }

    // Anchor motion to the parity of the first active raster row. Selecting a
    // fixed phase rather than whichever field just completed prevents the
    // whole moving picture from alternating vertically by one scanline.
    let source_row_parity = geometry.active_y & 1;
    for y in ((source_row_parity ^ 1)..height).step_by(2) {
        for x in 0..width {
            if previous.is_none() || nearby_field_motion(motion, width, height, x, y) {
                output[y * width + x] = bob_pixel(current, width, height, x, y);
            }
        }
    }
}

pub(super) fn presentation_aspect(
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

pub(super) fn fit_aspect(available: egui::Vec2, aspect: f32) -> egui::Vec2 {
    if available.x <= 0.0 || available.y <= 0.0 || aspect <= 0.0 {
        return egui::Vec2::ZERO;
    }
    let width = available.x.min(available.y * aspect);
    egui::vec2(width, width / aspect)
}

pub(super) fn display_aperture(frame: &SharedFrame) -> DisplayAperture {
    DisplayAperture {
        left: frame.geometry.active_x,
        top: frame.geometry.active_y,
        width: frame.geometry.active_width,
        height: frame.geometry.active_height,
    }
}

pub(super) fn pointer_mapping(
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

pub(super) fn screenshot_dimensions(
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
pub(super) fn screenshot_image(
    frame: &SharedFrame,
    crt_aspect: bool,
) -> cdi_photocd::decode::DecodedImage {
    let aperture = display_aperture(frame);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry(interlaced: bool, odd_field: bool) -> DisplayGeometry {
        DisplayGeometry {
            raster_width: 4,
            raster_height: 4,
            active_x: 0,
            active_y: 0,
            active_width: 4,
            active_height: 4,
            compatibility_mode: false,
            interlaced,
            odd_field,
            frame_duration_60hz: false,
            pixel_aspect_num: 1,
            pixel_aspect_den: 1,
        }
    }

    #[test]
    fn progressive_frame_is_unchanged() {
        let input: Vec<u32> = (0..16).collect();
        let mut output = vec![0; input.len()];
        let mut motion = vec![0; input.len()];
        motion_adaptive_deinterlace(
            &input,
            None,
            &mut output,
            &mut motion,
            4,
            4,
            geometry(false, false),
        );
        assert_eq!(output, input);
    }

    #[test]
    fn static_interlaced_frame_preserves_the_original_weave() {
        let input: Vec<u32> = (0..16).map(|value| 0x101010 + value).collect();
        let mut output = vec![0; input.len()];
        let mut motion = vec![0; input.len()];
        motion_adaptive_deinterlace(
            &input,
            Some(&input),
            &mut output,
            &mut motion,
            4,
            4,
            geometry(true, true),
        );
        assert_eq!(output, input);
    }

    #[test]
    fn moving_even_field_replaces_old_odd_rows() {
        let previous = vec![0x101010; 16];
        let mut current = previous.clone();
        current[0..4].fill(0x303030);
        current[8..12].fill(0x505050);
        let mut output = vec![0; current.len()];
        let mut motion = vec![0; current.len()];
        // odd_field is the next field, so even rows are the new field.
        motion_adaptive_deinterlace(
            &current,
            Some(&previous),
            &mut output,
            &mut motion,
            4,
            4,
            geometry(true, true),
        );
        assert_eq!(&output[0..4], &[0x303030; 4]);
        assert_eq!(&output[4..8], &[0x404040; 4]);
        assert_eq!(&output[8..12], &[0x505050; 4]);
        assert_eq!(&output[12..16], &[0x505050; 4]);
    }

    #[test]
    fn moving_odd_field_holds_the_stable_even_phase() {
        let previous = vec![0x101010; 16];
        let mut current = previous.clone();
        current[4..8].fill(0x303030);
        current[12..16].fill(0x505050);
        let mut output = vec![0; current.len()];
        let mut motion = vec![0; current.len()];
        // even is the next field, so odd rows are the new field.
        motion_adaptive_deinterlace(
            &current,
            Some(&previous),
            &mut output,
            &mut motion,
            4,
            4,
            geometry(true, false),
        );
        assert_eq!(output, previous);
    }

    #[test]
    fn consecutive_motion_fields_do_not_alternate_vertical_phase() {
        let old = vec![0x101010; 16];
        let mut even_field = old.clone();
        even_field[0..4].fill(0x303030);
        even_field[8..12].fill(0x505050);
        let mut first_output = vec![0; even_field.len()];
        let mut motion = vec![0; even_field.len()];
        motion_adaptive_deinterlace(
            &even_field,
            Some(&old),
            &mut first_output,
            &mut motion,
            4,
            4,
            geometry(true, true),
        );

        let mut completed_pair = even_field.clone();
        completed_pair[4..8].fill(0x303030);
        completed_pair[12..16].fill(0x505050);
        let mut second_output = vec![0; completed_pair.len()];
        motion_adaptive_deinterlace(
            &completed_pair,
            Some(&even_field),
            &mut second_output,
            &mut motion,
            4,
            4,
            geometry(true, false),
        );

        assert_eq!(second_output, first_output);
    }

    #[test]
    fn motion_mask_persists_before_returning_to_weave() {
        let previous = vec![0x101010; 16];
        let mut current = previous.clone();
        current[0..4].fill(0x303030);
        current[8..12].fill(0x505050);
        let mut output = vec![0; current.len()];
        let mut motion = vec![0; current.len()];
        motion_adaptive_deinterlace(
            &current,
            Some(&previous),
            &mut output,
            &mut motion,
            4,
            4,
            geometry(true, true),
        );
        assert_eq!(&output[4..8], &[0x404040; 4]);

        for remaining in (1..MOTION_HOLD_FIELDS).rev() {
            motion_adaptive_deinterlace(
                &current,
                Some(&current),
                &mut output,
                &mut motion,
                4,
                4,
                geometry(true, false),
            );
            assert_eq!(&output[4..8], &[0x404040; 4]);
            assert_eq!(motion[0], remaining);
        }

        motion_adaptive_deinterlace(
            &current,
            Some(&current),
            &mut output,
            &mut motion,
            4,
            4,
            geometry(true, false),
        );
        assert_eq!(output, current);
        assert_eq!(motion[0], 0);
    }

    #[test]
    fn low_delta_continues_existing_motion_but_does_not_start_it() {
        let previous = vec![0x101010; 16];
        let mut current = previous.clone();
        current[0..4].fill(0x141414);
        let mut output = vec![0; current.len()];
        let mut motion = vec![0; current.len()];
        motion_adaptive_deinterlace(
            &current,
            Some(&previous),
            &mut output,
            &mut motion,
            4,
            4,
            geometry(true, true),
        );
        assert_eq!(motion[0], 0);

        motion[0..4].fill(2);
        motion_adaptive_deinterlace(
            &current,
            Some(&previous),
            &mut output,
            &mut motion,
            4,
            4,
            geometry(true, true),
        );
        assert_eq!(motion[0], MOTION_HOLD_FIELDS);
    }

    #[test]
    fn field_layout_ignores_only_parity() {
        let even = geometry(true, false);
        let mut odd = even;
        odd.odd_field = true;
        assert!(same_field_layout(even, odd));
        odd.active_y = 1;
        assert!(!same_field_layout(even, odd));
    }
}
