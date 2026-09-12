//! Gamma correction and autocontrast/autolevel.
//!
//! Port targets in KCC's `image.py`:
//! - `gammaCorrectImage()`: `pixel = 255 * (pixel/255)^gamma`. Comes from
//!   `-g/--gamma` if the user passes a value >= 0.1, else the profile's
//!   gamma (always `1.0` upstream today — see [`crate::profile`] — so this
//!   is a no-op unless the user overrides it).
//! - `autolevelImage()` (`--autolevel`, optional, runs first if enabled):
//!   sets the black point to the histogram's most-common low value.
//! - `autocontrastImage()`: `ImageOps.autocontrast(image, preserve_tone=true)`
//!   equivalent. Skipped for webtoon mode, `--noautocontrast`, color pages
//!   without `--colorautocontrast`, and pages whose histogram extrema are
//!   already extreme (upstream treats that as "nothing to gain").

use image::GrayImage;

pub fn gamma_correct(_page: &mut GrayImage, _gamma: f32) {
    todo!("apply 255*(v/255)^gamma per-pixel — trivial, but wants a fixture-based test first")
}

pub fn autolevel(_page: &mut GrayImage) {
    todo!("port autolevelImage against synthetic fixtures")
}

pub fn autocontrast(_page: &mut GrayImage) {
    todo!("port autocontrastImage (preserve_tone autocontrast) against synthetic fixtures")
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
}
