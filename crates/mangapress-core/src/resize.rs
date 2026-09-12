//! Resize filter selection and execution.
//!
//! Port target: `resize_method()` and `resizeImage()` in KCC's `image.py`
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
use image::{GrayImage, Luma};

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
    /// The page's detected background color (`fillCheck()` upstream, not
    /// yet ported — see [`crate::crop::Background`]), used as the pad fill.
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
pub fn resize_page(img: &GrayImage, options: &ResizeOptions) -> GrayImage {
    let src = img.dimensions();
    let filter = choose_filter(src, options.target);

    if options.stretch {
        return image::imageops::resize(img, options.target.0, options.target.1, filter);
    }
    if options.wallpaper {
        return fit(img, options.target, filter);
    }
    if filter == FilterType::CatmullRom && !options.upscale {
        return img.clone();
    }

    let ratio_device = options.target.1 as f64 / options.target.0 as f64;
    let ratio_image = src.1 as f64 / src.0 as f64;
    let diff = (ratio_image - ratio_device).abs();

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
pub fn contain(img: &GrayImage, target: (u32, u32), filter: FilterType) -> GrayImage {
    let (nw, nh) = contain_dimensions(img.dimensions(), target);
    image::imageops::resize(img, nw, nh, filter)
}

/// `ImageOps.pad()`: [`contain`], then pad with `fill` to exactly `target`,
/// centered.
pub fn pad(img: &GrayImage, target: (u32, u32), filter: FilterType, fill: u8) -> GrayImage {
    let contained = contain(img, target, filter);
    let (cw, ch) = contained.dimensions();
    let (tw, th) = target;
    let mut out = GrayImage::from_pixel(tw, th, Luma([fill]));
    let x_off = tw.saturating_sub(cw) / 2;
    let y_off = th.saturating_sub(ch) / 2;
    image::imageops::overlay(&mut out, &contained, x_off as i64, y_off as i64);
    out
}

/// `ImageOps.fit()`: scale so `target` is entirely filled (may exceed it on
/// one axis), then center-crop down to exactly `target`. No padding; may
/// crop away source content.
pub fn fit(img: &GrayImage, target: (u32, u32), filter: FilterType) -> GrayImage {
    let (sw, sh) = img.dimensions();
    let (tw, th) = target;
    let scale = (tw as f64 / sw as f64).max(th as f64 / sh as f64);
    let new_w = ((sw as f64 * scale).round() as u32).max(tw).max(1);
    let new_h = ((sh as f64 * scale).round() as u32).max(th).max(1);
    let resized = image::imageops::resize(img, new_w, new_h, filter);
    let x_off = new_w.saturating_sub(tw) / 2;
    let y_off = new_h.saturating_sub(th) / 2;
    image::imageops::crop_imm(&resized, x_off, y_off, tw, th).to_image()
}

fn contain_dimensions((sw, sh): (u32, u32), (tw, th): (u32, u32)) -> (u32, u32) {
    let scale = (tw as f64 / sw as f64).min(th as f64 / sh as f64);
    (
        ((sw as f64 * scale).round() as u32).max(1),
        ((sh as f64 * scale).round() as u32).max(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let ratio_device = th as f64 / tw as f64;
        // Pick a source ratio whose diff from ratio_device sits strictly
        // between the normal and KDX tolerances.
        let diff = ASPECT_MATCH_TOLERANCE * 2.0;
        let ratio_image = ratio_device + diff;
        let sw = 2000u32;
        let sh = (sw as f64 * ratio_image).round() as u32;
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
}
