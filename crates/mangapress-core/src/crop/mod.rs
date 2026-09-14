//! Crop algorithms. KCC keeps these as three cooperating modules sharing a
//! common thresholding primitive; we mirror that split rather than merging
//! them, since each is independently testable against its own fixtures.

pub mod inter_panel;
pub mod margin;
pub mod page_number;

use image::{GrayImage, Luma};

/// `group_close_values()` (`common_crop.py`), shared by [`page_number`] and
/// [`inter_panel`]. `vals` must already be sorted ascending (matches how
/// `np.where(...)` produces them upstream).
///
/// Faithfully reproduces an upstream quirk: the value that triggers a group
/// split (too far from the current group) is dropped entirely — neither
/// appended to the group it broke away from, nor used to start the next
/// one. Confirmed by tracing the source rather than assumed.
pub(super) fn group_close_values(vals: &[i64], max_dist_tolerated: f64) -> Vec<(i64, i64)> {
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

/// `threshold_from_power(power)` from `common_crop.py`: `240 - power * 64`.
/// Shared by [`margin`] and [`page_number`] — both grayscale, invert if the
/// page background is dark, autocontrast, box-blur, then threshold at this
/// value and take a bounding box of what's left.
pub fn threshold_from_power(power: f32) -> f32 {
    240.0 - power * 64.0
}

/// The page's detected background color (upstream: `fillCheck()` in
/// `image.py`; detection itself lives in [`crate::fill_check`], out of scope
/// for the crop algorithms themselves, which just take the result as an
/// input).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    White,
    Dark,
}

/// A PIL-style bounding box: half-open, `right`/`bottom` exclusive
/// (`[left, right) x [top, bottom)`), matching `Image.getbbox()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bbox {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

/// The final crop rectangle to actually apply, in absolute pixel
/// coordinates (half-open, like [`Bbox`]) — distinct from `Bbox` because by
/// the time we have one of these, [`CropPolicy`]'s 10% cap and
/// `--preservemargin` back-off have already been folded in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropBox {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

/// Shared policy wrapped around either crop algorithm's raw bbox detection —
/// `cropMargin()`/`cropPageNumber()` + `maybeCrop()` in `image.py` are the
/// same wrapper applied to two different detectors.
#[derive(Debug, Clone, Copy)]
pub struct CropPolicy {
    /// `--croppingpower`. Higher power crops through more.
    pub power: f32,
    /// `--croppingminimum`: only actually crop if the crop region
    /// would keep at least this fraction of the page's area.
    pub minimum_area_ratio: f64,
    /// `--preservemargin`: back the computed crop off by this percentage
    /// after the 10% cap, so *some* margin is deliberately kept.
    pub preserve_margin_percent: f32,
    pub background: Background,
}

impl Default for CropPolicy {
    fn default() -> Self {
        CropPolicy {
            power: 1.0,
            minimum_area_ratio: 0.0,
            preserve_margin_percent: 0.0,
            background: Background::White,
        }
    }
}

/// `maybeCrop()`'s policy on top of a raw detected [`Bbox`]: cap to at most
/// 10% cropped per side, back off by `--preservemargin`, and only actually
/// crop if the result keeps at least `minimum_area_ratio` of the page.
/// Returns `None` when the computed crop doesn't clear the minimum-area
/// gate — meaning "leave the page as-is."
pub fn apply_policy(bbox: Bbox, image_size: (u32, u32), policy: &CropPolicy) -> Option<CropBox> {
    let capped = cap_to_ten_percent(bbox, image_size);
    let preserved = apply_preserve_margin(capped, image_size, policy.preserve_margin_percent);

    if area_ratio(preserved, image_size) >= policy.minimum_area_ratio {
        Some(preserved)
    } else {
        None
    }
}

/// Crop an image to a previously computed [`CropBox`].
pub fn apply_crop(img: &GrayImage, crop: CropBox) -> GrayImage {
    image::imageops::crop_imm(
        img,
        crop.left,
        crop.top,
        crop.right - crop.left,
        crop.bottom - crop.top,
    )
    .to_image()
}

/// `bbox = (min(0.1*w, left), min(0.1*h, upper), max(0.9*w, right),
/// max(0.9*h, lower))` — caps how much can be cropped off each side to at
/// most 10% of that dimension, without forcing a full 10% crop when the
/// detected margin is smaller than that.
fn cap_to_ten_percent(bbox: Bbox, (w, h): (u32, u32)) -> CropBox {
    let (w, h) = (w as f64, h as f64);
    CropBox {
        left: (0.1 * w).min(bbox.left as f64).round() as u32,
        top: (0.1 * h).min(bbox.top as f64).round() as u32,
        right: (0.9 * w).max(bbox.right as f64).round() as u32,
        bottom: (0.9 * h).max(bbox.bottom as f64).round() as u32,
    }
}

/// `ratio = 1 - preservemargin/100; box = left*ratio, upper*ratio,
/// right+(w-right)*(1-ratio), lower+(h-lower)*(1-ratio)`. With
/// `preserve_margin_percent == 0` this is an identity transform (`ratio ==
/// 1`), so upstream's `if self.opt.preservemargin:` guard doesn't need a
/// separate branch here.
fn apply_preserve_margin(
    crop: CropBox,
    (w, h): (u32, u32),
    preserve_margin_percent: f32,
) -> CropBox {
    // Clamped to keep `ratio` in [0.0, 1.0]: outside that range `right`
    // could end up left of `left`, and `apply_crop`'s `right - left` would
    // underflow and panic.
    let preserve_margin_percent = preserve_margin_percent.clamp(0.0, 100.0);
    let ratio = 1.0 - (preserve_margin_percent as f64) / 100.0;
    let (w, h) = (w as f64, h as f64);
    CropBox {
        left: (crop.left as f64 * ratio).round() as u32,
        top: (crop.top as f64 * ratio).round() as u32,
        right: (crop.right as f64 + (w - crop.right as f64) * (1.0 - ratio)).round() as u32,
        bottom: (crop.bottom as f64 + (h - crop.bottom as f64) * (1.0 - ratio)).round() as u32,
    }
}

fn area_ratio(crop: CropBox, (w, h): (u32, u32)) -> f64 {
    let box_area =
        crop.right.saturating_sub(crop.left) as f64 * crop.bottom.saturating_sub(crop.top) as f64;
    let image_area = w as f64 * h as f64;
    box_area / image_area
}

/// `Image.getbbox()`: the bounding box of non-zero pixels, or `None` if the
/// image is entirely zero.
pub fn get_bbox(img: &GrayImage) -> Option<Bbox> {
    let (w, h) = img.dimensions();
    let mut min_x = None;
    let mut max_x = None;
    let mut min_y = None;
    let mut max_y = None;

    for y in 0..h {
        for x in 0..w {
            if img.get_pixel(x, y)[0] != 0 {
                min_x = Some(min_x.map_or(x, |v: u32| v.min(x)));
                max_x = Some(max_x.map_or(x, |v: u32| v.max(x)));
                min_y = Some(min_y.map_or(y, |v: u32| v.min(y)));
                max_y = Some(max_y.map_or(y, |v: u32| v.max(y)));
            }
        }
    }

    match (min_x, max_x, min_y, max_y) {
        (Some(l), Some(r), Some(t), Some(b)) => Some(Bbox {
            left: l,
            top: t,
            right: r + 1,
            bottom: b + 1,
        }),
        _ => None,
    }
}

/// `ignore_pixels_near_edge()` from `page_number_crop_alg.py`. Confirmed by
/// reading the real upstream source directly (an earlier version of this
/// function guessed at the algorithm from its name/effect instead, and got
/// it wrong): it does *not* judge each outer 2% edge strip by its own
/// density. For each of the four edges it instead looks at a thin *inner*
/// band just past the raw edge (offset 2%-2.5% of that dimension) — if that
/// band is almost entirely empty (foreground density under upstream's
/// literal 0.1%, not 2%), real content is assumed not to reach that far in,
/// and then: that thin inner band itself is cleared if it had *any*
/// foreground at all (density > 0), and — independently, still gated on the
/// same inner-band density — the raw outer edge strip is wiped *entirely*
/// as scan noise if it contains *any* foreground pixel (a single stray dark
/// pixel is enough; this second check is not a density comparison). A page
/// small enough that the 2% and 2.5% cutoffs for a given dimension round
/// down to the same integer pixel count can't express these as distinct
/// bands, so upstream skips the whole function in that case — for both axes,
/// not just the too-small one.
pub fn ignore_pixels_near_edge(bw_img: &mut GrayImage) {
    let (w, h) = bw_img.dimensions();
    if (0.02 * h as f64) as u32 == (0.025 * h as f64) as u32 {
        return;
    }
    if (0.02 * w as f64) as u32 == (0.025 * w as f64) as u32 {
        return;
    }

    let e = |frac: f64, dim: u32| (frac * dim as f64) as u32;

    // (edge_box, inner_box) pairs, each (x0, y0, x1, y1).
    let regions = [
        (
            (0, 0, w, e(0.02, h)),
            (e(0.02, w), e(0.02, h), e(0.98, w), e(0.025, h)),
        ), // top
        (
            (0, e(0.98, h), w, h),
            (e(0.02, w), e(0.975, h), e(0.98, w), e(0.98, h)),
        ), // bottom
        (
            (0, 0, e(0.02, w), h),
            (e(0.02, w), e(0.02, h), e(0.025, w), e(0.98, h)),
        ), // left
        (
            (e(0.98, w), 0, w, h),
            (e(0.975, w), e(0.02, h), e(0.98, w), e(0.98, h)),
        ), // right
    ];

    let has_foreground = |img: &GrayImage, (x0, y0, x1, y1): (u32, u32, u32, u32)| {
        (y0..y1).any(|y| (x0..x1).any(|x| img.get_pixel(x, y)[0] == 255))
    };
    let fill_zero = |img: &mut GrayImage, (x0, y0, x1, y1): (u32, u32, u32, u32)| {
        for y in y0..y1 {
            for x in x0..x1 {
                img.put_pixel(x, y, Luma([0]));
            }
        }
    };

    for (edge_box, inner_box) in regions {
        let (ix0, iy0, ix1, iy1) = inner_box;
        if ix1 <= ix0 || iy1 <= iy0 {
            continue;
        }
        let inner_area = (ix1 - ix0) as f64 * (iy1 - iy0) as f64;
        let mut inner_foreground = 0u32;
        for y in iy0..iy1 {
            for x in ix0..ix1 {
                if bw_img.get_pixel(x, y)[0] == 255 {
                    inner_foreground += 1;
                }
            }
        }
        let imperfections = inner_foreground as f64 / inner_area;

        if imperfections > 0.0 && imperfections < 0.001 {
            fill_zero(bw_img, inner_box);
        }
        if imperfections < 0.001 && has_foreground(bw_img, edge_box) {
            fill_zero(bw_img, edge_box);
        }
    }
}

/// Output of the shared prefix both [`margin::get_bbox_crop_margin`] and
/// [`page_number::get_bbox_crop_margin_page_number`] start from: grayscale
/// input, invert if the background is dark, autocontrast (cutoff 1%),
/// box-blur (radius 1), threshold, clear edge noise, take a bbox.
pub struct Binarized {
    /// The contrasted + blurred grayscale image (not yet thresholded) —
    /// [`page_number`]'s windowed digit-shape scan re-thresholds directly
    /// against this, matching upstream re-using the same intermediate
    /// image rather than the final binary mask.
    pub grayscale: GrayImage,
    pub binary: GrayImage,
    pub bbox: Option<Bbox>,
    pub threshold: f32,
}

/// `ignore_edge_noise` controls whether [`ignore_pixels_near_edge`] runs
/// before the bbox is taken: margin and page-number cropping want it
/// (matches `page_number_crop_alg.py`'s own preprocessing), inter-panel
/// cropping doesn't — it applies its own, distinct border exclusion instead
/// (see [`inter_panel`]'s module docs).
///
/// Known, accepted residual: on a real 186-page volume with a mix of
/// backgrounds, this function's own bbox already lands 1px off real KCC's
/// (before any page-number-specific narrowing even runs) for one recurring
/// page type — [`Background::Dark`] pages with dense text near an edge
/// (e.g. a stylized chapter-title/credits illustration). Confirmed by
/// diffing against a real KCC checkout instrumented to dump its own actual
/// per-page bbox; every other page (179/186, including every other
/// dark-background one) matches exactly. Not chased further: the boundary
/// row a 1px bbox difference includes or excludes is by definition right at
/// the edge between "content" and "background" already, the source image
/// is downscaled to the target device resolution right after this anyway
/// (diluting a 1px difference in an ~800px-tall source further still), and
/// this page shape — dark background *and* dense text right at an edge —
/// is rare outside of stylized splash pages, not the traditional
/// white-background manga this project targets. Revisit only if it turns
/// out to matter on real content, not as an exercise in exact parity for
/// its own sake.
pub fn binarize_for_crop(
    img: &GrayImage,
    power: f32,
    background: Background,
    ignore_edge_noise: bool,
) -> Binarized {
    let prepped = match background {
        Background::White => img.clone(),
        Background::Dark => {
            let mut inverted = img.clone();
            image::imageops::invert(&mut inverted);
            inverted
        }
    };

    let grayscale = box_blur_radius1(&crate::contrast::autocontrast_cutoff(&prepped, 1));
    let threshold = threshold_from_power(power);
    let mut binary = threshold_binary(&grayscale, threshold);
    if ignore_edge_noise {
        ignore_pixels_near_edge(&mut binary);
    }
    let bbox = get_bbox(&binary);

    Binarized {
        grayscale,
        binary,
        bbox,
        threshold,
    }
}

/// `ImageFilter.BoxBlur(1)`. Confirmed empirically against real Pillow
/// (not assumed) that this is a *separable* two-pass 1D blur (horizontal
/// then vertical, each averaging 3 clamped-at-the-edge samples and
/// rounding to the nearest integer), not a single-pass 2D 3x3 convolution.
/// The two are mathematically equivalent for interior pixels, but diverge
/// near the image edges — confirmed to matter in practice: this divergence,
/// compounded through [`threshold_binary`] and [`ignore_pixels_near_edge`],
/// was shifting real page crop boundaries by as much as ~20px (the width of
/// the edge-noise strip `ignore_pixels_near_edge` checks) whenever a pixel
/// near the border landed on the wrong side of the threshold. A window of
/// exactly 3 samples can never average to a `.5` boundary (that would need
/// a non-integer sum), so plain `f64::round()` can't disagree with
/// Pillow's own rounding here regardless of tie-breaking rule.
fn box_blur_radius1(img: &GrayImage) -> GrayImage {
    let (w, h) = img.dimensions();
    let horizontal = GrayImage::from_fn(w, h, |x, y| {
        let x0 = x.saturating_sub(1);
        let x2 = (x + 1).min(w - 1);
        let sum = img.get_pixel(x0, y)[0] as u32
            + img.get_pixel(x, y)[0] as u32
            + img.get_pixel(x2, y)[0] as u32;
        Luma([(sum as f64 / 3.0).round() as u8])
    });
    GrayImage::from_fn(w, h, |x, y| {
        let y0 = y.saturating_sub(1);
        let y2 = (y + 1).min(h - 1);
        let sum = horizontal.get_pixel(x, y0)[0] as u32
            + horizontal.get_pixel(x, y)[0] as u32
            + horizontal.get_pixel(x, y2)[0] as u32;
        Luma([(sum as f64 / 3.0).round() as u8])
    })
}

fn threshold_binary(img: &GrayImage, threshold: f32) -> GrayImage {
    GrayImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y)[0] as f32;
        if p <= threshold {
            Luma([255])
        } else {
            Luma([0])
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn default_power_matches_kcc_default() {
        // KCC's --croppingpower default is 1.0 -> threshold 176.
        assert_eq!(threshold_from_power(1.0), 176.0);
    }

    #[test]
    fn get_bbox_of_blank_image_is_none() {
        let img = GrayImage::from_pixel(20, 20, Luma([0]));
        assert_eq!(get_bbox(&img), None);
    }

    #[test]
    fn get_bbox_finds_tight_rectangle() {
        let mut img = GrayImage::from_pixel(20, 20, Luma([0]));
        for y in 5..10 {
            for x in 3..8 {
                img.put_pixel(x, y, Luma([255]));
            }
        }
        assert_eq!(
            get_bbox(&img),
            Some(Bbox {
                left: 3,
                top: 5,
                right: 8,
                bottom: 10
            })
        );
    }

    // 1000x1000 so 2%/2.5% of each dimension round to different pixel
    // counts (20 vs 25) — on a too-small canvas upstream's own degenerate
    // guard skips the whole function, which would make these tests
    // vacuously pass no matter what they assert.
    const EDGE_TEST_DIM: u32 = 1000;

    #[test]
    fn ignore_pixels_near_edge_clears_sparse_border_noise() {
        let mut img = GrayImage::from_pixel(EDGE_TEST_DIM, EDGE_TEST_DIM, Luma([0]));
        // A single stray foreground pixel right at the raw top edge, with
        // nothing in the inner 2%-2.5% band just past it (density 0 there).
        // Upstream reads the empty inner band as "content doesn't reach
        // this far in", then wipes the whole outer edge strip because it
        // has *any* foreground pixel at all.
        img.put_pixel(500, 0, Luma([255]));
        ignore_pixels_near_edge(&mut img);
        assert_eq!(img.get_pixel(500, 0)[0], 0);
    }

    #[test]
    fn ignore_pixels_near_edge_keeps_dense_border_content() {
        let mut img = GrayImage::from_pixel(EDGE_TEST_DIM, EDGE_TEST_DIM, Luma([0]));
        // Fill the top edge strip *and* the inner 2%-2.5% band just past it
        // — real content reaching that far in, not scan noise, so upstream
        // must leave both untouched.
        for y in 0..30 {
            for x in 0..EDGE_TEST_DIM {
                img.put_pixel(x, y, Luma([255]));
            }
        }
        ignore_pixels_near_edge(&mut img);
        assert_eq!(img.get_pixel(500, 0)[0], 255);
    }

    #[test]
    fn large_margin_is_capped_to_ten_percent() {
        let bbox = Bbox {
            left: 60,
            top: 90,
            right: 140,
            bottom: 210,
        };
        let crop = apply_policy(bbox, (200, 300), &CropPolicy::default())
            .expect("a crop should be produced");
        assert_eq!(
            crop,
            CropBox {
                left: 20,
                top: 30,
                right: 180,
                bottom: 270
            }
        );
    }

    #[test]
    fn minimum_area_ratio_suppresses_crop_when_too_aggressive() {
        let bbox = Bbox {
            left: 60,
            top: 90,
            right: 140,
            bottom: 210,
        };
        // The capped crop keeps (180-20)*(270-30)=38400 of 60000 px = 64%.
        let crop = apply_policy(
            bbox,
            (200, 300),
            &CropPolicy {
                minimum_area_ratio: 0.9,
                ..Default::default()
            },
        );
        assert_eq!(crop, None);
    }

    #[test]
    fn preserve_margin_backs_off_the_crop() {
        let full = CropBox {
            left: 20,
            top: 30,
            right: 180,
            bottom: 270,
        };
        let backed_off = apply_preserve_margin(full, (200, 300), 50.0);
        assert_eq!(
            backed_off,
            CropBox {
                left: 10,
                top: 15,
                right: 190,
                bottom: 285
            }
        );
    }

    #[test]
    fn preserve_margin_zero_is_identity() {
        let full = CropBox {
            left: 20,
            top: 30,
            right: 180,
            bottom: 270,
        };
        assert_eq!(apply_preserve_margin(full, (200, 300), 0.0), full);
    }

    #[test]
    fn apply_crop_produces_expected_dimensions() {
        let img = GrayImage::from_pixel(200, 300, Luma([255]));
        let cropped = apply_crop(
            &img,
            CropBox {
                left: 20,
                top: 30,
                right: 180,
                bottom: 270,
            },
        );
        assert_eq!(cropped.dimensions(), (160, 240));
    }
}
