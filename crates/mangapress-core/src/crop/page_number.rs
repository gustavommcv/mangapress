//! Page-number-aware margin cropping (KCC's `--cropping 2`, the default).
//!
//! Port target: `get_bbox_crop_margin_page_number()` in
//! `page_number_crop_alg.py`. Same base pipeline as [`super::margin`], plus:
//! after the normal margin bbox, inspect a bottom window of height
//! `0.02 * 1.25` of the page height for a single small isolated blob shaped
//! like a 1-3 digit page number (size-tolerance range defined in upstream
//! lines 14-17 — re-derive exact bounds from fixtures rather than guessing,
//! since this is the most heuristic of the three crop algorithms). If found,
//! force-crop it out even if it would otherwise survive the margin bbox.

use crate::crop::margin::CropBox;
use image::GrayImage;

pub fn compute_margin_crop_ignoring_page_number(
    _page: &GrayImage,
    _power: f32,
    _minimum: f32,
) -> CropBox {
    todo!("port get_bbox_crop_margin_page_number against synthetic fixtures — see module docs")
}
