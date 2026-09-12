//! Inter-panel empty-space cropping (KCC's `--ipc/--interpanelcrop`).
//!
//! Port target: `crop_empty_inter_panel()` in `inter_panel_crop_alg.py`.
//! Removes empty horizontal (and optionally vertical, mode 2) gutters
//! *between* panels on the same page — distinct from [`super::margin`],
//! which only trims the page's outer border.

use image::GrayImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterPanelMode {
    Disabled,
    Horizontal,
    Both,
}

pub fn crop_empty_inter_panel_sections(_page: &GrayImage, _mode: InterPanelMode) -> GrayImage {
    todo!("port crop_empty_inter_panel against synthetic fixtures — see module docs")
}
