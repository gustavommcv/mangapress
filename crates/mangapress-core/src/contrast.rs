//! Gamma correction and autocontrast/autolevel.
//!
//! Upstream references in KCC's `image.py` (GPLv3 upstream — reimplemented from
//! documented/read behavior, not copied; see
//! `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`):
//! - `gammaCorrectImage()`: `pixel = 255 * (pixel/255)^gamma`. Comes from
//!   `-g/--gamma` if the user passes a value >= 0.1, else the profile's
//!   gamma (always `1.0` upstream today — see [`crate::profile`] — so this
//!   is a no-op unless the user overrides it).
//! - `autolevelImage()` (`--autolevel`, optional, runs first inside
//!   `autocontrastImage()` if enabled): sets the black point to the
//!   histogram's most-common value among the darkest 64 levels, clamping
//!   anything darker up to that floor.
//! - `autocontrastImage()`: `ImageOps.autocontrast(image, preserve_tone=true)`.
//!   Gated on the image's histogram extrema spanning at least 159 levels
//!   (`255 - 32*3`) — upstream treats an already-low-contrast page as
//!   intentional and leaves it alone. **Read Pillow's actual source** to
//!   confirm this precisely rather than assume: for a single-channel
//!   (grayscale) image specifically, `preserve_tone=True` and the plain
//!   cutoff=0 autocontrast compute the *exact same* lookup table (the
//!   `preserve_tone` branch only changes anything for multi-band/color
//!   images, where it computes one shared luminance-based LUT instead of a
//!   separate one per channel) — so this pipeline, being grayscale-only,
//!   just reuses [`autocontrast_cutoff`] with `cutoff = 0`, not a
//!   second implementation of the same math.
//!
//! Skipping autocontrast entirely for webtoon mode, `--noautocontrast`, or
//! color pages without `--colorautocontrast` is a pipeline-level decision
//! (which stage runs at all), not part of the algorithm itself — that gate
//! belongs in `pipeline::process_page`, not here.

use image::{GrayImage, Luma};

/// `gammaCorrectImage()`'s per-value mapping, `int(255 * (v / 255) ** gamma)`,
/// as a lookup table. Computed in `f64` like upstream's Python floats, from
/// the gamma's shortest decimal form rather than its `f32` bits: `1.8f32`
/// widened directly is `1.7999999523...`, not the `1.8` a user typed and
/// Python parsed, and that last-digit difference is enough to move a
/// handful of the 256 entries by one.
fn gamma_table(gamma: f32) -> [u8; 256] {
    let gamma: f64 = gamma.to_string().parse().unwrap_or(gamma as f64);
    let mut table = [0u8; 256];
    for (value, slot) in table.iter_mut().enumerate() {
        *slot = (255.0 * (value as f64 / 255.0).powf(gamma)) as u8;
    }
    table
}

/// `gammaCorrectImage()`. `gamma == 1.0` is a no-op (matching upstream's
/// explicit `if gamma == 1.0: pass`), returning a clone rather than
/// re-deriving an identity transform.
pub fn gamma_correct(page: &GrayImage, gamma: f32) -> GrayImage {
    if gamma == 1.0 {
        return page.clone();
    }
    let table = gamma_table(gamma);
    GrayImage::from_fn(page.width(), page.height(), |x, y| {
        Luma([table[page.get_pixel(x, y)[0] as usize]])
    })
}

/// [`gamma_correct`] on each channel of an RGB page — where upstream applies
/// it: before the page is converted to grayscale, not after. The two orders
/// agree on a neutral pixel and differ on a colored one.
pub fn gamma_correct_rgb(page: &image::RgbImage, gamma: f32) -> image::RgbImage {
    if gamma == 1.0 {
        return page.clone();
    }
    let table = gamma_table(gamma);
    let mut out = page.clone();
    for sample in out.iter_mut() {
        *sample = table[*sample as usize];
    }
    out
}

/// `ImageOps.autocontrast(image, preserve_tone=True)` on an RGB image: one
/// stretch for all three channels, taken from the darkest and lightest
/// values of the image's *grayscale*, so the colors keep their balance. An
/// image whose grayscale is a single value is left as it is. Used for the
/// cover, and for color pages kept in color.
pub fn autocontrast_preserving_tone(image: &image::RgbImage) -> image::RgbImage {
    let mut histogram = [0u32; 256];
    for pixel in crate::color::to_gray(image).pixels() {
        histogram[pixel[0] as usize] += 1;
    }
    let (Some(low), Some(high)) = (
        histogram.iter().position(|&count| count > 0),
        histogram.iter().rposition(|&count| count > 0),
    ) else {
        return image.clone();
    };
    if high <= low {
        return image.clone();
    }

    let scale = 255.0 / (high - low) as f64;
    let offset = -(low as f64) * scale;
    let mut table = [0u8; 256];
    for (value, slot) in table.iter_mut().enumerate() {
        *slot = ((value as f64 * scale + offset) as i32).clamp(0, 255) as u8;
    }

    let mut out = image.clone();
    for sample in out.iter_mut() {
        *sample = table[*sample as usize];
    }
    out
}

/// `autolevelImage()` on a color page: the black point is found and applied
/// on the luma channel alone — the page goes to YCbCr, every Y below the
/// most common of its 64 darkest values is raised to it, and it comes back.
/// The trip there and back is Pillow's own, losses included (see
/// [`crate::color::YCbCr`]).
pub fn autolevel_rgb(page: &image::RgbImage) -> image::RgbImage {
    let ycbcr = crate::color::YCbCr::new();
    let converted: Vec<[u8; 3]> = page
        .as_raw()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&pixel| ycbcr.from_rgb(pixel))
        .collect();

    let mut histogram = [0u32; 256];
    for pixel in &converted {
        histogram[pixel[0] as usize] += 1;
    }
    let mut black_point: u8 = 0;
    let mut best_count = histogram[0];
    for (level, &count) in histogram.iter().enumerate().take(64).skip(1) {
        if count > best_count {
            best_count = count;
            black_point = level as u8;
        }
    }

    let mut out = page.clone();
    for (sample, [y, cb, cr]) in out
        .as_mut()
        .as_chunks_mut::<3>()
        .0
        .iter_mut()
        .zip(converted)
    {
        *sample = ycbcr.to_rgb([y.max(black_point), cb, cr]);
    }
    out
}

/// `autocontrastImage()` for a color page kept in color: the same
/// low-contrast guard as [`autocontrast`], judged on the page's grayscale,
/// then [`autolevel_rgb`] if asked for and [`autocontrast_preserving_tone`].
pub fn autocontrast_rgb(page: &image::RgbImage, apply_autolevel: bool) -> image::RgbImage {
    let (min, max) = extrema(&crate::color::to_gray(page));
    if (max as i32 - min as i32) < 255 - 32 * 3 {
        return page.clone();
    }
    let leveled = if apply_autolevel {
        autolevel_rgb(page)
    } else {
        page.clone()
    };
    autocontrast_preserving_tone(&leveled)
}

/// `autolevelImage()` (grayscale path only — upstream's `self.color` branch
/// converts to YCbCr and only touches the Y channel, which doesn't apply
/// here). Finds the darkest-64-level bin with the highest pixel count (the
/// *first* such bin if there's a tie, matching Python's `list.index()`
/// semantics — `Iterator::max_by_key` alone would pick the *last* tied
/// bin, which is why this isn't written as one), then clamps every pixel
/// darker than that floor up to it.
pub fn autolevel(page: &GrayImage) -> GrayImage {
    let mut histogram = [0u32; 256];
    for p in page.pixels() {
        histogram[p[0] as usize] += 1;
    }

    let mut black_point: u8 = 0;
    let mut best_count = histogram[0];
    for (i, &count) in histogram.iter().enumerate().take(64).skip(1) {
        if count > best_count {
            best_count = count;
            black_point = i as u8;
        }
    }

    GrayImage::from_fn(page.width(), page.height(), |x, y| {
        let p = page.get_pixel(x, y)[0];
        Luma([p.max(black_point)])
    })
}

/// `autocontrastImage()`. Skips entirely (returns a clone) if the page's
/// grayscale extrema already span less than 159 levels — assumed
/// intentional low contrast, not something to "fix". Otherwise, optionally
/// autolevels first (`--autolevel`), then applies the same math as
/// [`autocontrast_cutoff`] with no cutoff (see this module's top-level
/// docs for why that's the correct equivalent of `preserve_tone=True` for
/// a single-channel image).
pub fn autocontrast(page: &GrayImage, apply_autolevel: bool) -> GrayImage {
    let (min, max) = extrema(page);
    if (max as i32 - min as i32) < 255 - 32 * 3 {
        return page.clone();
    }

    let leveled = if apply_autolevel {
        autolevel(page)
    } else {
        page.clone()
    };
    autocontrast_cutoff(&leveled, 0)
}

fn extrema(page: &GrayImage) -> (u8, u8) {
    let mut min = 255u8;
    let mut max = 0u8;
    for p in page.pixels() {
        min = min.min(p[0]);
        max = max.max(p[0]);
    }
    (min, max)
}

/// `ImageOps.autocontrast(image, cutoff)` (non-`preserve_tone` variant) —
/// used by the crop algorithms with `cutoff = 1`, not the general per-page
/// pipeline (which uses the `preserve_tone` variant in [`autocontrast`]
/// above; the two are genuinely different PIL call sites upstream, not the
/// same operation with a different name).
///
/// Algorithm (PIL's `ImageOps.autocontrast`, cutoff branch): build a 256-bin
/// histogram, trim `cutoff` percent of pixels off each end (by count, not by
/// value), find the remaining lowest/highest non-empty bins, and linearly
/// remap `[lo, hi] -> [0, 255]`. If nothing remains after trimming (a
/// flat/blank image), the image is returned unchanged rather than divide by
/// zero.
pub fn autocontrast_cutoff(img: &GrayImage, cutoff_percent: u32) -> GrayImage {
    let mut histogram = [0u32; 256];
    for pixel in img.pixels() {
        histogram[pixel[0] as usize] += 1;
    }

    let total: u32 = histogram.iter().sum();
    let mut trimmed = histogram;

    if cutoff_percent > 0 && total > 0 {
        let mut cut = total * cutoff_percent / 100;
        for bin in trimmed.iter_mut() {
            if cut == 0 {
                break;
            }
            if cut > *bin {
                cut -= *bin;
                *bin = 0;
            } else {
                *bin -= cut;
                cut = 0;
            }
        }

        let mut cut = total * cutoff_percent / 100;
        for bin in trimmed.iter_mut().rev() {
            if cut == 0 {
                break;
            }
            if cut > *bin {
                cut -= *bin;
                *bin = 0;
            } else {
                *bin -= cut;
                cut = 0;
            }
        }
    }

    let lo = trimmed.iter().position(|&count| count > 0);
    let hi = trimmed.iter().rposition(|&count| count > 0);

    let (lo, hi) = match (lo, hi) {
        (Some(lo), Some(hi)) if hi > lo => (lo, hi),
        _ => return img.clone(),
    };

    let scale = 255.0 / (hi - lo) as f64;
    let offset = -(lo as f64) * scale;
    let mut lut = [0u8; 256];
    for (ix, slot) in lut.iter_mut().enumerate() {
        let value = (ix as f64 * scale + offset) as i32;
        *slot = value.clamp(0, 255) as u8;
    }

    image::GrayImage::from_fn(img.width(), img.height(), |x, y| {
        image::Luma([lut[img.get_pixel(x, y)[0] as usize]])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_image_is_unchanged() {
        let img = GrayImage::from_pixel(4, 4, image::Luma([128]));
        let out = autocontrast_cutoff(&img, 1);
        assert_eq!(out, img);
    }

    #[test]
    fn stretches_a_narrow_range_to_full_scale() {
        // Values 100..=150 only, no cutoff: should stretch to roughly 0..=255.
        let mut img = GrayImage::new(51, 1);
        for x in 0..51u32 {
            img.put_pixel(x, 0, image::Luma([100 + x as u8]));
        }
        let out = autocontrast_cutoff(&img, 0);
        assert_eq!(out.get_pixel(0, 0)[0], 0);
        assert_eq!(out.get_pixel(50, 0)[0], 255);
    }

    #[test]
    fn cutoff_trims_outlier_pixels_before_stretching() {
        // Mostly mid-gray, with a handful of near-black/near-white outlier
        // pixels that a cutoff should discard before computing lo/hi.
        let mut img = GrayImage::from_pixel(10, 10, image::Luma([128]));
        img.put_pixel(0, 0, image::Luma([0]));
        img.put_pixel(1, 0, image::Luma([255]));
        // With cutoff=0, the outliers alone would define lo=0/hi=255 and
        // leave the mid-gray fill essentially unmoved (already central).
        let uncut = autocontrast_cutoff(&img, 0);
        assert_eq!(uncut.get_pixel(5, 5)[0], 128);
        // With a large enough cutoff, the 2 outlier pixels (2% of 100) are
        // trimmed and the remaining flat 128 fill has no range to stretch.
        let cut = autocontrast_cutoff(&img, 5);
        assert_eq!(cut.get_pixel(5, 5)[0], 128);
    }

    #[test]
    fn gamma_of_one_is_a_no_op() {
        let img = GrayImage::from_pixel(4, 4, image::Luma([77]));
        assert_eq!(gamma_correct(&img, 1.0), img);
    }

    #[test]
    fn gamma_below_one_brightens_midtones() {
        // gamma < 1 pushes values up: 255*(128/255)^0.5 ~= 180.
        let img = GrayImage::from_pixel(2, 2, image::Luma([128]));
        let out = gamma_correct(&img, 0.5);
        assert!(out.get_pixel(0, 0)[0] > 128);
    }

    #[test]
    fn gamma_above_one_darkens_midtones() {
        let img = GrayImage::from_pixel(2, 2, image::Luma([128]));
        let out = gamma_correct(&img, 2.0);
        assert!(out.get_pixel(0, 0)[0] < 128);
    }

    #[test]
    fn gamma_preserves_black_and_white_endpoints() {
        let img = GrayImage::from_fn(2, 1, |x, _| image::Luma([if x == 0 { 0 } else { 255 }]));
        let out = gamma_correct(&img, 2.2);
        assert_eq!(out.get_pixel(0, 0)[0], 0);
        assert_eq!(out.get_pixel(1, 0)[0], 255);
    }

    #[test]
    fn autolevel_clamps_pixels_below_the_common_dark_floor() {
        // Bin 20 dominates the darkest-64 range; a stray pixel at 5 should
        // get lifted up to 20, everything else stays put.
        let mut img = GrayImage::from_pixel(10, 10, image::Luma([20]));
        img.put_pixel(0, 0, image::Luma([5]));
        img.put_pixel(1, 0, image::Luma([200]));
        let out = autolevel(&img);
        assert_eq!(
            out.get_pixel(0, 0)[0],
            20,
            "pixel darker than the floor should be lifted"
        );
        assert_eq!(
            out.get_pixel(1, 0)[0],
            200,
            "pixel brighter than the floor is untouched"
        );
        assert_eq!(
            out.get_pixel(5, 5)[0],
            20,
            "pixels at the floor are untouched"
        );
    }

    #[test]
    fn autolevel_breaks_ties_by_taking_the_first_index() {
        // Bins 3 and 40 tie for the darkest-64 max count (2 pixels each);
        // Python's list.index() (and thus upstream's black_point) picks
        // the first (3), not the last (40) -- Iterator::max_by_key alone
        // would pick the last, which is why this needs its own test.
        let img = GrayImage::from_fn(5, 1, |x, _| {
            image::Luma([match x {
                0 | 1 => 3,
                2 | 3 => 40,
                _ => 1, // probe pixel, below both candidate floors
            }])
        });
        let out = autolevel(&img);
        assert_eq!(
            out.get_pixel(4, 0)[0],
            3,
            "black point should resolve to the first tied bin (3), not 40"
        );
    }

    #[test]
    fn autocontrast_skips_already_low_contrast_images() {
        // Extrema span only 50 levels (< 159 threshold) -- should be a no-op.
        let img = GrayImage::from_fn(2, 1, |x, _| image::Luma([if x == 0 { 100 } else { 150 }]));
        let out = autocontrast(&img, false);
        assert_eq!(out, img);
    }

    #[test]
    fn autocontrast_stretches_high_contrast_images() {
        let mut img = GrayImage::from_pixel(10, 10, image::Luma([128]));
        img.put_pixel(0, 0, image::Luma([50]));
        img.put_pixel(1, 0, image::Luma([220]));
        let out = autocontrast(&img, false);
        // extrema (50, 220) span 170 >= 159, so this should stretch --
        // confirm it actually changed rather than passing through.
        assert_ne!(out, img);
    }

    #[test]
    fn autocontrast_applies_autolevel_first_when_requested() {
        let mut img = GrayImage::from_pixel(10, 10, image::Luma([30]));
        img.put_pixel(0, 0, image::Luma([2])); // stray dark outlier
        img.put_pixel(1, 0, image::Luma([220])); // wide enough extrema to pass the gate
        let without_autolevel = autocontrast(&img, false);
        let with_autolevel = autocontrast(&img, true);
        assert_ne!(
            without_autolevel, with_autolevel,
            "enabling --autolevel should change the result"
        );
    }

    #[test]
    fn gamma_table_matches_pythons_arithmetic() {
        // int(255 * (v / 255.) ** gamma) in Python, for each v below.
        let values = [0usize, 1, 64, 128, 200, 254, 255];
        let table = gamma_table(1.8);
        assert_eq!(values.map(|v| table[v]), [0, 0, 21, 73, 164, 253, 255]);
        let table = gamma_table(0.5);
        assert_eq!(values.map(|v| table[v]), [0, 15, 127, 180, 225, 254, 255]);
    }

    #[test]
    fn rgb_gamma_treats_each_channel_like_the_gray_path() {
        let rgb = image::RgbImage::from_pixel(2, 2, image::Rgb([64, 128, 200]));
        let out = gamma_correct_rgb(&rgb, 1.8);
        assert_eq!(out.get_pixel(0, 0).0, [21, 73, 164]);
        assert_eq!(gamma_correct_rgb(&rgb, 1.0), rgb);
    }

    /// Eight pixels with a clear black point (three at 10), one below it,
    /// and three saturated colors.
    fn small_color_image() -> image::RgbImage {
        let pixels: [[u8; 3]; 8] = [
            [10, 10, 10],
            [10, 10, 10],
            [10, 10, 10],
            [30, 20, 10],
            [200, 100, 50],
            [5, 5, 5],
            [250, 250, 250],
            [120, 200, 90],
        ];
        image::RgbImage::from_fn(4, 2, |x, y| image::Rgb(pixels[(y * 4 + x) as usize]))
    }

    fn pixels(image: &image::RgbImage) -> Vec<[u8; 3]> {
        image.pixels().map(|p| p.0).collect()
    }

    #[test]
    fn color_autolevel_matches_upstreams_ycbcr_round_trip() {
        // Upstream's own steps in real Pillow: to YCbCr, raise Y to the black
        // point (10 here), back to RGB. Note the saturated pixels shifting by
        // a level or two — that is the round trip's loss, part of the result.
        assert_eq!(
            pixels(&autolevel_rgb(&small_color_image())),
            [
                [10, 10, 10],
                [10, 10, 10],
                [10, 10, 10],
                [28, 19, 8],
                [199, 99, 49],
                [10, 10, 10],
                [250, 250, 250],
                [118, 200, 88],
            ]
        );
    }

    #[test]
    fn tone_preserving_autocontrast_matches_pillows() {
        // ImageOps.autocontrast(image, preserve_tone=True) in real Pillow.
        assert_eq!(
            pixels(&autocontrast_preserving_tone(&small_color_image())),
            [
                [5, 5, 5],
                [5, 5, 5],
                [5, 5, 5],
                [26, 15, 5],
                [202, 98, 46],
                [0, 0, 0],
                [255, 255, 255],
                [119, 202, 88],
            ]
        );
    }
}
