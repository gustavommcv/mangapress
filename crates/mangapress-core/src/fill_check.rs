//! Per-page background-color detection — `fillCheck()`/`getImageHistogram()`
//! in KCC's `image.py` (`ComicPageParser`). GPLv3 upstream; this is an
//! independent reimplementation from its documented/observed behavior, not
//! a port — see `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`.
//!
//! Nearly every manga page has a white background with dark ink content,
//! which is what every crop/resize/contrast function in this crate assumes
//! by default (see [`crate::crop::Background`]'s doc comment). A minority of
//! pages — typically stylized chapter-title or cover illustrations — have a
//! genuinely dark background instead, and need those algorithms to run
//! inverted: this module is what decides which case a given page falls
//! into.

use crate::crop::{get_bbox, Background, Bbox};
use image::{GrayImage, Luma};

/// `fillCheck()`. Thresholds the page to pure black/white at a fixed
/// midpoint (128), then compares the bounding-box area of the light pixels
/// against the bounding-box area of the dark pixels: whichever color is
/// confined to the *smaller* box is assumed to be the actual foreground
/// content (art/text), and the other — typically spanning most or all of
/// the page — is assumed to be the background. This only decides the page
/// when the two areas differ by more than upstream's own 0.5% tolerance
/// (relative to the smaller one); a near-tie, including the degenerate case
/// where the page is a single flat color (so one side has no bounding box
/// at all), falls back to [`strip_dominance`].
pub fn fill_check(img: &GrayImage) -> Background {
    let (w, h) = img.dimensions();
    let mut bw = GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let light = img.get_pixel(x, y)[0] >= 128;
            bw.put_pixel(x, y, Luma([if light { 255 } else { 0 }]));
        }
    }

    let light_bbox = get_bbox(&bw);
    let mut dark_mask = bw.clone();
    image::imageops::invert(&mut dark_mask);
    let dark_bbox = get_bbox(&dark_mask);

    let (light_area, dark_area) = match (light_bbox, dark_bbox) {
        (Some(l), Some(d)) => (bbox_area(l), bbox_area(d)),
        _ => return strip_dominance(&bw),
    };

    let (bigger, smaller) = if light_area >= dark_area {
        (light_area, dark_area)
    } else {
        (dark_area, light_area)
    };
    let diff_percent = (bigger - smaller) as f64 / smaller as f64 * 100.0;

    if diff_percent > 0.5 {
        if dark_area < light_area {
            Background::White
        } else {
            Background::Dark
        }
    } else {
        strip_dominance(&bw)
    }
}

fn bbox_area(bbox: Bbox) -> u64 {
    (bbox.right - bbox.left) as u64 * (bbox.bottom - bbox.top) as u64
}

/// Tie-breaker fallback: walks the whole page in 5px-wide horizontal bands,
/// then again in 5px-wide vertical bands (the two scans overlap the same
/// pixels — matching upstream exactly rather than deduplicating), scoring
/// each band -1 if it's entirely light, +1 if entirely dark, 0 if mixed. A
/// net-positive score across every band means solid-dark bands were more
/// common overall than solid-light ones, and vice versa.
fn strip_dominance(bw: &GrayImage) -> Background {
    let (w, h) = bw.dimensions();
    if w == 0 || h == 0 {
        return Background::White;
    }

    let mut score = 0i64;

    let mut y = 0u32;
    loop {
        let y0 = if y + 5 > h { h.saturating_sub(5) } else { y };
        score += band_score(bw, 0, y0, w, (y0 + 5).min(h));
        if y + 5 >= h {
            break;
        }
        y += 5;
    }

    let mut x = 0u32;
    loop {
        let x0 = if x + 5 > w { w.saturating_sub(5) } else { x };
        score += band_score(bw, x0, 0, (x0 + 5).min(w), h);
        if x + 5 >= w {
            break;
        }
        x += 5;
    }

    if score > 0 {
        Background::Dark
    } else {
        Background::White
    }
}

fn band_score(bw: &GrayImage, x0: u32, y0: u32, x1: u32, y1: u32) -> i64 {
    let mut has_light = false;
    let mut has_dark = false;
    for y in y0..y1 {
        for x in x0..x1 {
            if bw.get_pixel(x, y)[0] == 255 {
                has_light = true;
            } else {
                has_dark = true;
            }
        }
    }
    match (has_dark, has_light) {
        (false, true) => -1,
        (true, false) => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, value: u8) -> GrayImage {
        GrayImage::from_pixel(w, h, Luma([value]))
    }

    fn page_with_rect(
        size: (u32, u32),
        background: u8,
        foreground: u8,
        rect: (u32, u32, u32, u32),
    ) -> GrayImage {
        let (w, h) = size;
        let (l, t, r, b) = rect;
        GrayImage::from_fn(w, h, |x, y| {
            if x >= l && x < r && y >= t && y < b {
                Luma([foreground])
            } else {
                Luma([background])
            }
        })
    }

    #[test]
    fn a_dark_blob_on_a_white_page_reads_as_white_background() {
        // A normal manga page: mostly-white background, a smaller block of
        // dark ink/art comfortably inside it.
        let img = page_with_rect((800, 1200), 255, 0, (100, 100, 700, 1100));
        assert_eq!(fill_check(&img), Background::White);
    }

    #[test]
    fn a_light_blob_on_a_dark_page_reads_as_dark_background() {
        // A stylized chapter-title page: mostly-black background, a smaller
        // block of light artwork/text inside it.
        let img = page_with_rect((800, 1200), 0, 255, (100, 100, 700, 1100));
        assert_eq!(fill_check(&img), Background::Dark);
    }

    #[test]
    fn a_uniformly_white_page_reads_as_white_background() {
        let img = solid(400, 600, 255);
        assert_eq!(fill_check(&img), Background::White);
    }

    #[test]
    fn a_uniformly_black_page_reads_as_dark_background() {
        let img = solid(400, 600, 0);
        assert_eq!(fill_check(&img), Background::Dark);
    }

    #[test]
    fn a_bbox_tie_falls_back_to_which_color_dominates_overall() {
        // Almost the entire page is dark, except for a single light pixel
        // in each corner -- placed so that both the light bbox and the dark
        // bbox independently span the full page (each corner has a light
        // pixel exactly at the extreme row/column, but the rest of that same
        // row/column is still dark), making the bbox-area comparison an
        // exact tie (0% difference) that must fall back to strip_dominance.
        // Since the page is otherwise almost entirely dark, that fallback
        // should call it a dark background.
        let (w, h) = (400u32, 400u32);
        let corners = [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)];
        let img = GrayImage::from_fn(w, h, |x, y| {
            if corners.contains(&(x, y)) {
                Luma([255])
            } else {
                Luma([0])
            }
        });
        assert_eq!(fill_check(&img), Background::Dark);
    }
}
