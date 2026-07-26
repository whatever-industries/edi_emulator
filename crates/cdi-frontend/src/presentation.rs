// SPDX-License-Identifier: GPL-3.0-or-later
//! Host presentation geometry shared by rendering, screenshots, and pointer
//! mapping.

use cdi_core::mcd212::{presentation_rgb, DisplayGeometry};

use super::SharedFrame;

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub(super) enum DisplayArea {
    #[default]
    TypicalCrt,
    FullSignal,
}

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

pub(super) fn display_aperture(frame: &SharedFrame, display_area: DisplayArea) -> DisplayAperture {
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
