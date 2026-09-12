//! Page-number-aware margin cropping (KCC's `--cropping 2`, the default).
//!
//! Port target: `get_bbox_crop_margin_page_number()` in
//! `page_number_crop_alg.py`, plus the `group_close_values`/`merge_boxes`/
//! `box_intersect` helpers it depends on (`common_crop.py`/same file). No
//! license header upstream on any of these — treated with the same caution
//! as GPLv3 files per `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`: this
//! is a reimplementation from documented/observed behavior.
//!
//! Same base pipeline as [`super::margin`] (via [`super::binarize_for_crop`])
//! to get an overall content [`super::Bbox`]. Then: look at a thin window
//! just above the bbox's bottom edge, find small dark "blob" shapes in it by
//! grouping nearby dark pixels per row and merging nearby row-groups across
//! rows, and — *only* if there's exactly one such blob and it's small enough
//! to plausibly be a 1-3 digit page number — crop everything from just above
//! that blob downward. Two or more blobs (real content, not a lone page
//! number) or an oversized blob leave the page's bbox untouched.
//!
//! The size/distance tolerances below are fractions of the *full page*
//! dimensions, not the small window — on realistic manga page resolutions
//! (hundreds to low thousands of pixels tall) this gives the row-merge
//! tolerance enough slack (a few pixels) to bridge anti-aliased/gapped digit
//! strokes across rows. On a tiny page (well under ~500px tall) the
//! y-tolerance rounds down below one pixel and adjacent-row merging
//! effectively stops working — a real characteristic of upstream's
//! fraction-of-full-page-height scaling, not a bug introduced here.

use super::{apply_policy, Background, Bbox, CropBox, CropPolicy};
use image::GrayImage;

const MAX_SHAPE_WIDTH_FRAC: f64 = 0.015 * 3.0; // 0.045
const MAX_SHAPE_HEIGHT_FRAC: f64 = 0.02;
const MIN_SHAPE_WIDTH_FRAC: f64 = 0.003;
const MIN_SHAPE_HEIGHT_FRAC: f64 = 0.006;
const WINDOW_HEIGHT_FRAC: f64 = MAX_SHAPE_HEIGHT_FRAC * 1.25; // 0.025
const MAX_DIST_X_FRAC: f64 = 0.01;
const MAX_DIST_Y_FRAC: f64 = 0.002;

/// (left, right, top, bottom) — matches upstream's `(x0, x1, y0, y1)` row/blob
/// box tuples used while scanning the bottom window. Coordinates here are
/// window-local (x relative to the overall bbox's left edge, y relative to
/// the window's top row), not absolute page coordinates.
type RowBox = (f64, f64, f64, f64);

/// `get_bbox_crop_margin_page_number()`.
pub fn get_bbox_crop_margin_page_number(
    img: &GrayImage,
    power: f32,
    background: Background,
) -> Option<Bbox> {
    let prepped = super::binarize_for_crop(img, power, background);
    let bbox = prepped.bbox?;
    let (w, h) = img.dimensions();

    let window_h = (h as f64 * WINDOW_HEIGHT_FRAC) as u32;
    // Degenerate window (tiny page, or content shorter than the window
    // itself): upstream would rely on PIL's out-of-bounds crop padding
    // here, which isn't behavior worth reproducing exactly — just skip the
    // page-number-specific narrowing and return the plain margin bbox.
    if window_h == 0 || bbox.bottom < window_h {
        return Some(bbox);
    }
    let window_top = bbox.bottom - window_h;

    let max_dist_x = w as f64 * MAX_DIST_X_FRAC;
    let max_dist_y = h as f64 * MAX_DIST_Y_FRAC;

    let mut window_groups: Vec<RowBox> = Vec::new();
    for row_local in 0..window_h {
        let abs_y = window_top + row_local;
        let dark_columns: Vec<i64> = (bbox.left..bbox.right)
            .filter(|&x| prepped.grayscale.get_pixel(x, abs_y)[0] as f32 <= prepped.threshold)
            .map(|x| (x - bbox.left) as i64)
            .collect();
        for (g0, g1) in group_close_values(&dark_columns, max_dist_x) {
            window_groups.push((g0 as f64, g1 as f64, row_local as f64, row_local as f64));
        }
    }

    let boxes = merge_boxes(window_groups, (max_dist_x, max_dist_y));

    let min_shape_w = w as f64 * MIN_SHAPE_WIDTH_FRAC;
    let min_shape_h = h as f64 * MIN_SHAPE_HEIGHT_FRAC;
    let boxes: Vec<RowBox> = boxes
        .into_iter()
        .filter(|b| (b.1 - b.0) >= min_shape_w && (b.3 - b.2) >= min_shape_h)
        .collect();

    let lowest_row = (window_h - 1) as f64;
    let lowest_boxes: Vec<RowBox> = boxes
        .iter()
        .copied()
        .filter(|b| b.3 == lowest_row)
        .collect();

    let min_y_of_lowest_boxes = lowest_boxes
        .iter()
        .map(|b| b.2)
        .fold(None, |acc: Option<f64>, v| {
            Some(acc.map_or(v, |a| a.min(v)))
        })
        .unwrap_or(0.0);

    let boxes_in_same_y_range: Vec<RowBox> = boxes
        .into_iter()
        .filter(|b| b.3 >= min_y_of_lowest_boxes)
        .collect();

    let max_shape_w = w as f64 * MAX_SHAPE_WIDTH_FRAC;
    let max_shape_h = (h as f64 * MAX_SHAPE_HEIGHT_FRAC).max(3.0);

    let should_force_crop = boxes_in_same_y_range.len() == 1
        && (boxes_in_same_y_range[0].1 - boxes_in_same_y_range[0].0) <= max_shape_w
        && (boxes_in_same_y_range[0].3 - boxes_in_same_y_range[0].2) <= max_shape_h;

    let restrict_to = if should_force_crop {
        let top_row = boxes_in_same_y_range[0].2;
        let new_bottom = bbox.bottom as f64 - (window_h as f64 - top_row + 1.0);
        new_bottom.max(0.0) as u32
    } else {
        h
    };

    let restricted = image::imageops::crop_imm(&prepped.binary, 0, 0, w, restrict_to).to_image();
    super::get_bbox(&restricted)
}

/// `cropPageNumber()` + `maybeCrop()`.
pub fn compute_margin_crop_ignoring_page_number(
    img: &GrayImage,
    policy: &CropPolicy,
) -> Option<CropBox> {
    let bbox = get_bbox_crop_margin_page_number(img, policy.power, policy.background)?;
    apply_policy(bbox, img.dimensions(), policy)
}

/// `group_close_values()` (`common_crop.py`). `vals` must already be sorted
/// ascending (matches how `np.where(...)` produces them upstream).
///
/// Faithfully reproduces an upstream quirk: the value that triggers a group
/// split (too far from the current group) is dropped entirely — neither
/// appended to the group it broke away from, nor used to start the next
/// one. Confirmed by tracing the source rather than assumed.
fn group_close_values(vals: &[i64], max_dist_tolerated: f64) -> Vec<(i64, i64)> {
    let mut groups = Vec::new();
    let mut group_start: Option<i64> = None;
    let mut group_end: i64 = 0;

    for &v in vals {
        match group_start {
            None => {
                group_start = Some(v);
                group_end = v;
            }
            Some(gs) => {
                let dist = (v - group_end) as f64;
                if dist <= max_dist_tolerated {
                    group_end = v;
                } else {
                    groups.push((gs, group_end));
                    group_start = None;
                }
            }
        }
    }
    if let Some(gs) = group_start {
        groups.push((gs, group_end));
    }
    groups
}

/// `box_intersect()`: are `box1`/`box2` within `max_dist` (per-axis) of each
/// other?
fn box_intersect(box1: RowBox, box2: RowBox, max_dist: (f64, f64)) -> bool {
    !(box2.0 - max_dist.0 > box1.1
        || box2.1 + max_dist.0 < box1.0
        || box2.2 - max_dist.1 > box1.3
        || box2.3 + max_dist.1 < box1.2)
}

/// `merge_boxes()`: repeatedly merges any box within `max_dist_tolerated` of
/// `boxes[j]` into one bounding box, restarting the scan from the start
/// whenever a merge happens (so transitively-connected boxes fully merge
/// even though each pass only looks at what's currently at/after index `j`).
fn merge_boxes(mut boxes: Vec<RowBox>, max_dist_tolerated: (f64, f64)) -> Vec<RowBox> {
    let mut j = 0;
    while j + 1 < boxes.len() {
        let g1 = boxes[j];
        let mut intersecting = Vec::new();
        let mut others = Vec::new();
        for &g2 in &boxes[j + 1..] {
            if box_intersect(g1, g2, max_dist_tolerated) {
                intersecting.push(g2);
            } else {
                others.push(g2);
            }
        }

        if intersecting.is_empty() {
            j += 1;
        } else {
            let mut all = vec![g1];
            all.extend(intersecting);
            let merged = (
                all.iter().map(|b| b.0).fold(f64::INFINITY, f64::min),
                all.iter().map(|b| b.1).fold(f64::NEG_INFINITY, f64::max),
                all.iter().map(|b| b.2).fold(f64::INFINITY, f64::min),
                all.iter().map(|b| b.3).fold(f64::NEG_INFINITY, f64::max),
            );
            others.push(merged);
            let mut next = boxes[..j].to_vec();
            next.extend(others);
            boxes = next;
            j = 0;
        }
    }
    boxes
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    // Realistic-ish page dimensions matter here: the merge/size tolerances
    // are fractions of the *page*, and at toy sizes (a few hundred px) they
    // round down to sub-pixel values that never let adjacent scan-rows
    // merge at all. See the module doc.
    const W: u32 = 800;
    const H: u32 = 1200;

    fn page_with_rects(rects: &[(u32, u32, u32, u32)]) -> GrayImage {
        GrayImage::from_fn(W, H, |x, y| {
            let hit = rects
                .iter()
                .any(|&(l, t, r, b)| x >= l && x < r && y >= t && y < b);
            if hit {
                Luma([0])
            } else {
                Luma([255])
            }
        })
    }

    #[test]
    fn group_close_values_merges_within_tolerance_and_splits_beyond_it() {
        assert_eq!(
            group_close_values(&[1, 2, 3, 100, 101], 5.0),
            vec![(1, 3), (101, 101)]
        );
    }

    #[test]
    fn group_close_values_of_empty_input_is_empty() {
        assert_eq!(group_close_values(&[], 5.0), vec![]);
    }

    #[test]
    fn lone_small_blob_at_bottom_gets_cropped_out() {
        // Main content, then an isolated small "page number" blob well
        // below it (within size limits: <0.045*W=36 wide, <0.02*H=24 tall).
        let img = page_with_rects(&[(40, 40, 760, 1100), (385, 1140, 415, 1160)]);
        let bbox = get_bbox_crop_margin_page_number(&img, 1.0, Background::White)
            .expect("content should be detected");
        // The page number (bottom ~1160) must be excluded; the result
        // should track the main content's actual bottom edge (~1100), not
        // the page number's.
        assert!(bbox.bottom <= 1110, "bottom={}", bbox.bottom);
        assert!(bbox.bottom > 1095, "bottom={}", bbox.bottom);
    }

    #[test]
    fn two_blobs_at_bottom_are_not_treated_as_a_page_number() {
        // Two separate small marks near the bottom — real content
        // (e.g. two speech-bubble tails), not a single page number.
        let img = page_with_rects(&[
            (40, 40, 760, 1100),
            (200, 1140, 220, 1160),
            (500, 1140, 520, 1160),
        ]);
        let bbox = get_bbox_crop_margin_page_number(&img, 1.0, Background::White)
            .expect("content should be detected");
        // Nothing should be cropped off: bottom should still reach down to
        // the lower blobs (~1160), not stop at the main content (~1100).
        assert!(bbox.bottom > 1150, "bottom={}", bbox.bottom);
    }

    #[test]
    fn oversized_blob_at_bottom_is_not_treated_as_a_page_number() {
        // A single blob at the bottom, but too wide (>0.045*W=36) to
        // plausibly be a 1-3 digit page number.
        let img = page_with_rects(&[(40, 40, 760, 1100), (300, 1140, 500, 1160)]);
        let bbox = get_bbox_crop_margin_page_number(&img, 1.0, Background::White)
            .expect("content should be detected");
        assert!(bbox.bottom > 1150, "bottom={}", bbox.bottom);
    }

    #[test]
    fn content_reaching_all_the_way_down_is_left_alone() {
        // No separate page number at all — content fills the whole window.
        let img = page_with_rects(&[(40, 40, 760, 1160)]);
        let bbox = get_bbox_crop_margin_page_number(&img, 1.0, Background::White)
            .expect("content should be detected");
        assert!(bbox.bottom > 1150, "bottom={}", bbox.bottom);
    }
}
