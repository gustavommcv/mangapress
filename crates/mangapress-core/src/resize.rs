//! Resize filter selection and execution.
//!
//! Upstream reference: `resize_method()` and `resizeImage()` in KCC's `image.py`
//! (GPLv3-licensed upstream — reimplement from this spec, do not copy; see
//! `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`). Out of scope from
//! `resizeImage()`: the `--kfx` branch (KFX isn't a supported output format
//! here — see `ebook` module docs) and the `--norotate` early-return special
//! case for rotated-spread-variant pages (`targetPathOrder in ('-kcc-a',
//! '-kcc-d')`), which needs spread-output tagging that
//! [`crate::pipeline::spread`] doesn't produce yet.
//!
//! Filter rule (`resize_method`): if the source image already fits within
//! the target resolution on both axes, use bicubic (here: `CatmullRom`,
//! the `image` crate's closest equivalent); otherwise (the common case —
//! manga scans are almost always higher-res than e-ink screens) use
//! Lanczos3.
//!
//! Decision tree (`resizeImage`, KFX/norotate branches excluded), in
//! priority order:
//! 1. `--stretch`: plain resize to the exact target, ignoring aspect ratio.
//! 2. `--wallpaper`: crop-to-fill (`ImageOps.fit` equivalent) regardless of
//!    upscale settings.
//! 3. Filter is bicubic-equivalent (image already fits) and `--upscale`
//!    isn't set: leave the image untouched, even if smaller than target.
//! 4. Aspect ratio close enough to the device's (within
//!    [`ASPECT_MATCH_TOLERANCE`], tripled for the `KDX` profile
//!    specifically): crop-to-fill (`ImageOps.fit`).
//! 5. Output format is CBZ/PDF and `--whiteborders` isn't set: fit-within +
//!    pad the remainder with the page's detected background color
//!    (`ImageOps.pad`).
//! 6. Otherwise: fit-within, no padding (`ImageOps.contain`) — the result
//!    may be smaller than the target on one axis.

use image::imageops::FilterType;

/// An owned 8-bit image of any pixel layout: pages are grayscale unless
/// color output was asked for (`--forcecolor`), and then RGB.
type Buffer<P> = image::ImageBuffer<P, Vec<u8>>;

/// KCC's `AUTO_CROP_THRESHOLD`. The `KDX` profile uses `3.0 *` this value.
pub const ASPECT_MATCH_TOLERANCE: f64 = 0.015;

#[derive(Debug, Clone, Copy)]
pub struct ResizeOptions {
    pub target: (u32, u32),
    pub upscale: bool,
    pub stretch: bool,
    pub wallpaper: bool,
    /// `-p KDX`: gets a 3x wider aspect-match tolerance for historical
    /// reasons upstream doesn't explain further.
    pub is_kdx_profile: bool,
    /// Output format is CBZ or PDF (EPUB/MOBI go through `contain` instead
    /// — those formats can rely on reader-side letterboxing).
    pub pads_for_cbz_or_pdf: bool,
    /// `--whiteborders`: suppresses the CBZ/PDF padding path in favor of
    /// plain `contain`, even when the format would otherwise pad.
    pub white_borders: bool,
    /// The page's detected background color (`fillCheck()` upstream, see
    /// [`crate::fill_check`]), used as the pad fill: 255 for white, 0 for
    /// dark.
    pub fill: u8,
}

/// Picks Lanczos3 for downscaling, bicubic-equivalent for same-size/upscale
/// — matching `resize_method()`'s filter choice (not its early-return
/// no-upscale behavior, which [`resize_page`] applies separately).
pub fn choose_filter(src: (u32, u32), target: (u32, u32)) -> FilterType {
    let fits = src.0 <= target.0 && src.1 <= target.1;
    if fits {
        FilterType::CatmullRom
    } else {
        FilterType::Lanczos3
    }
}

/// `resizeImage()`'s main decision tree (KFX/norotate branches excluded —
/// see module docs).
pub fn resize_page<P: image::Pixel<Subpixel = u8> + 'static>(
    img: &Buffer<P>,
    options: &ResizeOptions,
) -> Buffer<P> {
    let src = img.dimensions();
    let filter = choose_filter(src, options.target);

    if options.stretch {
        return resample(img, options.target, filter, None);
    }
    if options.wallpaper {
        return fit(img, options.target, filter);
    }
    if filter == FilterType::CatmullRom && !options.upscale {
        return img.clone();
    }

    let device_aspect = options.target.1 as f64 / options.target.0 as f64;
    let page_aspect = src.1 as f64 / src.0 as f64;
    let diff = (page_aspect - device_aspect).abs();

    let kdx_tolerance_clears = options.is_kdx_profile && diff < ASPECT_MATCH_TOLERANCE * 3.0;
    if kdx_tolerance_clears || diff < ASPECT_MATCH_TOLERANCE {
        fit(img, options.target, filter)
    } else if options.pads_for_cbz_or_pdf && !options.white_borders {
        pad(img, options.target, filter, options.fill)
    } else {
        contain(img, options.target, filter)
    }
}

/// `ImageOps.contain()`: scale to fit entirely within `target`, preserving
/// aspect ratio. The result may be smaller than `target` on one axis — it
/// is never padded or cropped.
pub fn contain<P: image::Pixel<Subpixel = u8> + 'static>(
    img: &Buffer<P>,
    target: (u32, u32),
    filter: FilterType,
) -> Buffer<P> {
    let (nw, nh) = contain_dimensions(img.dimensions(), target);
    resample(img, (nw, nh), filter, None)
}

/// `ImageOps.pad()`: [`contain`], then pad with `fill` to exactly `target`,
/// centered.
pub fn pad<P: image::Pixel<Subpixel = u8> + 'static>(
    img: &Buffer<P>,
    target: (u32, u32),
    filter: FilterType,
    fill: u8,
) -> Buffer<P> {
    let contained = contain(img, target, filter);
    let (cw, ch) = contained.dimensions();
    let (tw, th) = target;
    // The same value on every channel: white or black, in gray or in RGB.
    let fill_pixel = *P::from_slice(&vec![fill; P::CHANNEL_COUNT as usize]);
    let mut out = Buffer::<P>::from_pixel(tw, th, fill_pixel);
    let x_off = round_half_even(tw.saturating_sub(cw) as f64 * 0.5);
    let y_off = round_half_even(th.saturating_sub(ch) as f64 * 0.5);
    image::imageops::overlay(&mut out, &contained, x_off as i64, y_off as i64);
    out
}

/// Round-half-to-even ("banker's rounding"), matching Python's `round()`.
/// `ImageOps.pad()`'s centering offset is `round((target - resized) * 0.5)`
/// in real Pillow, and every odd remainder here lands exactly on a `.5`
/// boundary (the value being rounded is always `(non-negative integer) *
/// 0.5`) -- so which rounding rule is used actually matters, not just a
/// style choice. A prior version of this function used plain truncating
/// integer division (`remainder / 2`) instead of rounding at all, which
/// happened to agree with Pillow only when `floor(remainder / 2)` was even
/// and was off by one pixel otherwise. Confirmed directly against real
/// Pillow output, not assumed: a 400x201 image padded to 400x400 has a
/// 199px vertical remainder, and Pillow's `round(199 * 0.5)` -- `round(99.5)`
/// -- is `100` (nearest even), while the old `199 / 2` gave `99`.
pub(crate) fn round_half_even(x: f64) -> u32 {
    let floor = x.floor();
    let diff = x - floor;
    let rounded = if diff < 0.5 {
        floor
    } else if diff > 0.5 {
        floor + 1.0
    } else if (floor as i64).rem_euclid(2) == 0 {
        floor
    } else {
        floor + 1.0
    };
    rounded as u32
}

/// Resamples the way Pillow does (see [`crate::resample`]). The filter is
/// still named by the `image` crate's type, which is what this module's
/// callers and [`choose_filter`] speak: its Catmull-Rom is Pillow's bicubic,
/// its Lanczos3 Pillow's Lanczos.
pub(crate) fn resample<P: image::Pixel<Subpixel = u8> + 'static>(
    img: &Buffer<P>,
    size: (u32, u32),
    filter: FilterType,
    source_box: Option<[f64; 4]>,
) -> Buffer<P> {
    let filter = match filter {
        FilterType::CatmullRom => crate::resample::Filter::Bicubic,
        _ => crate::resample::Filter::Lanczos,
    };
    crate::resample::resize(img, size, filter, source_box)
}

/// `ImageOps.fit()`: scale so `target` is entirely filled (may exceed it on
/// one axis), then center-crop down to exactly `target`. No padding; may
/// crop away source content.
///
/// Done as Pillow does it, in one resampling pass from a *fractional*
/// source box — the centered part of the image with the target's
/// proportions, which rarely starts or ends on a whole pixel. Two earlier
/// versions got this wrong in turn: resizing the whole image and cropping
/// afterwards blended content from outside the crop into its edges (mean
/// error in the tens), and rounding the box to whole pixels before resizing
/// shifted the result by a fraction of a pixel, which the parity check
/// against real KCC measured at 2.4 gray levels on hatched and screentoned
/// art — a page whose cropped proportions land within upstream's tolerance
/// of the screen's takes this path, so it is not a rare one.
pub fn fit<P: image::Pixel<Subpixel = u8> + 'static>(
    img: &Buffer<P>,
    target: (u32, u32),
    filter: FilterType,
) -> Buffer<P> {
    let (sw, sh) = img.dimensions();
    let (sw_f, sh_f) = (sw as f64, sh as f64);
    let live_ratio = sw_f / sh_f;
    let out_ratio = target.0 as f64 / target.1 as f64;

    let (crop_w, crop_h) = if live_ratio == out_ratio {
        (sw_f, sh_f)
    } else if live_ratio >= out_ratio {
        (out_ratio * sh_f, sh_f)
    } else {
        (sw_f, sw_f / out_ratio)
    };
    let crop_left = (sw_f - crop_w) * 0.5;
    let crop_top = (sh_f - crop_h) * 0.5;

    resample(
        img,
        target,
        filter,
        Some([crop_left, crop_top, crop_left + crop_w, crop_top + crop_h]),
    )
}

/// The size `ImageOps.contain()` resizes to, computed the way Pillow
/// computes it rather than by an equivalent-looking shortcut: the limiting
/// axis takes the target's size outright, and the other one is
/// `round(other / limiting * target)` — in that order of operations, and
/// with Python's `round()` (half to even). An earlier version scaled both
/// axes by `min(tw / sw, th / sh)` and rounded half away from zero, which
/// agrees almost always and is a pixel off when the free axis lands on (or
/// within a float's error of) a `.5`: 94 of 409,500 source sizes checked
/// against Pillow's rule across five device resolutions, e.g. 720x1280 on a
/// 1072x1448 screen is 814x1448 in Pillow and was 815x1448 here.
fn contain_dimensions((sw, sh): (u32, u32), (tw, th): (u32, u32)) -> (u32, u32) {
    let (sw_f, sh_f) = (sw as f64, sh as f64);
    let image_ratio = sw_f / sh_f;
    let target_ratio = tw as f64 / th as f64;

    if image_ratio == target_ratio {
        (tw, th)
    } else if image_ratio > target_ratio {
        (tw, round_half_even(sh_f / sw_f * tw as f64).max(1))
    } else {
        (round_half_even(sw_f / sh_f * th as f64).max(1), th)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma};

    fn default_options(target: (u32, u32)) -> ResizeOptions {
        ResizeOptions {
            target,
            upscale: false,
            stretch: false,
            wallpaper: false,
            is_kdx_profile: false,
            pads_for_cbz_or_pdf: false,
            white_borders: false,
            fill: 255,
        }
    }

    #[test]
    fn downscale_uses_lanczos() {
        assert_eq!(
            choose_filter((2000, 3000), (1072, 1448)),
            FilterType::Lanczos3
        );
    }

    #[test]
    fn same_size_uses_bicubic_equivalent() {
        assert_eq!(
            choose_filter((1072, 1448), (1072, 1448)),
            FilterType::CatmullRom
        );
    }

    #[test]
    fn contain_preserves_aspect_and_fits_within_bounds() {
        let img = GrayImage::from_pixel(1000, 500, Luma([0])); // 2:1 landscape
        let out = contain(&img, (400, 400), FilterType::Lanczos3);
        // Width-limited: 400/1000 = 0.4 scale -> 400x200.
        assert_eq!(out.dimensions(), (400, 200));
    }

    #[test]
    fn round_half_even_matches_pythons_banker_rounding() {
        assert_eq!(round_half_even(99.5), 100); // nearest even is 100
        assert_eq!(round_half_even(100.5), 100); // nearest even is 100
        assert_eq!(round_half_even(3.0), 3); // no ambiguity
        assert_eq!(round_half_even(0.0), 0);
    }

    #[test]
    fn pad_centers_with_an_odd_remainder_like_pillow_does() {
        // 999x501 contained into 400x400 lands at 400x201 -- an odd
        // (199px) vertical remainder. Confirmed directly against real
        // Pillow (ImageOps.pad): the offset is 100, not floor(199/2)=99.
        let img = GrayImage::from_pixel(999, 501, Luma([0]));
        let out = pad(&img, (400, 400), FilterType::Lanczos3, 200);
        assert_eq!(out.dimensions(), (400, 400));
        assert_eq!(
            out.get_pixel(200, 99)[0],
            200,
            "row 99 should still be fill"
        );
        assert_eq!(
            out.get_pixel(200, 100)[0],
            0,
            "row 100 should already be content"
        );
    }

    #[test]
    fn pad_reaches_exact_target_with_fill_in_the_margins() {
        let img = GrayImage::from_pixel(1000, 500, Luma([0]));
        let out = pad(&img, (400, 400), FilterType::Lanczos3, 200);
        assert_eq!(out.dimensions(), (400, 400));
        // Content lands at rows 100..300 (centered 200-tall content in a
        // 400-tall canvas); the top margin should be pure fill.
        assert_eq!(out.get_pixel(200, 10)[0], 200);
        assert_eq!(out.get_pixel(200, 200)[0], 0);
    }

    #[test]
    fn fit_reaches_exact_target_with_no_fill_pixels() {
        let img = GrayImage::from_pixel(1000, 500, Luma([7]));
        let out = fit(&img, (400, 400), FilterType::Lanczos3);
        assert_eq!(out.dimensions(), (400, 400));
        // Every pixel is source content (7), never a fill color, since fit
        // crops rather than pads.
        assert!(out.pixels().all(|p| p[0] == 7));
    }

    #[test]
    fn fit_keeps_a_centered_marker_centered_under_an_aggressive_crop() {
        // Regression test for the crop-then-resize reordering: a wide
        // source needing its left/right thirds cropped away to fill a
        // square target should keep a marker at the source's exact
        // horizontal center still near the output's horizontal center --
        // the bug this replaced could shift/blend that framing badly
        // under exactly this kind of aspect-ratio mismatch.
        let (sw, sh) = (798u32, 501u32);
        let mut img = GrayImage::from_pixel(sw, sh, Luma([0]));
        for y in 0..sh {
            for x in (sw / 2 - 5)..(sw / 2 + 5) {
                img.put_pixel(x, y, Luma([255]));
            }
        }
        let out = fit(&img, (400, 400), FilterType::Lanczos3);
        assert_eq!(out.dimensions(), (400, 400));
        let brightest_col = (0..out.width())
            .max_by_key(|&x| out.get_pixel(x, 200)[0] as u32)
            .unwrap();
        assert!(
            (190..=210).contains(&brightest_col),
            "marker should land near the horizontal center, was at column {brightest_col}"
        );
    }

    #[test]
    fn small_image_is_left_untouched_without_upscale() {
        let img = GrayImage::from_pixel(500, 500, Luma([0]));
        let out = resize_page(&img, &default_options((1072, 1448)));
        assert_eq!(out.dimensions(), (500, 500));
    }

    #[test]
    fn small_image_is_upscaled_when_requested() {
        let img = GrayImage::from_pixel(500, 500, Luma([0]));
        let mut options = default_options((1072, 1448));
        options.upscale = true;
        let out = resize_page(&img, &options);
        assert_ne!(out.dimensions(), (500, 500));
    }

    #[test]
    fn stretch_always_resizes_to_the_exact_target() {
        let img = GrayImage::from_pixel(2000, 1000, Luma([0])); // very different aspect
        let mut options = default_options((1072, 1448));
        options.stretch = true;
        let out = resize_page(&img, &options);
        assert_eq!(out.dimensions(), (1072, 1448));
    }

    #[test]
    fn wallpaper_fits_regardless_of_upscale() {
        let img = GrayImage::from_pixel(500, 500, Luma([0]));
        let mut options = default_options((1072, 1448));
        options.wallpaper = true; // upscale left false
        let out = resize_page(&img, &options);
        assert_eq!(out.dimensions(), (1072, 1448));
    }

    #[test]
    fn matching_aspect_ratio_crops_to_fill_exact_target() {
        // 1072x1448 target ratio == a same-ratio-but-larger source.
        let img = GrayImage::from_pixel(2144, 2896, Luma([0]));
        let out = resize_page(&img, &default_options((1072, 1448)));
        assert_eq!(out.dimensions(), (1072, 1448));
    }

    #[test]
    fn mismatched_aspect_pads_for_cbz_output() {
        let img = GrayImage::from_pixel(2000, 1000, Luma([0])); // very wide
        let mut options = default_options((1072, 1448));
        options.pads_for_cbz_or_pdf = true;
        options.fill = 123;
        let out = resize_page(&img, &options);
        assert_eq!(out.dimensions(), (1072, 1448));
        // Top/bottom margins should carry the fill color for a wide source
        // padded into a tall target.
        assert_eq!(out.get_pixel(536, 5)[0], 123);
    }

    #[test]
    fn white_borders_suppresses_padding_in_favor_of_contain() {
        let img = GrayImage::from_pixel(2000, 1000, Luma([0]));
        let mut options = default_options((1072, 1448));
        options.pads_for_cbz_or_pdf = true;
        options.white_borders = true;
        let out = resize_page(&img, &options);
        // contain() on a very wide source into a tall target is
        // width-limited: dimensions won't reach the target height.
        assert!(out.dimensions().1 < 1448);
    }

    #[test]
    fn mismatched_aspect_contains_for_non_cbz_output() {
        let img = GrayImage::from_pixel(2000, 1000, Luma([0]));
        let out = resize_page(&img, &default_options((1072, 1448)));
        assert!(out.dimensions().1 < 1448);
    }

    #[test]
    fn kdx_profile_gets_a_wider_aspect_match_tolerance() {
        // A ratio difference that clears the normal tolerance but not the
        // KDX-widened one (3x).
        let (tw, th) = (824u32, 1000u32); // KDX resolution
        let device_aspect = th as f64 / tw as f64;
        // Pick a source ratio whose diff from device_aspect sits strictly
        // between the normal and KDX tolerances.
        let diff = ASPECT_MATCH_TOLERANCE * 2.0;
        let page_aspect = device_aspect + diff;
        let sw = 2000u32;
        let sh = (sw as f64 * page_aspect).round() as u32;
        let img = GrayImage::from_pixel(sw, sh, Luma([0]));

        let mut options = default_options((tw, th));
        options.is_kdx_profile = true;
        let kdx_out = resize_page(&img, &options);
        assert_eq!(kdx_out.dimensions(), (tw, th), "KDX should crop-to-fill");

        options.is_kdx_profile = false;
        let non_kdx_out = resize_page(&img, &options);
        assert_ne!(
            non_kdx_out.dimensions(),
            (tw, th),
            "non-KDX should not crop-to-fill at this ratio difference"
        );
    }

    #[test]
    fn contain_dimensions_match_pillows_rounding() {
        // Pillow's `ImageOps.contain()` size for each source on a 1072x1448
        // device; the first three are sizes the previous rule got one pixel
        // wrong.
        for (source, expected) in [
            ((720, 1280), (814, 1448)),
            ((608, 741), (1072, 1306)),
            ((510, 1632), (452, 1448)),
            ((900, 1350), (965, 1448)),
            ((1072, 1448), (1072, 1448)),
            ((2144, 2896), (1072, 1448)),
        ] {
            assert_eq!(
                contain_dimensions(source, (1072, 1448)),
                expected,
                "{source:?}"
            );
        }
    }
}
