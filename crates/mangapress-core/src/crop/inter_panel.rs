//! Inter-panel empty-space cropping (KCC's `--ipc/--interpanelcrop`).
//!
//! Port target: `crop_empty_inter_panel()`/`empty_sections()` in
//! `inter_panel_crop_alg.py` (no license header upstream — treated with the
//! same caution as GPLv3 files per
//! `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`). Removes empty
//! horizontal (and, in `Both` mode, vertical too) gutters *between* panels
//! on the same page — distinct from [`super::margin`], which only trims
//! the page's outer border and explicitly ignores near-border emptiness.
//!
//! Detection uses the same autocontrast(cutoff=1)+box-blur(1)+threshold
//! preprocessing as margin/page-number crop (see
//! [`super::binarize_for_crop`]), but with two differences confirmed by
//! reading the source, not assumed: the crop power is hardcoded to `1.0`
//! here regardless of `-c/--croppingpower`, and `ignore_pixels_near_edge`
//! is never called (border exclusion is handled separately below).
//!
//! A row/column is "empty" if every pixel in it is background (0) in the
//! binarized proxy. Consecutive empty rows/columns form a "gutter"
//! section; sections touching within 1% of either border are excluded
//! (matches upstream's confirmed quirk of comparing *both* the row and
//! column border checks against the image's **height**, `img.size[1]`, not
//! the relevant axis's own length — almost certainly an upstream oversight
//! for the column/vertical case, reproduced here deliberately rather than
//! "fixed", since silently diverging from a real, traceable behavior is
//! worse than flagging it). Each remaining gutter is then shrunk by `KEEP`
//! (a fixed 4%, split evenly off each side) before removal, so panels don't
//! end up touching edge-to-edge.

use super::{binarize_for_crop, group_close_values, Background};
use image::GrayImage;
use std::collections::HashSet;

/// `KCC`'s `-r/--interpanelcrop` values: `0: Disabled 1: Horizontally 2: Both`
/// (there is no vertical-only option upstream).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterPanelMode {
    Disabled,
    Horizontal,
    Both,
}

/// Fraction of each gutter's own width kept as breathing room between
/// panels after cropping (upstream's `empty_sections(..., keep=0.04)`
/// default — not exposed as a CLI flag upstream, so not one here either).
const KEEP: f64 = 0.04;

/// Crop power is hardcoded upstream (`power = 1` as a local variable, not
/// `self.opt.croppingpower`) — this is not an oversight in this port, it's
/// what the source actually does.
const CROP_POWER: f32 = 1.0;

pub fn crop_empty_inter_panel_sections(
    page: &GrayImage,
    mode: InterPanelMode,
    background: Background,
) -> GrayImage {
    if mode == InterPanelMode::Disabled {
        return page.clone();
    }

    let binarized = binarize_for_crop(page, CROP_POWER, background).binary;

    let rows_to_remove = empty_sections(&binarized, true);
    let cols_to_remove = if mode == InterPanelMode::Both {
        empty_sections(&binarized, false)
    } else {
        Vec::new()
    };

    delete_rows_and_cols(page, &rows_to_remove, &cols_to_remove)
}

/// `empty_sections()`. `horizontal = true` finds empty *rows* (to delete,
/// shrinking the page vertically); `false` finds empty *columns*.
fn empty_sections(binarized: &GrayImage, horizontal: bool) -> Vec<u32> {
    let (w, h) = binarized.dimensions();
    let (outer_len, inner_len) = if horizontal { (h, w) } else { (w, h) };

    let mut empty_indices: Vec<i64> = Vec::new();
    for outer in 0..outer_len {
        let mut max_val = 0u8;
        for inner in 0..inner_len {
            let (x, y) = if horizontal {
                (inner, outer)
            } else {
                (outer, inner)
            };
            let v = binarized.get_pixel(x, y)[0];
            if v > max_val {
                max_val = v;
                if max_val == 255 {
                    break;
                }
            }
        }
        if max_val == 0 {
            empty_indices.push(outer as i64);
        }
    }

    let groups = group_close_values(&empty_indices, 1.0);

    // Border exclusion: deliberately uses the image HEIGHT for both axes,
    // matching upstream's `img.size[1]` reference in both the row and
    // column cases — see module docs.
    let border_reference = h as f64;
    let sections_to_remove: Vec<(i64, i64)> = groups
        .into_iter()
        .filter(|&(start, end)| {
            (end as f64) < border_reference * 0.99 && (start as f64) > border_reference * 0.01
        })
        .collect();

    sections_to_remove
        .into_iter()
        .flat_map(|(x1, x2)| {
            let span = (x2 - x1) as f64;
            let shrunk_start = (x1 as f64 + (KEEP / 2.0) * span) as i64;
            let shrunk_end = (x2 as f64 - (KEEP / 2.0) * span) as i64;
            // Matches `np.arange(shrunk_start, shrunk_end)`: exclusive of
            // the end, so a section's own last original index is never
            // removed even at KEEP=0 -- an asymmetry inherent to upstream
            // reusing the inclusive (x1,x2) group bounds as an exclusive
            // arange call, not something to "correct" here.
            shrunk_start..shrunk_end
        })
        .map(|v| v as u32)
        .collect()
}

/// `np.delete(img_mat, idx, axis)` for both axes at once: builds a new
/// image containing only the rows/columns *not* marked for removal.
fn delete_rows_and_cols(img: &GrayImage, rows: &[u32], cols: &[u32]) -> GrayImage {
    let (w, h) = img.dimensions();
    let rows_set: HashSet<u32> = rows.iter().copied().collect();
    let cols_set: HashSet<u32> = cols.iter().copied().collect();

    let kept_rows: Vec<u32> = (0..h).filter(|y| !rows_set.contains(y)).collect();
    let kept_cols: Vec<u32> = (0..w).filter(|x| !cols_set.contains(x)).collect();

    if kept_rows.is_empty() || kept_cols.is_empty() {
        // Degenerate (would delete the whole page) -- leave it untouched
        // rather than produce a zero-sized image.
        return img.clone();
    }

    GrayImage::from_fn(kept_cols.len() as u32, kept_rows.len() as u32, |x, y| {
        *img.get_pixel(kept_cols[x as usize], kept_rows[y as usize])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    // Same reasoning as page_number.rs: tolerances/border fractions are
    // relative to full-page dimensions, so realistic-ish sizes matter for
    // the blur/threshold detection step to behave sensibly.
    const W: u32 = 400;
    const H: u32 = 600;

    fn page_with_two_panels_stacked(gutter_top: u32, gutter_bottom: u32) -> GrayImage {
        // Two black panels separated by a white horizontal gutter, well
        // away from the page borders.
        GrayImage::from_fn(W, H, |_x, y| {
            if y >= gutter_top && y < gutter_bottom {
                Luma([255]) // gutter (background)
            } else {
                Luma([0]) // panel content
            }
        })
    }

    #[test]
    fn disabled_mode_is_a_no_op() {
        let img = page_with_two_panels_stacked(250, 350);
        let out =
            crop_empty_inter_panel_sections(&img, InterPanelMode::Disabled, Background::White);
        assert_eq!(out, img);
    }

    #[test]
    fn horizontal_mode_removes_most_of_a_wide_gutter() {
        let img = page_with_two_panels_stacked(250, 350); // 100px gutter
        let out =
            crop_empty_inter_panel_sections(&img, InterPanelMode::Horizontal, Background::White);
        // Most of the 100px gutter should be gone, but KEEP=4% leaves a
        // small margin -- so height shrinks by roughly 90-99px, not the
        // full 100, and not zero.
        assert!(out.height() < H - 85, "height={}", out.height());
        assert!(out.height() > H - 100, "height={}", out.height());
        assert_eq!(out.width(), W, "horizontal mode must not touch width");
    }

    #[test]
    fn gutter_touching_the_border_is_not_removed() {
        // Gutter starts at y=0 -- within 1% of the top border, excluded by
        // the near-border check (this is what margin cropping is for).
        let img = page_with_two_panels_stacked(0, 50);
        let out =
            crop_empty_inter_panel_sections(&img, InterPanelMode::Horizontal, Background::White);
        assert_eq!(
            out.height(),
            H,
            "border-adjacent gutters must be left alone"
        );
    }

    #[test]
    fn both_mode_also_removes_vertical_gutters() {
        // Two panels side by side (vertical gutter) instead of stacked.
        let img = GrayImage::from_fn(W, H, |x, _y| {
            if (150..250).contains(&x) {
                Luma([255])
            } else {
                Luma([0])
            }
        });
        let horizontal_only =
            crop_empty_inter_panel_sections(&img, InterPanelMode::Horizontal, Background::White);
        assert_eq!(
            horizontal_only.width(),
            W,
            "Horizontal mode must not touch a vertical gutter"
        );

        let both = crop_empty_inter_panel_sections(&img, InterPanelMode::Both, Background::White);
        assert!(
            both.width() < W,
            "Both mode should remove the vertical gutter too"
        );
        assert_eq!(both.height(), H);
    }

    #[test]
    fn dark_background_page_is_inverted_before_detection() {
        let img = GrayImage::from_fn(W, H, |_x, y| {
            if (250..350).contains(&y) {
                Luma([0]) // gutter is dark here
            } else {
                Luma([255])
            }
        });
        let out =
            crop_empty_inter_panel_sections(&img, InterPanelMode::Horizontal, Background::Dark);
        assert!(
            out.height() < H,
            "dark-background gutter should still be detected and cropped"
        );
    }
}
