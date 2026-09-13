//! Grayscale palette quantization with dithering (`--forcepng` output path
//! — the default JPEG path ships full 8-bit tone, undithered).
//!
//! Port target: `quantizeImage()` in KCC's `image.py` (GPLv3 upstream —
//! reimplemented from documented behavior, not copied; see
//! `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`). Upstream calls
//! Pillow's `Image.quantize(palette=palImg)` with no explicit `dither=`
//! argument, which defaults to Floyd-Steinberg error diffusion — so
//! quantization is dithered implicitly, not by an explicit KCC choice.
//! Since every profile here is grayscale-only (see
//! [`crate::profile::Palette`]), this only needs to quantize the L channel
//! to N fixed gray levels, not general RGB/adaptive-palette quantization —
//! no need for a full color-quantization crate.
//!
//! Confirmed empirically (see `docs/adr/`): comparing a real KCC-converted
//! volume against mangapress's output, KCC's *actual* default codec for
//! this content was a quantized GIF, not full-tone JPEG — upstream's
//! `save_with_codec()` only picks that path when `--forcepng` combines
//! with a Kindle-AZW3-specific GIF branch this project doesn't support
//! (MOBI/AZW3 is out of scope, see
//! `docs/adr/0008-mobi-azw3-permanently-out-of-scope.md`);
//! the PNG branch (relevant here) doesn't depend on that. The algorithm
//! below is the same either way — only the container format differs.

use crate::profile::Palette;
use image::{GrayImage, Luma};

/// Floyd-Steinberg error-diffusion dithering while quantizing to `palette`'s
/// fixed gray levels. Every output pixel is guaranteed to be one of
/// `palette.level_values()` — the accumulated error only ever influences
/// *which* palette value a later pixel rounds to, never appears in the
/// output directly.
pub fn quantize_with_floyd_steinberg(page: &GrayImage, palette: Palette) -> GrayImage {
    let levels = palette.level_values();
    let (w, h) = page.dimensions();
    let (w_i, h_i) = (w as i64, h as i64);
    let mut buffer: Vec<f32> = page.pixels().map(|p| p[0] as f32).collect();

    for y in 0..h_i {
        for x in 0..w_i {
            let idx = (y * w_i + x) as usize;
            let old_value = buffer[idx].clamp(0.0, 255.0);
            let new_value = nearest_palette_value(old_value, levels);
            buffer[idx] = new_value as f32;
            let error = old_value - new_value as f32;

            // Floyd-Steinberg kernel: 7/16 right, 3/16 below-left, 5/16
            // below, 1/16 below-right -- only pixels not yet processed.
            for (dx, dy, factor) in [
                (1i64, 0i64, 7.0f32 / 16.0),
                (-1, 1, 3.0 / 16.0),
                (0, 1, 5.0 / 16.0),
                (1, 1, 1.0 / 16.0),
            ] {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && nx < w_i && ny >= 0 && ny < h_i {
                    let nidx = (ny * w_i + nx) as usize;
                    buffer[nidx] += error * factor;
                }
            }
        }
    }

    GrayImage::from_fn(w, h, |x, y| {
        Luma([buffer[(y as i64 * w_i + x as i64) as usize] as u8])
    })
}

fn nearest_palette_value(value: f32, levels: &[u8]) -> u8 {
    levels
        .iter()
        .copied()
        .min_by(|&a, &b| {
            let da = (a as f32 - value).abs();
            let db = (b as f32 - value).abs();
            da.partial_cmp(&db).unwrap()
        })
        .expect("palette level lists are never empty")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_palette_value_picks_the_closest_level() {
        // Gray16 levels are 0,17,34,...,255 (step 17). 100 sits between
        // 85 and 102, closer to 102.
        assert_eq!(
            nearest_palette_value(100.0, Palette::Gray16.level_values()),
            102
        );
    }

    #[test]
    fn gray15s_gap_is_respected_during_quantization() {
        // 230 sits between Gray16's 221 and 238, but Gray15 has no 238 --
        // nearest available is 221, not the naive halfway point.
        assert_eq!(
            nearest_palette_value(230.0, Palette::Gray15.level_values()),
            0xdd
        );
    }

    #[test]
    fn every_output_pixel_is_an_exact_palette_value() {
        let img = GrayImage::from_fn(32, 32, |x, y| Luma([((x * 8 + y) % 256) as u8]));
        let quantized = quantize_with_floyd_steinberg(&img, Palette::Gray16);
        let allowed = Palette::Gray16.level_values();
        assert!(quantized.pixels().all(|p| allowed.contains(&p[0])));
    }

    #[test]
    fn flat_image_at_an_exact_palette_value_is_unchanged() {
        let img = GrayImage::from_pixel(16, 16, Luma([0x88]));
        let quantized = quantize_with_floyd_steinberg(&img, Palette::Gray16);
        assert!(quantized.pixels().all(|p| p[0] == 0x88));
    }

    #[test]
    fn dithering_mixes_two_levels_to_approximate_an_in_between_value() {
        // 8 is roughly halfway between palette levels 0 and 17. Without
        // error diffusion every pixel would round to 0 (nearest to 8).
        // With it, accumulated error should push some pixels to 17,
        // producing a mix of both rather than a flat single value.
        let img = GrayImage::from_pixel(40, 40, Luma([8]));
        let quantized = quantize_with_floyd_steinberg(&img, Palette::Gray16);
        let has_zero = quantized.pixels().any(|p| p[0] == 0);
        let has_next_level = quantized.pixels().any(|p| p[0] == 0x11);
        assert!(has_zero, "expected some pixels to round down to 0");
        assert!(
            has_next_level,
            "expected error diffusion to push some pixels up to the next level (0x11)"
        );
    }

    #[test]
    fn dithered_average_approximates_the_source_value() {
        // Over a large-enough flat region, the average of the dithered
        // output should land close to the original (unditherable) value --
        // that's the entire point of error diffusion.
        let source_value = 40.0;
        let img = GrayImage::from_pixel(64, 64, Luma([source_value as u8]));
        let quantized = quantize_with_floyd_steinberg(&img, Palette::Gray16);
        let sum: u64 = quantized.pixels().map(|p| p[0] as u64).sum();
        let avg = sum as f64 / (64 * 64) as f64;
        assert!(
            (avg - source_value).abs() < 3.0,
            "dithered average {avg} should be close to source value {source_value}"
        );
    }
}
