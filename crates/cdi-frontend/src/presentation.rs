// SPDX-License-Identifier: GPL-3.0-or-later
//! Host presentation geometry shared by rendering, screenshots, and pointer
//! mapping.

use cdi_core::mcd212::{presentation_rgb, DisplayGeometry};

use super::SharedFrame;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DisplayAperture {
    pub(super) left: usize,
    pub(super) top: usize,
    pub(super) width: usize,
    pub(super) height: usize,
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
